
use smithay::utils::IsAlive;

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    desktop::{
        LayerSurface, PopupKeyboardGrab, PopupKind, PopupPointerGrab, PopupUngrabStrategy, Window,
        WindowSurfaceType, find_popup_root_surface, get_popup_toplevel_coords, layer_map_for_output,
    },
    input::{Seat, pointer::Focus},
    output::Output,
    reexports::{
        calloop::Interest,
        wayland_protocols::xdg::{
            decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
            shell::server::xdg_toplevel,
        },
        wayland_server::{
            Client, Resource,
            protocol::{wl_buffer::WlBuffer, wl_output, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Logical, Point, Rectangle, Serial, Size},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            BufferAssignment, CompositorClientState, CompositorHandler, SurfaceAttributes, TraversalAction,
            add_blocker, add_pre_commit_hook, get_parent, is_sync_subsurface, with_states,
            with_surface_tree_upward,
        },
        dmabuf::get_dmabuf,
        shell::{
            wlr_layer::{
                LayerSurface as WlrLayerSurface, LayerSurfaceConfigure, LayerSurfaceData, WlrLayerShellHandler,
            },
            xdg::{
                Configure, PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler,
            },
        },
    },
};

use crate::{
    layout::LayoutMode,
    state::{ClientState, KyonState},
    window::WindowElement,
};

impl<BackendData: crate::state::Backend + 'static> BufferHandler for KyonState<BackendData> {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl<BackendData: crate::state::Backend + 'static> CompositorHandler for KyonState<BackendData> {
    fn compositor_state(&mut self) -> &mut smithay::wayland::compositor::CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(state) = client.get_data::<ClientState>() {
            return &state.compositor_state;
        }
        panic!("unknown client data")
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        add_pre_commit_hook::<Self, _>(surface, move |state, _dh, surface| {
            let maybe_dmabuf = with_states(surface, |data| {
                data.cached_state
                    .get::<SurfaceAttributes>()
                    .pending()
                    .buffer
                    .as_ref()
                    .and_then(|assignment| match assignment {
                        BufferAssignment::NewBuffer(buffer) => get_dmabuf(buffer).cloned().ok(),
                        _ => None,
                    })
            });
            if let Some(dmabuf) = maybe_dmabuf {
                if let Ok((blocker, source)) = dmabuf.generate_blocker(Interest::READ) {
                    if let Some(client) = surface.client() {
                        let res = state.handle.insert_source(source, move |_, _, data| {
                            let dh = data.display_handle.clone();
                            data.client_compositor_state(&client).blocker_cleared(data, &dh);
                            Ok(())
                        });
                        if res.is_ok() {
                            add_blocker(surface, blocker);
                        }
                    }
                }
            }
        });
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        self.backend_data.early_import(surface);

        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_for_surface(&root) {
                window.0.on_commit();
                if &root == surface {
                    let buffer_offset = with_states(surface, |states| {
                        states
                            .cached_state
                            .get::<SurfaceAttributes>()
                            .current()
                            .buffer_delta
                            .take()
                    });
                    if let Some(buffer_offset) = buffer_offset {
                        let loc = self
                            .active_workspace()
                            .space
                            .element_location(&window)
                            .unwrap_or_default();
                        self.active_workspace_mut()
                            .space
                            .relocate_element(&window, loc + buffer_offset);
                    }
                }
                self.refresh_window_identity(&window);
                self.ensure_initial_configure(surface);
                if !self.focused.as_ref().is_some_and(|f| f.alive()) {
                    self.focused = Some(window.clone());
                }
            } else {
                self.ensure_initial_configure(surface);
            }
        }

        self.popups.commit(surface);
        if matches!(&self.cursor_status, smithay::input::pointer::CursorImageStatus::Surface(s) if s == surface) {
            with_states(surface, |states| {
                if let Some(attrs) = states.data_map.get::<smithay::input::pointer::CursorImageSurfaceData>() {
                    if let Some(mut attrs) = attrs.lock().ok() {
                        let delta = states
                            .cached_state
                            .get::<SurfaceAttributes>()
                            .current()
                            .buffer_delta
                            .take();
                        if let Some(delta) = delta {
                            attrs.hotspot -= delta;
                        }
                    }
                }
            });
        }
        if matches!(&self.dnd_icon, Some(icon) if icon.surface == *surface) {
            let icon = self.dnd_icon.as_mut().unwrap();
            with_states(&icon.surface, |states| {
                let delta = states
                    .cached_state
                    .get::<SurfaceAttributes>()
                    .current()
                    .buffer_delta
                    .take()
                    .unwrap_or_default();
                icon.offset += delta;
            });
        }
    }
}

impl<BackendData: crate::state::Backend + 'static> XdgShellHandler for KyonState<BackendData> {
    fn xdg_shell_state(&mut self) -> &mut smithay::wayland::shell::xdg::XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        tracing::debug!("new xdg toplevel");
        let window = WindowElement(Window::new_wayland_window(surface.clone()));
        let mut meta = window.meta();
        meta.workspace = self.active_workspace;
        meta.ssd = true;
        let floating = self.active_workspace().mode == LayoutMode::Floating;
        meta.floating = floating;
        let cascade = self.active_workspace().order.len() as i32 % 8;
        let target_output = self
            .output_under(self.pointer_location)
            .or_else(|| self.primary_output());
        if let Some(output) = target_output {
            meta.output = output.name().to_string();
            let area = self.usable_area(&output);
            let w = (area.size.w as f64 * 0.62) as i32;
            let h = (area.size.h as f64 * 0.68) as i32;
            meta.saved_float = Some(Rectangle::new(
                (
                    area.loc.x + self.config.decor.gaps_out + 34 * cascade,
                    area.loc.y + self.config.decor.gaps_out + 28 + 28 * cascade,
                )
                    .into(),
                smithay::utils::Size::from((w.max(280), h.max(180))),
            ));
        }
        meta.outer = meta.saved_float.unwrap_or(Rectangle::from_size((800, 600).into()));
        drop(meta);
        self.refresh_window_identity(&window);
        self.apply_window_rules(&window);
        self.active_workspace_mut().order.push(window.clone());
        self.active_workspace_mut().focused = Some(window.clone());
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.focused = Some(window.clone());
        window.set_activated(true);
        self.arrange_active();
        let keyboard = self.seat.get_keyboard().unwrap();
        keyboard.set_focus(self, window.wl_surface(), serial);
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        if let Err(err) = self.popups.track_popup(PopupKind::from(surface)) {
            tracing::warn!(?err, "failed to track popup");
        }
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        if let Some(seat) = Seat::from_resource(&seat) {
            self.start_move(surface, &seat, serial);
        }
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        if let Some(seat) = Seat::from_resource(&seat) {
            self.start_resize(surface, &seat, serial, edges);
        }
    }

    fn ack_configure(&mut self, surface: WlSurface, configure: Configure) {
        if let Configure::Toplevel(configure) = configure {
            if let Some(window) = self.window_for_surface(&surface) {
                let is_ssd = configure
                    .state
                    .decoration_mode
                    .map(|mode| mode == DecorationMode::ServerSide)
                    .unwrap_or(false);
                let mut meta = window.meta();
                if meta.ssd != is_ssd {
                    meta.ssd = is_ssd;
                    drop(meta);
                    self.arrange_active();
                }
            }
        }
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, _output: Option<wl_output::WlOutput>) {
        if let Some(window) = self.window_for_surface(surface.wl_surface()) {
            let already = window.meta_ref().fullscreen;
            if !already {
                let mut meta = window.meta();
                meta.fullscreen = true;
                meta.maximized = false;
                drop(meta);
                self.fullscreen = Some(window.clone());
                surface.with_pending_state(|state| {
                    state.states.set(xdg_toplevel::State::Fullscreen);
                    if let Some(output) = self.primary_output() {
                        state.size = Some(self.output_geometry(&output).size);
                    }
                });
                if surface.is_initial_configure_sent() {
                    surface.send_pending_configure();
                }
                let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                self.focus_window(Some(window), serial);
                self.arrange_active();
            }
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for_surface(surface.wl_surface()) {
            window.meta().fullscreen = false;
            if self.fullscreen.as_ref() == Some(&window) {
                self.fullscreen = None;
            }
            surface.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Fullscreen);
                state.size = None;
            });
            if surface.is_initial_configure_sent() {
                surface.send_pending_configure();
            }
            self.arrange_active();
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for_surface(surface.wl_surface()) {
            let mut meta = window.meta();
            meta.fullscreen = false;
            meta.maximized = true;
            let floating = meta.floating;
            if floating {
                meta.saved_float = Some(meta.outer);
            }
            drop(meta);
            if self.fullscreen.as_ref() == Some(&window) {
                self.fullscreen = None;
            }
            let size = self
                .primary_output()
                .map(|output| {
                    let area = self.usable_area(&output);
                    let tb = crate::decor::titlebar_height(&self.config, &window.meta_ref(), self.active_workspace().mode);
                    Size::from(((area.size.w).max(1), (area.size.h - tb).max(1)))
                })
                .unwrap_or_else(|| Size::from((800, 600)));
            surface.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Maximized);
                state.states.unset(xdg_toplevel::State::Fullscreen);
                state.size = Some(size);
            });
            if surface.is_initial_configure_sent() {
                surface.send_pending_configure();
            }
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.focus_window(Some(window), serial);
            self.arrange_active();
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for_surface(surface.wl_surface()) {
            window.meta().maximized = false;
            window.meta().last_configure = None;
            surface.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Maximized);
                state.size = None;
            });
            if surface.is_initial_configure_sent() {
                surface.send_pending_configure();
            }
            self.arrange_active();
        }
    }

    fn minimize_request(&mut self, _surface: ToplevelSurface) {}

    fn show_window_menu(&mut self, _surface: ToplevelSurface, _seat: wl_seat::WlSeat, _serial: Serial, _location: Point<i32, Logical>) {}

    fn grab(&mut self, surface: PopupSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some(seat) = Seat::from_resource(&seat) else {
            return;
        };
        let kind = PopupKind::Xdg(surface);
        let Ok(root) = find_popup_root_surface(&kind) else {
            return;
        };
        let ret = self.popups.grab_popup(root, kind, &seat, serial);
        if let Ok(mut grab) = ret {
            if let Some(keyboard) = seat.get_keyboard() {
                if keyboard.is_grabbed()
                    && !(keyboard.has_grab(serial)
                        || keyboard.has_grab(grab.previous_serial().unwrap_or(serial)))
                {
                    grab.ungrab(PopupUngrabStrategy::All);
                    return;
                }
                keyboard.set_focus(self, grab.current_grab(), serial);
                keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
            }
            if let Some(pointer) = seat.get_pointer() {
                if pointer.is_grabbed()
                    && !(pointer.has_grab(serial)
                        || pointer.has_grab(grab.previous_serial().unwrap_or_else(|| grab.serial())))
                {
                    grab.ungrab(PopupUngrabStrategy::All);
                    return;
                }
                pointer.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep);
            }
        }
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for_surface(surface.wl_surface()) {
            self.refresh_window_identity(&window);
            self.apply_window_rules(&window);
        }
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for_surface(surface.wl_surface()) {
            self.refresh_window_identity(&window);
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        for ws in &mut self.composer {
            if ws.order.iter().any(|w| w == &window) {
                ws.order.retain(|w| w != &window);
                ws.space.unmap_elem(&window);
                if ws.focused.as_ref() == Some(&window) {
                    ws.focused = None;
                }
            }
        }
        if self.focused.as_ref() == Some(&window) {
            self.focused = None;
            let next = self.active_workspace().order.last().cloned();
            if let Some(next) = next {
                let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                self.focus_window(Some(next), serial);
            } else {
                let keyboard = self.seat.get_keyboard().unwrap();
                keyboard.set_focus(self, Option::<WlSurface>::None, smithay::utils::SERIAL_COUNTER.next_serial());
            }
        }
        if self.fullscreen.as_ref() == Some(&window) {
            self.fullscreen = None;
        }
        self.arrange_active();
    }
}

impl<BackendData: crate::state::Backend + 'static> KyonState<BackendData> {
    pub fn refresh_window_identity(&self, window: &WindowElement) {
        let (app_id, title) = window.refresh_identity();
        let mut meta = window.meta();
        let changed = meta.app_id != app_id || meta.title != title;
        meta.app_id = app_id;
        meta.title = title;
        let shell = meta.app_id.contains("kyonshell")
            || meta.app_id.contains("kyon-settings")
            || meta.title == "Kyon Settings";
        if shell {
            meta.shell = true;
            meta.ssd = false;
        }
        let _ = changed;
    }

    pub fn apply_window_rules(&mut self, window: &WindowElement) {
        let (app_id, title) = {
            let meta = window.meta_ref();
            (meta.app_id.clone(), meta.title.clone())
        };
        if app_id.is_empty() && title.is_empty() {
            return;
        }
        let rules: Vec<_> = self
            .config
            .rules
            .iter()
            .filter(|rule| {
                (rule.app_id.is_empty()
                    || app_id.to_ascii_lowercase().contains(&rule.app_id.to_ascii_lowercase()))
                    && (rule.title.is_empty()
                        || title.to_ascii_lowercase().contains(&rule.title.to_ascii_lowercase()))
            })
            .cloned()
            .collect();
        let shell = app_id.contains("kyonshell") || app_id.contains("kyon-settings") || title == "Kyon Settings";
        let mut target_workspace = None;
        {
            let mut meta = window.meta();
            if shell {
                meta.shell = true;
                meta.ssd = false;
            }
            if !meta.rules_applied {
                for rule in &rules {
                    if let Some(floating) = rule.floating {
                        meta.floating = floating;
                    }
                    if let Some(opacity) = rule.opacity {
                        meta.opacity = opacity.clamp(0.1, 1.0);
                    }
                    if let Some(blur) = rule.blur {
                        meta.blur = blur;
                    }
                    if let Some(decorations) = rule.decorations {
                        meta.ssd = decorations;
                    }
                    if let Some(ws) = rule.workspace {
                        target_workspace = Some(ws);
                    }
                }
                meta.rules_applied = !rules.is_empty();
            } else {
                for rule in &rules {
                    if let Some(opacity) = rule.opacity {
                        meta.opacity = opacity.clamp(0.1, 1.0);
                    }
                    if let Some(blur) = rule.blur {
                        meta.blur = blur;
                    }
                }
            }
        }
        if let Some(target) = target_workspace {
            let target = target.saturating_sub(1).min(8);
            if target != self.active_workspace
                && self.active_workspace().order.iter().any(|w| w == window)
            {
                self.remove_window_from_active(window);
                window.meta().workspace = target;
                self.composer[target].order.push(window.clone());
                self.composer[target].focused = Some(window.clone());
                self.focused = self.active_workspace().order.last().cloned();
                self.arrange_active();
            }
        }
        self.arrange_active();
    }

    fn ensure_initial_configure(&mut self, surface: &WlSurface) {
        with_surface_tree_upward(
            surface,
            (),
            |_, _, _| TraversalAction::DoChildren(()),
            |_, states, _| {
                let _ = states;
            },
            |_, _, _| true,
        );
        if let Some(window) = self.window_for_surface(surface) {
            if let Some(toplevel) = window.0.toplevel() {
                if !toplevel.is_initial_configure_sent() {
                    toplevel.send_configure();
                }
            }
            return;
        }
        if let Some(popup) = self.popups.find_popup(surface) {
            if let PopupKind::Xdg(popup) = popup {
                if !popup.is_initial_configure_sent() {
                    let _ = popup.send_configure();
                }
            }
            return;
        }
        if let Some(output) = self.outputs.iter().find(|output| {
            let map = layer_map_for_output(output);
            map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL).is_some()
        }) {
            let initial = with_states(surface, |states| {
                states
                    .data_map
                    .get::<LayerSurfaceData>()
                    .map(|data| !data.lock().unwrap().initial_configure_sent)
                    .unwrap_or(false)
            });
            tracing::debug!(initial, "layer surface commit");
            if initial {
                let mut map = layer_map_for_output(output);
                map.arrange();
                if let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL) {
                    layer.layer_surface().send_configure();
                }
            }
        }
    }

    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self.window_for_surface(&root) else {
            return;
        };
        let Some(output) = self
            .active_workspace()
            .space
            .outputs_for_element(&window)
            .first()
            .cloned()
            .or_else(|| self.primary_output())
        else {
            return;
        };
        let mut output_geo = self.output_geometry(&output);
        for extra in self.active_workspace().space.outputs_for_element(&window) {
            output_geo = output_geo.merge(self.output_geometry(&extra));
        }
        let window_geo = self
            .active_workspace()
            .space
            .element_geometry(&window)
            .unwrap_or_default();
        let mut target = output_geo;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window_geo.loc;
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}

impl<BackendData: crate::state::Backend + 'static> WlrLayerShellHandler for KyonState<BackendData> {
    fn shell_state(&mut self) -> &mut smithay::wayland::shell::wlr_layer::WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        wl_output: Option<wl_output::WlOutput>,
        _layer: smithay::wayland::shell::wlr_layer::Layer,
        namespace: String,
    ) {
        tracing::debug!(namespace = %namespace, "new layer surface");
        let output = wl_output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.primary_output());
        if let Some(output) = output {
            let mut map = layer_map_for_output(&output);
            if let Err(err) = map.map_layer(&LayerSurface::new(surface, namespace)) {
                tracing::warn!(?err, "failed to map layer surface");
            }
        }
        self.arrange_active();
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        if let Some((mut map, layer)) = self.outputs.iter().find_map(|output| {
            let map = layer_map_for_output(output);
            let layer = map
                .layers()
                .find(|layer| layer.layer_surface() == &surface)
                .cloned();
            layer.map(|layer| (map, layer))
        }) {
            map.unmap_layer(&layer);
        }
        self.arrange_active();
    }

    fn ack_configure(&mut self, _surface: WlSurface, _configure: LayerSurfaceConfigure) {
        tracing::debug!("layer surface ack configure");
        if let Some(output) = self.primary_output() {
            layer_map_for_output(&output).arrange();
        }
        self.arrange_active();
    }

    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        self.unconstrain_popup(&popup);
        if let Err(err) = self.popups.track_popup(PopupKind::from(popup)) {
            tracing::warn!(?err, "failed to track layer popup");
        }
    }
}


