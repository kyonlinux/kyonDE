use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use kyon_ipc::{KyonConfig, Request, Snapshot};
use smithay::{
    utils::IsAlive,
    backend::renderer::element::RenderElementStates,
    desktop::{
        PopupManager, Space, layer_map_for_output,
    },
    input::{
        Seat, SeatHandler, SeatState,
        dnd::{DnDGrab, DndGrabHandler, DndTarget, GrabType, Source},
        keyboard::{LedState, XkbConfig},
        pointer::{CursorImageStatus, Focus, PointerHandle},
        tablet::TabletSeatHandler,
    },
    output::Output,
    input::keyboard::Keysym,
    backend::input::TabletToolDescriptor,
    reexports::{
        calloop::{Interest, LoopHandle, LoopSignal, Mode, PostAction, generic::Generic},
        wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
        wayland_server::{
            Display, DisplayHandle, Resource,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_output, wl_surface::WlSurface},
        },
    },
    utils::{Clock, Logical, Monotonic, Point, Rectangle, Serial, Size, Transform, SERIAL_COUNTER},
    wayland::{
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, get_parent, with_states,
        },
        cursor_shape::CursorShapeManagerState,
        fractional_scale::{FractionalScaleHandler, FractionalScaleManagerState, with_fractional_scale},
        keyboard_shortcuts_inhibit::{
            KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState, KeyboardShortcutsInhibitor,
        },
        output::{OutputHandler, OutputManagerState},
        pointer_constraints::{PointerConstraintsHandler, PointerConstraintsState},
        pointer_gestures::PointerGesturesState,
        presentation::PresentationState,
        relative_pointer::RelativePointerManagerState,
        seat::WaylandFocus,
        selection::{
            SelectionHandler,
            data_device::{DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler, set_data_device_focus},
            primary_selection::{PrimarySelectionHandler, PrimarySelectionState, set_primary_focus},
        },
        shell::{
            wlr_layer::WlrLayerShellState,
            xdg::{
                ToplevelSurface, XdgShellState,
                decoration::{XdgDecorationHandler, XdgDecorationState},
            },
        },
        shm::{ShmHandler, ShmState},
        single_pixel_buffer::SinglePixelBufferState,
        socket::ListeningSocketSource,
        tablet_manager::TabletManagerState,
        text_input::TextInputManagerState,
        text_input::TextInputActivation,
        viewporter::ViewporterState,
        virtual_keyboard::VirtualKeyboardManagerState,
        xdg_activation::{XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData},
    },
};

use crate::{
    config,
    layout::{LayoutItem, LayoutMode, compute_layout},
    util::Easing,
    window::WindowElement,
};

#[derive(Debug, Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

#[derive(Debug)]
pub struct DndIcon {
    pub surface: WlSurface,
    pub offset: Point<i32, Logical>,
}

#[derive(Debug)]
pub struct Workspace {
    pub space: Space<WindowElement>,
    pub order: Vec<WindowElement>,
    pub mode: LayoutMode,
    pub focused: Option<WindowElement>,
    pub scroll_offsets: std::collections::HashMap<String, f64>,
}

impl Workspace {
    pub fn new(mode: LayoutMode) -> Self {
        Self {
            space: Space::default(),
            order: Vec::new(),
            mode,
            focused: None,
            scroll_offsets: std::collections::HashMap::new(),
        }
    }
}

pub struct KyonState<BackendData: Backend + 'static> {
    pub backend_data: BackendData,
    pub socket_name: Option<String>,
    pub display_handle: DisplayHandle,
    pub running: Arc<AtomicBool>,
    pub handle: LoopHandle<'static, KyonState<BackendData>>,
    pub loop_signal: LoopSignal,
    pub clock: Clock<Monotonic>,

    pub config: KyonConfig,
    pub xkb_layout: String,
    pub xkb_variant: String,
    pub composer: Vec<Workspace>,
    pub active_workspace: usize,
    pub outputs: Vec<Output>,
    pub displays: Vec<kyon_ipc::DisplayInfo>,
    pub focused: Option<WindowElement>,
    pub fullscreen: Option<WindowElement>,
    pub popups: PopupManager,
    pub dnd_icon: Option<DndIcon>,

    pub compositor_state: CompositorState,
    pub data_device_state: DataDeviceState,
    pub layer_shell_state: WlrLayerShellState,
    pub output_manager_state: OutputManagerState,
    pub primary_selection_state: PrimarySelectionState,
    pub seat_state: SeatState<KyonState<BackendData>>,
    pub keyboard_shortcuts_inhibit_state: KeyboardShortcutsInhibitState,
    pub shm_state: ShmState,
    pub viewporter_state: ViewporterState,
    pub xdg_activation_state: XdgActivationState,
    pub xdg_decoration_state: XdgDecorationState,
    pub xdg_shell_state: XdgShellState,
    pub presentation_state: PresentationState,
    pub fractional_scale_manager_state: FractionalScaleManagerState,
    pub single_pixel_buffer_state: SinglePixelBufferState,
    pub cursor_shape_manager_state: CursorShapeManagerState,
    pub pointer_constraints_state: PointerConstraintsState,

    pub seat: Seat<KyonState<BackendData>>,
    pub pointer: PointerHandle<KyonState<BackendData>>,
    pub cursor_status: CursorImageStatus,
    pub pointer_location: Point<f64, Logical>,
    pub suppressed_keys: Vec<Keysym>,
    pub cursor_position_hint: Option<(WlSurface, Point<f64, Logical>)>,

    pub pending_initial: Vec<WindowElement>,
    pub frame_count: u64,
    pub last_tick: Instant,
    pub ipc_path: Option<String>,
    pub shaders: Option<crate::render::Shaders>,
    pub wallpaper_cache: Option<crate::render::WallpaperCache>,
    pub titlebars: HashMap<smithay::reexports::wayland_server::backend::ObjectId, crate::render::TitlebarCache>,
    pub cursor_texture: Option<crate::render::CursorTexture>,
    pub needs_redraw: bool,
    pub redraw_hook: Option<fn(&mut KyonState<BackendData>)>,
    pub config_mtime: Option<std::time::SystemTime>,
}

#[derive(Clone, Copy, Debug)]
pub struct WindowDrawInfo {
    pub outer: Rectangle<f64, Logical>,
    pub opacity: f32,
    pub active: bool,
    pub titlebar_height: i32,
    pub rounding: f32,
    pub border_width: f32,
    pub border: [f32; 4],
}

impl<BackendData: Backend + 'static> KyonState<BackendData> {
    pub fn new(
        display: Display<Self>,
        handle: LoopHandle<'static, Self>,
        loop_signal: LoopSignal,
        backend_data: BackendData,
        listen_on_socket: bool,
    ) -> Self {
        let dh = display.handle();
        let clock = Clock::new();
        let config = config::load();
        let mode = config.layout.mode.parse::<LayoutMode>().unwrap_or(LayoutMode::Tiling);

        let socket_name = if listen_on_socket {
            let source = ListeningSocketSource::new_auto().unwrap();
            let socket_name = source.socket_name().to_string_lossy().into_owned();
            handle
                .insert_source(source, |client_stream, _, data| {
                    if let Err(err) = data
                        .display_handle
                        .insert_client(client_stream, Arc::new(ClientState::default()))
                    {
                        tracing::warn!(?err, "failed to insert wayland client");
                    }
                })
                .expect("failed to insert socket source");
            Some(socket_name)
        } else {
            None
        };

        handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, data| {
                    unsafe {
                        display.get_mut().dispatch_clients(data).unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .expect("failed to insert display source");

        let compositor_state = CompositorState::new::<Self>(&dh);
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let primary_selection_state = PrimarySelectionState::new::<Self>(&dh);
        let mut seat_state = SeatState::new();
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let viewporter_state = ViewporterState::new::<Self>(&dh);
        let xdg_activation_state = XdgActivationState::new::<Self>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let presentation_state = PresentationState::new::<Self>(&dh, clock.id() as u32);
        let fractional_scale_manager_state = FractionalScaleManagerState::new::<Self>(&dh);
        let single_pixel_buffer_state = SinglePixelBufferState::new::<Self>(&dh);
        let cursor_shape_manager_state = CursorShapeManagerState::new::<Self>(&dh);
        let pointer_constraints_state = PointerConstraintsState::new::<Self>(&dh);
        let keyboard_shortcuts_inhibit_state = KeyboardShortcutsInhibitState::new::<Self>(&dh);
        TextInputManagerState::new::<Self>(&dh);
        VirtualKeyboardManagerState::new::<Self, _>(&dh, |_| true);
        RelativePointerManagerState::new::<Self>(&dh);
        PointerGesturesState::new::<Self>(&dh);
        TabletManagerState::new::<Self>(&dh);

        let seat_name = backend_data.seat_name();
        let mut seat = seat_state.new_wl_seat(&dh, seat_name);
        let pointer = seat.add_pointer();
        seat.add_keyboard(
            XkbConfig {
                rules: "",
                model: "",
                layout: &config.input.keyboard_layout,
                variant: &config.input.keyboard_variant,
                options: None,
            },
            config.input.repeat_delay,
            config.input.repeat_rate,
        )
        .expect("failed to add keyboard");
        seat.add_touch();

        let mut composer = Vec::new();
        for _ in 0..9 {
            composer.push(Workspace::new(mode));
        }

        let backend_ipc = std::env::var("KYONDE_NO_IPC").is_err();
        let xkb_layout = config.input.keyboard_layout.clone();
        let xkb_variant = config.input.keyboard_variant.clone();
        let mut state = Self {
            backend_data,
            socket_name,
            display_handle: dh,
            running: Arc::new(AtomicBool::new(true)),
            handle,
            loop_signal,
            clock,
            config,
            xkb_layout,
            xkb_variant,
            composer,
            active_workspace: 0,
            outputs: Vec::new(),
            displays: Vec::new(),
            focused: None,
            fullscreen: None,
            popups: PopupManager::default(),
            dnd_icon: None,
            compositor_state,
            data_device_state,
            layer_shell_state,
            output_manager_state,
            primary_selection_state,
            seat_state,
            keyboard_shortcuts_inhibit_state,
            shm_state,
            viewporter_state,
            xdg_activation_state,
            xdg_decoration_state,
            xdg_shell_state,
            presentation_state,
            fractional_scale_manager_state,
            single_pixel_buffer_state,
            cursor_shape_manager_state,
            pointer_constraints_state,
            seat,
            pointer,
            cursor_status: CursorImageStatus::default_named(),
            pointer_location: (0.0, 0.0).into(),
            suppressed_keys: Vec::new(),
            cursor_position_hint: None,
            pending_initial: Vec::new(),
            frame_count: 0,
            last_tick: Instant::now(),
            ipc_path: None,
            shaders: None,
            wallpaper_cache: None,
            titlebars: HashMap::new(),
            cursor_texture: None,
            needs_redraw: true,
            redraw_hook: None,
            config_mtime: std::fs::metadata(kyon_ipc::config_path())
                .and_then(|m| m.modified())
                .ok(),
        };

        if backend_ipc {
            state.init_ipc();
        }
        state
    }

    pub fn init_ipc(&mut self) {
        let path = kyon_ipc::socket_path();
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        let listener = match std::os::unix::net::UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(err) => {
                tracing::warn!(?err, path = %path.display(), "failed to bind ipc socket");
                return;
            }
        };
        self.ipc_path = Some(path.to_string_lossy().into_owned());
        let (job_tx, job_rx) = std::sync::mpsc::channel::<(
            Request,
            std::sync::mpsc::Sender<kyon_ipc::Response>,
        )>();
        let (ping, ping_source) = match smithay::reexports::calloop::ping::make_ping() {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!(?err, "failed to create ipc wakeup source");
                return;
            }
        };
        if let Err(err) = self.handle.insert_source(ping_source, move |_, _, data| {
            while let Ok((request, sender)) = job_rx.try_recv() {
                let response = data.handle_request(request);
                let _ = sender.send(response);
            }
        }) {
            tracing::warn!(?err, "failed to add ipc wakeup source");
            return;
        }

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                let job_tx = job_tx.clone();
                let ping = ping.clone();
                std::thread::spawn(move || {
                    use std::io::{BufRead, BufReader, Write};
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(800)));
                    let Ok(read_stream) = stream.try_clone() else {
                        return;
                    };
                    let mut reader = BufReader::new(read_stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let request = match serde_json::from_str::<Request>(line.trim()) {
                        Ok(request) => request,
                        Err(err) => {
                            let response = kyon_ipc::Response::err(format!("invalid request: {err}"));
                            if let Ok(payload) = serde_json::to_string(&response) {
                                let mut writer = stream;
                                let _ = writer.write_all(payload.as_bytes());
                                let _ = writer.write_all(b"\n");
                                let _ = writer.flush();
                            }
                            return;
                        }
                    };
                    let (sender, receiver) = std::sync::mpsc::channel();
                    if job_tx.send((request, sender)).is_err() {
                        return;
                    }
                    let _ = ping.ping();
                    let Ok(response) = receiver.recv_timeout(Duration::from_millis(3000)) else {
                        return;
                    };
                    if let Ok(payload) = serde_json::to_string(&response) {
                        let mut writer = stream;
                        let _ = writer.write_all(payload.as_bytes());
                        let _ = writer.write_all(b"\n");
                        let _ = writer.flush();
                    }
                });
            }
        });
    }
    pub fn add_output(&mut self, output: &Output) {
        if self.outputs.contains(output) {
            return;
        }
        self.outputs.push(output.clone());
        let location = output.current_location();
        for ws in &mut self.composer {
            ws.space.map_output(output, location);
        }
        for idx in 0..self.composer.len() {
            if idx != self.active_workspace {
                self.composer[idx].space.unmap_output(output);
            }
        }
        layer_map_for_output(output).arrange();
        self.arrange_all();
    }

    pub fn remove_output(&mut self, output: &Output) {
        for ws in &mut self.composer {
            ws.space.unmap_output(output);
        }
        self.outputs.retain(|o| o != output);
    }

    pub fn primary_output(&self) -> Option<Output> {
        for rule in &self.config.outputs {
            if rule.primary && rule.enabled {
                if let Some(output) = self.outputs.iter().find(|output| output.name() == rule.name) {
                    return Some(output.clone());
                }
            }
        }
        self.outputs
            .iter()
            .find(|output| self.output_enabled(output))
            .or_else(|| self.outputs.first())
            .cloned()
    }

    pub fn update_display(&mut self, output: &Output) {
        let mode = output.current_mode().unwrap_or(smithay::output::Mode {
            size: (1280, 720).into(),
            refresh: 60_000,
        });
        let scale = output.current_scale().fractional_scale();
        let location = output.current_location();
        let rule = self.config.outputs.iter().find(|rule| rule.name == output.name());
        let existing = self
            .displays
            .iter()
            .find(|display| display.name == output.name())
            .cloned();
        let current_mode = kyon_ipc::DisplayMode {
            width: mode.size.w,
            height: mode.size.h,
            refresh: (mode.refresh / 1000).max(1),
            preferred: true,
        };
        let mut modes = existing
            .as_ref()
            .map(|display| display.modes.clone())
            .filter(|modes| !modes.is_empty())
            .unwrap_or_else(|| vec![current_mode.clone()]);
        if !modes.iter().any(|candidate| {
            candidate.width == current_mode.width
                && candidate.height == current_mode.height
                && candidate.refresh == current_mode.refresh
        }) {
            modes.insert(0, current_mode);
        }
        let info = kyon_ipc::DisplayInfo {
            name: output.name().to_string(),
            description: output.physical_properties().model.clone(),
            width: mode.size.w,
            height: mode.size.h,
            refresh: (mode.refresh / 1000).max(1),
            scale,
            x: location.x,
            y: location.y,
            enabled: rule.map(|rule| rule.enabled).unwrap_or(true),
            primary: rule.map(|rule| rule.primary).unwrap_or(false),
            modes,
        };
        if let Some(slot) = self
            .displays
            .iter_mut()
            .find(|display| display.name == output.name())
        {
            *slot = info;
        } else {
            self.displays.push(info);
        }
    }

    pub fn output_enabled(&self, output: &Output) -> bool {
        self.config
            .outputs
            .iter()
            .find(|rule| rule.name == output.name())
            .map(|rule| rule.enabled)
            .unwrap_or(true)
    }

    pub fn output_under(&self, pos: Point<f64, Logical>) -> Option<Output> {
        self.outputs
            .iter()
            .find(|output| {
                self.output_enabled(output) && self.output_geometry(output).to_f64().contains(pos)
            })
            .cloned()
    }

    pub fn output_for_window(&self, window: &WindowElement) -> Option<Output> {
        let name = window.meta_ref().output.clone();
        self.outputs
            .iter()
            .find(|output| output.name() == name)
            .cloned()
            .or_else(|| self.primary_output())
    }

    pub fn output_geometry(&self, output: &Output) -> Rectangle<i32, Logical> {
        let scale = output.current_scale().fractional_scale();
        let transform = output.current_transform();
        let mode = output.current_mode().unwrap_or(smithay::output::Mode {
            size: (1280, 720).into(),
            refresh: 60_000,
        });
        Rectangle::new(
            output.current_location(),
            transform.transform_size(mode.size).to_f64().to_logical(scale).to_i32_round(),
        )
    }

    pub fn usable_area(&self, output: &Output) -> Rectangle<i32, Logical> {
        let geo = self.output_geometry(output);
        let map = layer_map_for_output(output);
        let zone = map.non_exclusive_zone();
        Rectangle::new(geo.loc + zone.loc, zone.size)
    }

    pub fn active_workspace(&self) -> &Workspace {
        &self.composer[self.active_workspace]
    }

    pub fn active_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.composer[self.active_workspace]
    }

    pub fn workspace_ref(&self) -> &Workspace {
        &self.composer[self.active_workspace]
    }

    pub fn window_for_surface(&self, surface: &WlSurface) -> Option<WindowElement> {
        self.composer
            .iter()
            .flat_map(|ws| ws.order.iter())
            .find(|window| window.wl_surface().as_ref() == Some(surface))
            .cloned()
    }

    pub fn window_for_surface_active(&self, surface: &WlSurface) -> Option<WindowElement> {
        self.active_workspace()
            .order
            .iter()
            .find(|window| window.wl_surface().as_ref() == Some(surface))
            .cloned()
    }

    pub fn focused_window(&self) -> Option<WindowElement> {
        self.focused.clone().filter(|w| w.alive())
    }

    pub fn focus_window(&mut self, window: Option<WindowElement>, serial: Serial) {
        let previous = self.focused.clone();
        if let Some(old) = &previous {
            if Some(old) != window.as_ref() {
                old.set_activated(false);
                old.meta().activated = false;
            }
        }
        self.focused = window.clone();
        self.active_workspace_mut().focused = window.clone();
        if let Some(window) = &window {
            window.set_activated(true);
            window.meta().activated = true;
            if self.active_workspace().mode == LayoutMode::Floating {
                self.active_workspace_mut().space.raise_element(window, true);
            }
        }
        let keyboard = self.seat.get_keyboard().unwrap();
        keyboard.set_focus(self, window.as_ref().and_then(|w| w.wl_surface()), serial);
        self.arrange_active();
    }

    pub fn focus_next(&mut self, forward: bool, serial: Serial) {
        let order = self.active_workspace().order.clone();
        if order.is_empty() {
            return;
        }
        let current = self
            .focused
            .as_ref()
            .and_then(|f| order.iter().position(|w| w == f));
        let next = match current {
            Some(idx) if forward => (idx + 1) % order.len(),
            Some(idx) => (idx + order.len() - 1) % order.len(),
            None => 0,
        };
        self.focus_window(Some(order[next].clone()), serial);
    }

    pub fn focus_direction(&mut self, dx: i32, dy: i32, serial: Serial) {
        let Some(current) = self.focused_window() else {
            self.focus_next(true, serial);
            return;
        };
        let order = self.active_workspace().order.clone();
        if self.active_workspace().mode == LayoutMode::Scrolling && dy == 0 {
            if let Some(idx) = order.iter().position(|w| w == &current) {
                let next = if dx > 0 {
                    (idx + 1).min(order.len() - 1)
                } else {
                    idx.saturating_sub(1)
                };
                self.focus_window(Some(order[next].clone()), serial);
            }
            return;
        }
        if self.active_workspace().mode == LayoutMode::Tiling && dy != 0 {
            if let Some(idx) = order.iter().position(|w| w == &current) {
                if dy > 0 {
                    let next = (idx + 1) % order.len();
                    self.focus_window(Some(order[next].clone()), serial);
                } else {
                    let next = (idx + order.len() - 1) % order.len();
                    self.focus_window(Some(order[next].clone()), serial);
                }
            }
            return;
        }
        let current_center = {
            let meta = current.meta_ref();
            (
                meta.outer.loc.x as f64 + meta.outer.size.w as f64 / 2.0,
                meta.outer.loc.y as f64 + meta.outer.size.h as f64 / 2.0,
            )
        };
        let mut best: Option<(f64, WindowElement)> = None;
        for window in &order {
            if window == &current {
                continue;
            }
            let meta = window.meta_ref();
            let center = (
                meta.outer.loc.x as f64 + meta.outer.size.w as f64 / 2.0,
                meta.outer.loc.y as f64 + meta.outer.size.h as f64 / 2.0,
            );
            let vx = center.0 - current_center.0;
            let vy = center.1 - current_center.1;
            let directional = if dx > 0 {
                vx > 40.0
            } else if dx < 0 {
                vx < -40.0
            } else if dy > 0 {
                vy > 40.0
            } else if dy < 0 {
                vy < -40.0
            } else {
                false
            };
            if !directional {
                continue;
            }
            let score = vx * vx + vy * vy;
            if best.as_ref().map(|(s, _)| score < *s).unwrap_or(true) {
                best = Some((score, window.clone()));
            }
        }
        if let Some((_, window)) = best {
            self.focus_window(Some(window), serial);
        }
    }

    pub fn switch_workspace(&mut self, index: usize) {
        let count = self.config.shell.workspaces.clamp(1, 9);
        let index = index.min(count - 1);
        if index == self.active_workspace {
            return;
        }
        let serial = SERIAL_COUNTER.next_serial();
        let outputs: Vec<Output> = self
            .outputs
            .iter()
            .filter(|output| self.output_enabled(output))
            .cloned()
            .collect();
        for output in &outputs {
            self.composer[self.active_workspace].space.unmap_output(output);
            self.composer[index]
                .space
                .map_output(output, output.current_location());
        }
        self.active_workspace = index;
        let focused = self.composer[index].focused.clone().filter(|w| w.alive());
        self.focused = focused.clone();
        if let Some(window) = &focused {
            window.set_activated(true);
        }
        let keyboard = self.seat.get_keyboard().unwrap();
        keyboard.set_focus(self, focused.as_ref().and_then(|w| w.wl_surface()), serial);
        self.arrange_active();
    }

    pub fn set_layout_mode(&mut self, mode: LayoutMode) {
        let area = self
            .primary_output()
            .map(|output| self.usable_area(&output))
            .unwrap_or_else(|| Rectangle::from_size((1280, 720).into()));
        let ws = &mut self.composer[self.active_workspace];
        ws.mode = mode;
        let order = ws.order.clone();
        let mut cascade = 0;
        for window in &order {
            let mut meta = window.meta();
            if mode != LayoutMode::Floating {
                meta.floating = false;
            }
            if mode == LayoutMode::Floating {
                if !meta.floating {
                    let w = (area.size.w as f64 * 0.62) as i32;
                    let h = (area.size.h as f64 * 0.68) as i32;
                    meta.saved_float = Some(Rectangle::new(
                        (
                            area.loc.x + 24 + cascade * 34,
                            area.loc.y + 24 + cascade * 30,
                        )
                            .into(),
                        Size::from((w.max(260), h.max(160))),
                    ));
                    meta.last_configure = None;
                    meta.floating = true;
                    cascade = (cascade + 1) % 6;
                }
            }
        }
        self.arrange_active();
    }

    pub fn toggle_floating(&mut self) {
        let Some(window) = self.focused_window() else {
            return;
        };
        let ws = &mut self.composer[self.active_workspace];
        ws.mode = LayoutMode::Tiling;
        let mut meta = window.meta();
        meta.floating = !meta.floating;
        if meta.floating {
            meta.saved_float = Some(meta.outer);
            ws.space.raise_element(&window, true);
        }
        drop(meta);
        self.arrange_active();
    }

    pub fn move_window_output(&mut self, next: bool) {
        let Some(window) = self.focused_window() else {
            return;
        };
        let outputs: Vec<Output> = self
            .outputs
            .iter()
            .filter(|output| self.output_enabled(output))
            .cloned()
            .collect();
        if outputs.len() < 2 {
            return;
        }
        let current = window.meta_ref().output.clone();
        let index = outputs
            .iter()
            .position(|output| output.name() == current)
            .unwrap_or(0);
        let target = if next {
            (index + 1) % outputs.len()
        } else {
            (index + outputs.len() - 1) % outputs.len()
        };
        let output = &outputs[target];
        let area = self.usable_area(output);
        let name = output.name().to_string();
        let mut meta = window.meta();
        meta.output = name;
        meta.placed = false;
        meta.last_configure = None;
        if meta.floating || self.active_workspace().mode == LayoutMode::Floating {
            let w = (area.size.w as f64 * 0.62) as i32;
            let h = (area.size.h as f64 * 0.68) as i32;
            meta.saved_float = Some(Rectangle::new(
                (
                    area.loc.x + self.config.decor.gaps_out + 40,
                    area.loc.y + self.config.decor.gaps_out + 30,
                )
                    .into(),
                Size::from((w.max(280), h.max(180))),
            ));
        }
        drop(meta);
        self.arrange_active();
    }

    pub fn toggle_maximize(&mut self) {
        let Some(window) = self.focused_window() else {
            return;
        };
        let mode = self.active_workspace().mode;
        let maximize = !window.meta_ref().maximized;
        let mut meta = window.meta();
        if maximize {
            if meta.floating || mode == LayoutMode::Floating {
                meta.saved_float = Some(meta.outer);
            }
            meta.maximized = true;
        } else {
            meta.maximized = false;
            meta.last_configure = None;
        }
        let titlebar = if maximize {
            crate::decor::titlebar_height(&self.config, &meta, mode)
        } else {
            0
        };
        drop(meta);
        let size = self.output_for_window(&window).map(|output| {
            let area = self.usable_area(&output);
            Size::from(((area.size.w).max(1), (area.size.h - titlebar).max(1)))
        });
        if let Some(toplevel) = window.0.toplevel() {
            use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
            toplevel.with_pending_state(|state| {
                if maximize {
                    state.states.set(xdg_toplevel::State::Maximized);
                    state.size = size;
                } else {
                    state.states.unset(xdg_toplevel::State::Maximized);
                    state.size = None;
                }
            });
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
        self.arrange_active();
    }

    pub fn toggle_fullscreen(&mut self) {
        let Some(window) = self.focused_window() else {
            return;
        };
        let mut meta = window.meta();
        meta.fullscreen = !meta.fullscreen;
        let fullscreen = meta.fullscreen;
        drop(meta);
        if fullscreen {
            self.fullscreen = Some(window.clone());
        } else if self.fullscreen.as_ref() == Some(&window) {
            self.fullscreen = None;
        }
        if let Some(toplevel) = window.0.toplevel() {
            toplevel.with_pending_state(|state| {
                use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
                if fullscreen {
                    state.states.set(xdg_toplevel::State::Fullscreen);
                } else {
                    state.states.unset(xdg_toplevel::State::Fullscreen);
                }
            });
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
        self.arrange_active();
    }

    pub fn move_window(&mut self, dx: i32, dy: i32) {
        let Some(window) = self.focused_window() else {
            return;
        };
        let ws = &mut self.composer[self.active_workspace];
        if ws.mode == LayoutMode::Floating {
            let mut meta = window.meta();
            meta.outer.loc.x += dx;
            meta.outer.loc.y += dy;
            return;
        }
        if let Some(idx) = ws.order.iter().position(|w| w == &window) {
            let len = ws.order.len();
            let next = if dx > 0 || dy > 0 {
                (idx + 1) % len
            } else {
                (idx + len - 1) % len
            };
            ws.order.swap(idx, next);
        }
        self.arrange_active();
    }

    pub fn scroll_focus(&mut self, forward: bool) {
        let serial = SERIAL_COUNTER.next_serial();
        self.focus_next(forward, serial);
    }

    pub fn cycle_layout(&mut self) {
        let next = self.active_workspace().mode.next();
        self.set_layout_mode(next);
    }

    pub fn move_to_workspace(&mut self, target: usize) {
        let count = self.config.shell.workspaces.clamp(1, 9);
        let target = target.min(count - 1);
        if target == self.active_workspace {
            return;
        }
        let Some(window) = self.focused_window() else {
            return;
        };
        self.remove_window_from_active(&window);
        window.meta().workspace = target;
        self.composer[target].order.push(window.clone());
        self.composer[target].focused = Some(window.clone());
        if let Some(next) = self.active_workspace().order.first().cloned() {
            let serial = SERIAL_COUNTER.next_serial();
            self.focus_window(Some(next), serial);
        } else {
            let serial = SERIAL_COUNTER.next_serial();
            self.focus_window(None, serial);
        }
    }

    pub fn close_focused(&mut self) {
        if let Some(window) = self.focused_window() {
            window.send_close();
        }
    }

    pub fn remove_window_from_active(&mut self, window: &WindowElement) {
        let ws = &mut self.composer[self.active_workspace];
        ws.order.retain(|w| w != window);
        ws.space.unmap_elem(window);
        if ws.focused.as_ref() == Some(window) {
            ws.focused = None;
        }
    }

    pub fn arrange_all(&mut self) {
        self.arrange_active();
    }

    pub fn arrange_active(&mut self) {
        let outputs: Vec<Output> = self
            .outputs
            .iter()
            .filter(|output| self.output_enabled(output))
            .cloned()
            .collect();
        if outputs.is_empty() {
            return;
        }
        let names: Vec<String> = outputs.iter().map(|output| output.name().to_string()).collect();
        let primary = self
            .primary_output()
            .map(|output| output.name().to_string())
            .filter(|name| names.contains(name))
            .unwrap_or_else(|| names[0].clone());
        let order = self.active_workspace().order.clone();
        for window in &order {
            let mut meta = window.meta();
            if meta.output.is_empty() || !names.contains(&meta.output) {
                meta.output = primary.clone();
            }
        }
        for output in &outputs {
            self.arrange_output(output);
        }
    }

    fn arrange_output(&mut self, output: &Output) {
        let output_name = output.name().to_string();
        let area = self.usable_area(output);
        let mode = self.active_workspace().mode;
        let all_order = self.active_workspace().order.clone();
        let order: Vec<WindowElement> = all_order
            .iter()
            .filter(|window| window.meta_ref().output == output_name)
            .cloned()
            .collect();
        let focused = self.focused.clone();
        let config = self.config.clone();
        let now = Instant::now();
        let animation = config.animations.enabled;
        let duration = Duration::from_micros((900_000.0 / config.animations.speed.max(1.0)) as u64);
        let easing = Easing::from_spec(&config.animations.bezier);
        let scroll_offset = *self
            .active_workspace()
            .scroll_offsets
            .get(&output_name)
            .unwrap_or(&0.0);

        let mut items = Vec::new();
        for window in &order {
            let meta = window.meta_ref();
            let mut saved = meta.saved_float.unwrap_or(meta.outer);
            if saved.size.w < 120 || saved.size.h < 80 {
                let w = (area.size.w as f64 * 0.62) as i32;
                let h = (area.size.h as f64 * 0.68) as i32;
                saved = Rectangle::new(
                    (
                        area.loc.x + config.decor.gaps_out + 40,
                        area.loc.y + config.decor.gaps_out + 30,
                    )
                        .into(),
                    Size::from((w.max(280), h.max(180))),
                );
            }
            items.push(LayoutItem {
                floating: meta.floating && mode != LayoutMode::Floating,
                fullscreen: meta.fullscreen,
                maximized: meta.maximized,
                saved,
                content_height: meta.outer.size.h,
            });
        }

        let focused_index = focused
            .as_ref()
            .and_then(|f| order.iter().position(|w| w == f));
        let layout_output = compute_layout(
            mode,
            &config.layout,
            &config.decor,
            area,
            &items,
            focused_index,
            scroll_offset,
        );
        self.composer[self.active_workspace]
            .scroll_offsets
            .insert(output_name.clone(), layout_output.scroll_offset);

        for (idx, window) in order.iter().enumerate() {
            if !window.alive() {
                continue;
            }
            let mut target = layout_output.rects.get(idx).copied().flatten();
            let mut meta = window.meta();
            if meta.maximized {
                target = Some(area);
            }
            let Some(target) = target else {
                continue;
            };
            let first_place = !meta.placed;
            let is_active = focused.as_ref() == Some(window);
            let titlebar_h = crate::decor::titlebar_height(&config, &meta, mode);
            let content_size = Size::from((
                (target.size.w).max(1),
                (target.size.h - titlebar_h).max(1),
            ));
            let content_loc = Point::from((target.loc.x, target.loc.y + titlebar_h));

            let needs_configure = if meta.maximized
                || (mode != LayoutMode::Floating && !meta.floating && !meta.fullscreen)
            {
                meta.last_configure != Some(content_size)
            } else {
                meta.last_configure.is_none()
            };
            if needs_configure {
                meta.last_configure = Some(content_size);
                drop(meta);
                window.configure(content_size, is_active);
                meta = window.meta();
            }

            if animation {
                if first_place {
                    meta.x.from = target.loc.x as f32 + target.size.w as f32 * 0.015;
                    meta.y.from = target.loc.y as f32 + 24.0;
                    meta.width.from = target.size.w as f32 * 0.97;
                    meta.height.from = target.size.h as f32 * 0.97;
                    meta.fade.from = 0.0;
                    meta.x.start = now;
                    meta.y.start = now;
                    meta.width.start = now;
                    meta.height.start = now;
                    meta.fade.start = now;
                }
                meta.x.retarget(target.loc.x as f32, duration, easing, now);
                meta.y.retarget(target.loc.y as f32, duration, easing, now);
                meta.width.retarget(target.size.w as f32, duration, easing, now);
                meta.height.retarget(target.size.h as f32, duration, easing, now);
                meta.fade.retarget(1.0, duration, easing, now);
            } else {
                meta.x.snap(target.loc.x as f32);
                meta.y.snap(target.loc.y as f32);
                meta.width.snap(target.size.w as f32);
                meta.height.snap(target.size.h as f32);
                meta.fade.snap(1.0);
            }
            meta.outer = target;
            meta.placed = true;
            meta.opacity = if is_active {
                config.decor.active_opacity
            } else {
                config.decor.inactive_opacity
            };
            let maximized = meta.maximized;
            if (meta.floating || mode == LayoutMode::Floating) && !maximized {
                meta.saved_float = Some(target);
            }
            drop(meta);
            window.set_activated(is_active);
            let space = &mut self.composer[self.active_workspace].space;
            if space.element_location(window).is_some() {
                space.relocate_element(window, content_loc);
            } else {
                space.map_element(window.clone(), content_loc, false);
            }
            if maximized {
                space.raise_element(window, true);
            }
        }

        if let Some(fullscreen) = self.fullscreen.clone() {
            if !fullscreen.alive() {
                self.fullscreen = None;
            } else if fullscreen.meta_ref().output == output_name {
                let geo = self.output_geometry(output);
                let mut meta = fullscreen.meta();
                meta.outer = geo;
                meta.x.snap(geo.loc.x as f32);
                meta.y.snap(geo.loc.y as f32);
                meta.width.snap(geo.size.w as f32);
                meta.height.snap(geo.size.h as f32);
                meta.fade.snap(1.0);
                meta.placed = true;
                if meta.last_configure != Some(geo.size) {
                    meta.last_configure = Some(geo.size);
                    drop(meta);
                    fullscreen.configure(geo.size, true);
                } else {
                    drop(meta);
                }
                let space = &mut self.composer[self.active_workspace].space;
                space.map_element(fullscreen.clone(), geo.loc, true);
            }
        }

        if mode == LayoutMode::Floating {
            if let Some(focused) = &focused {
                if focused.meta_ref().output == output_name {
                    self.composer[self.active_workspace]
                        .space
                        .raise_element(focused, true);
                }
            }
        }

        let mut map = layer_map_for_output(output);
        map.arrange();
    }

    pub fn check_config_reload(&mut self) {
        let mtime = std::fs::metadata(kyon_ipc::config_path())
            .and_then(|m| m.modified())
            .ok();
        if mtime != self.config_mtime {
            self.config_mtime = mtime;
            self.reload_config();
        }
    }

    pub fn post_render(&mut self, output: &Output, time: Duration) {
        use smithay::desktop::utils::surface_primary_scanout_output;
        let throttle = Some(Duration::from_secs(1));
        let windows: Vec<WindowElement> = self
            .active_workspace()
            .order
            .iter()
            .filter(|w| w.alive() && self.active_workspace().space.outputs_for_element(w).contains(output))
            .cloned()
            .collect();
        for window in windows {
            window.0.send_frame(
                output,
                time,
                throttle,
                surface_primary_scanout_output,
            );
        }
        let map = layer_map_for_output(output);
        for layer in map.layers() {
            layer.send_frame(output, time, throttle, surface_primary_scanout_output);
        }
    }

    pub fn focused_title(&self) -> Option<String> {
        self.focused
            .as_ref()
            .map(|w| w.meta_ref().title.clone())
            .filter(|t| !t.is_empty())
    }

    pub fn snapshot(&self) -> Snapshot {
        let geo = self
            .primary_output()
            .map(|o| self.output_geometry(&o))
            .unwrap_or_default();
        Snapshot {
            version: kyon_ipc::VERSION.into(),
            layout_mode: self.active_workspace().mode.as_str().into(),
            active_workspace: self.active_workspace,
            workspace_count: self.config.shell.workspaces,
            windows: self.active_workspace().order.len(),
            focused_title: self.focused_title(),
            output_width: geo.size.w,
            output_height: geo.size.h,
            gpu_backend: std::env::var("KYONDE_GPU_BACKEND").unwrap_or_else(|_| "opengl-es".into()),
            displays: self.displays.clone(),
            config: self.config.clone(),
        }
    }

    pub fn apply_config_patch(&mut self, patch: &serde_json::Value, persist: bool) {
        let previous_mode = self.config.layout.mode.clone();
        config::apply_json_patch(&mut self.config, patch);
        if previous_mode != self.config.layout.mode {
            if let Ok(mode) = self.config.layout.mode.parse::<LayoutMode>() {
                self.composer[self.active_workspace].mode = mode;
            }
        }
        self.post_config_change(persist);
    }

    pub fn apply_output_settings(&mut self) {
        let outputs = self.outputs.clone();
        let active = self.active_workspace;
        for output in outputs {
            let location = output.current_location();
            let rule = self
                .config
                .outputs
                .iter()
                .find(|rule| rule.name == output.name())
                .cloned()
                .unwrap_or_else(|| kyon_ipc::OutputRule {
                    name: output.name().to_string(),
                    scale: output.current_scale().fractional_scale(),
                    mode: String::new(),
                    position_x: location.x,
                    position_y: location.y,
                    transform: String::new(),
                    enabled: true,
                    primary: false,
                });
            self.backend_data.configure_output(&output, &rule);
            let enabled = rule.enabled;
            for (idx, workspace) in self.composer.iter_mut().enumerate() {
                if enabled && idx == active {
                    workspace.space.map_output(&output, location);
                } else {
                    workspace.space.unmap_output(&output);
                }
            }
            layer_map_for_output(&output).arrange();
            self.update_display(&output);
        }
        self.arrange_active();
    }

    pub fn post_config_change(&mut self, persist: bool) {
        if persist {
            config::save_state(&self.config);
        }
        self.apply_output_settings();
        if let Some(keyboard) = self.seat.get_keyboard() {
            let layout = self.config.input.keyboard_layout.clone();
            let variant = self.config.input.keyboard_variant.clone();
            if layout != self.xkb_layout || variant != self.xkb_variant {
                match keyboard.set_xkb_config(
                    self,
                    XkbConfig {
                        rules: "",
                        model: "",
                        layout: &layout,
                        variant: &variant,
                        options: None,
                    },
                ) {
                    Ok(()) => {
                        tracing::info!(layout = %layout, variant = %variant, "keyboard layout applied");
                        self.xkb_layout = layout;
                        self.xkb_variant = variant;
                    }
                    Err(err) => tracing::warn!(?err, "failed to apply keyboard layout"),
                }
            }
            keyboard.change_repeat_info(self.config.input.repeat_rate, self.config.input.repeat_delay);
        }
        if let Some(output) = self.primary_output() {
            self.backend_data.reset_buffers(&output);
        }
        self.arrange_active();
        if let Some(hook) = self.redraw_hook {
            hook(self);
        }
    }

    pub fn reload_config(&mut self) {
        self.config = config::load();
        if let Ok(mode) = self.config.layout.mode.parse::<LayoutMode>() {
            self.composer[self.active_workspace].mode = mode;
        }
        self.post_config_change(false);
        tracing::info!("configuration reloaded");
    }

    pub fn spawn(&self, command: &str) {
        let mut parts = command.split_whitespace();
        let Some(program) = parts.next() else {
            return;
        };
        let program = resolve_binary(program);
        let mut cmd = std::process::Command::new(&program);
        cmd.args(parts);
        if let Some(dir) = program.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            let path = std::env::var("PATH").unwrap_or_default();
            cmd.env("PATH", format!("{}:{}", dir.display(), path));
        }
        if let Some(socket) = &self.socket_name {
            cmd.env("WAYLAND_DISPLAY", socket);
        }
        cmd.env("XDG_CURRENT_DESKTOP", "KyonDE");
        cmd.env("XDG_SESSION_DESKTOP", "kyonde");
        cmd.env("XDG_SESSION_TYPE", "wayland");
        match cmd.spawn() {
            Ok(_) => tracing::info!(command, "spawned"),
            Err(err) => tracing::warn!(?err, command, "failed to spawn"),
        }
    }

    pub fn spawn_terminal(&self) {
        for candidate in ["foot", "kitty", "alacritty", "wezterm", "konsole", "gnome-terminal", "xterm"] {
            if std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("command -v {candidate}"))
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
            {
                self.spawn(candidate);
                return;
            }
        }
    }

    pub fn handle_ipc_stream(&mut self, stream: std::os::unix::net::UnixStream) {
        let _ = stream.set_nonblocking(false);
        let mut reader = match stream.try_clone() {
            Ok(reader) => std::io::BufReader::new(reader),
            Err(_) => return,
        };
        use std::io::{BufRead, Write};
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let response = match serde_json::from_str::<Request>(line.trim()) {
            Ok(request) => self.handle_request(request),
            Err(err) => kyon_ipc::Response::err(format!("invalid request: {err}")),
        };
        if let Ok(payload) = serde_json::to_string(&response) {
            let mut writer = stream;
            let _ = writer.write_all(payload.as_bytes());
            let _ = writer.write_all(b"\n");
            let _ = writer.flush();
        }
    }

    pub fn handle_request(&mut self, request: Request) -> kyon_ipc::Response {
        match request.cmd.as_str() {
            "ping" => kyon_ipc::Response::ok(self.snapshot()),
            "state" => kyon_ipc::Response::ok(self.snapshot()),
            "apply" => {
                self.apply_config_patch(&request.args, true);
                kyon_ipc::Response::ok(self.snapshot())
            }
            "reload" => {
                self.reload_config();
                kyon_ipc::Response::ok(self.snapshot())
            }
            "layout" => {
                let mode = request
                    .args
                    .as_str()
                    .and_then(|value| value.parse::<LayoutMode>().ok());
                if let Some(mode) = mode {
                    self.set_layout_mode(mode);
                    kyon_ipc::Response::ok(self.snapshot())
                } else {
                    kyon_ipc::Response::err("layout expects tiling, scrolling or floating")
                }
            }
            "layout.cycle" => {
                self.cycle_layout();
                kyon_ipc::Response::ok(self.snapshot())
            }
            "workspace" => {
                let Some(index) = request.args.as_u64() else {
                    return kyon_ipc::Response::err("workspace expects an index");
                };
                self.switch_workspace(index as usize);
                kyon_ipc::Response::ok(self.snapshot())
            }
            "focus" => {
                let serial = SERIAL_COUNTER.next_serial();
                match request.args.as_str() {
                    Some("next") => self.focus_next(true, serial),
                    Some("prev") => self.focus_next(false, serial),
                    Some("left") => self.focus_direction(-1, 0, serial),
                    Some("right") => self.focus_direction(1, 0, serial),
                    Some("up") => self.focus_direction(0, -1, serial),
                    Some("down") => self.focus_direction(0, 1, serial),
                    _ => return kyon_ipc::Response::err("unknown focus target"),
                }
                kyon_ipc::Response::ok(self.snapshot())
            }
            "window" => {
                match request.args.as_str() {
                    Some("close") => self.close_focused(),
                    Some("toggle-floating") => self.toggle_floating(),
                    Some("fullscreen") => self.toggle_fullscreen(),
                    Some("maximize") => self.toggle_maximize(),
                    Some("move-output-next") => self.move_window_output(true),
                    Some("move-output-prev") => self.move_window_output(false),
                    Some("next") => self.focus_next(true, SERIAL_COUNTER.next_serial()),
                    Some("prev") => self.focus_next(false, SERIAL_COUNTER.next_serial()),
                    _ => return kyon_ipc::Response::err("unknown window command"),
                }
                kyon_ipc::Response::ok(self.snapshot())
            }
            "spawn" => {
                if let Some(command) = request.args.as_str() {
                    self.spawn(command);
                    kyon_ipc::Response::ok(self.snapshot())
                } else {
                    kyon_ipc::Response::err("spawn expects a command")
                }
            }
            "quit" => {
                self.running.store(false, Ordering::SeqCst);
                self.loop_signal.stop();
                kyon_ipc::Response::ok(self.snapshot())
            }
            other => kyon_ipc::Response::err(format!("unknown command {other}")),
        }
    }

    pub fn process_action(&mut self, action: &str) {
        let mut parts = action.split_whitespace();
        let Some(command) = parts.next() else {
            return;
        };
        let arg = parts.next();
        let serial = SERIAL_COUNTER.next_serial();
        match command {
            "spawn" => {
                if let Some(rest) = action.strip_prefix("spawn ") {
                    self.spawn(rest);
                }
            }
            "spawn-terminal" => self.spawn_terminal(),
            "kill" | "close" => self.close_focused(),
            "layout" => match arg {
                Some("cycle") => self.cycle_layout(),
                Some(mode) => {
                    if let Ok(mode) = mode.parse::<LayoutMode>() {
                        self.set_layout_mode(mode);
                    }
                }
                None => self.cycle_layout(),
            },
            "workspace" => match arg {
                Some("next") => {
                    let count = self.config.shell.workspaces;
                    self.switch_workspace((self.active_workspace + 1) % count);
                }
                Some("prev") => {
                    let count = self.config.shell.workspaces;
                    self.switch_workspace((self.active_workspace + count - 1) % count);
                }
                Some(value) => {
                    if let Ok(index) = value.parse::<usize>() {
                        self.switch_workspace(index.saturating_sub(1));
                    }
                }
                None => {}
            },
            "move-workspace" => match arg {
                Some("next") => {
                    let count = self.config.shell.workspaces;
                    self.move_to_workspace((self.active_workspace + 1) % count);
                }
                Some("prev") => {
                    let count = self.config.shell.workspaces;
                    self.move_to_workspace((self.active_workspace + count - 1) % count);
                }
                Some(value) => {
                    if let Ok(index) = value.parse::<usize>() {
                        self.move_to_workspace(index.saturating_sub(1));
                    }
                }
                None => {}
            },
            "focus" => match arg {
                Some("next") => self.focus_next(true, serial),
                Some("prev") => self.focus_next(false, serial),
                Some("left") => self.focus_direction(-1, 0, serial),
                Some("right") => self.focus_direction(1, 0, serial),
                Some("up") => self.focus_direction(0, -1, serial),
                Some("down") => self.focus_direction(0, 1, serial),
                _ => {}
            },
            "move-output" => match arg {
                Some("next") => self.move_window_output(true),
                Some("prev") => self.move_window_output(false),
                _ => {}
            },
            "move" => match arg {
                Some("left") => self.move_window(-1, 0),
                Some("right") => self.move_window(1, 0),
                Some("up") => self.move_window(0, -1),
                Some("down") => self.move_window(0, 1),
                _ => {}
            },
            "scroll" => match arg {
                Some("left") => self.scroll_focus(false),
                Some("right") => self.scroll_focus(true),
                _ => {}
            },
            "toggle-floating" => self.toggle_floating(),
            "fullscreen" => self.toggle_fullscreen(),
            "reload" => self.reload_config(),
            "quit" => {
                self.running.store(false, Ordering::SeqCst);
                self.loop_signal.stop();
            }
            "toggle-blur" => {
                self.config.blur.enabled = !self.config.blur.enabled;
                self.post_config_change(true);
            }
            "toggle-animations" => {
                self.config.animations.enabled = !self.config.animations.enabled;
                self.post_config_change(true);
            }
            _ => tracing::warn!(action, "unknown action"),
        }
    }
}

impl<BackendData: Backend + 'static> OutputHandler for KyonState<BackendData> {
    fn output_bound(&mut self, output: Output, wl_output: wl_output::WlOutput) {
        let _ = (output, wl_output);
    }
}

impl<BackendData: Backend + 'static> ShmHandler for KyonState<BackendData> {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl<BackendData: Backend + 'static> SeatHandler for KyonState<BackendData> {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<KyonState<BackendData>> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, target: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let focus = target.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, focus.clone());
        set_primary_focus(dh, seat, focus);
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }

    fn led_state_changed(&mut self, _seat: &Seat<Self>, led_state: LedState) {
        self.backend_data.update_led_state(led_state);
    }
}

impl<BackendData: Backend + 'static> TabletSeatHandler for KyonState<BackendData> {
    type ToolFocus = WlSurface;
    fn tablet_tool_image(&mut self, _tool: &TabletToolDescriptor, image: CursorImageStatus) {
        self.cursor_status = image;
    }
}

impl<BackendData: Backend + 'static> SelectionHandler for KyonState<BackendData> {
    type SelectionUserData = ();
}

impl<BackendData: Backend + 'static> DataDeviceHandler for KyonState<BackendData> {
    fn data_device_state(&mut self) -> &mut DataDeviceState {
        &mut self.data_device_state
    }
}

impl<BackendData: Backend + 'static> PrimarySelectionHandler for KyonState<BackendData> {
    fn primary_selection_state(&mut self) -> &mut PrimarySelectionState {
        &mut self.primary_selection_state
    }
}

impl<BackendData: Backend + 'static> WaylandDndGrabHandler for KyonState<BackendData> {
    fn dnd_requested<S: Source>(
        &mut self,
        source: S,
        icon: Option<WlSurface>,
        seat: Seat<Self>,
        serial: Serial,
        type_: GrabType,
    ) {
        self.dnd_icon = icon.map(|surface| DndIcon {
            surface,
            offset: (0, 0).into(),
        });
        match type_ {
            GrabType::Pointer => {
                if let Some(pointer) = seat.get_pointer() {
                    if let Some(start_data) = pointer.grab_start_data() {
                        pointer.set_grab(
                            self,
                            DnDGrab::new_pointer(&self.display_handle, start_data, source, seat),
                            serial,
                            Focus::Keep,
                        );
                    }
                }
            }
            GrabType::Touch => {}
        }
    }
}

impl<BackendData: Backend + 'static> DndGrabHandler for KyonState<BackendData> {
    fn dropped(
        &mut self,
        _target: Option<DndTarget<'_, Self>>,
        _validated: bool,
        _seat: Seat<Self>,
        _location: Point<f64, Logical>,
    ) {
        self.dnd_icon = None;
    }
}

impl<BackendData: Backend + 'static> KeyboardShortcutsInhibitHandler for KyonState<BackendData> {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.keyboard_shortcuts_inhibit_state
    }

    fn new_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        inhibitor.activate();
    }
}

impl<BackendData: Backend + 'static> PointerConstraintsHandler for KyonState<BackendData> {
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        smithay::wayland::pointer_constraints::with_pointer_constraint(surface, pointer, |constraint| {
            if let Some(constraint) = constraint {
                constraint.activate();
            }
        });
    }

    fn cursor_position_hint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>, location: Point<f64, Logical>) {
        if smithay::wayland::pointer_constraints::with_pointer_constraint(surface, pointer, |constraint| {
            constraint.is_some_and(|c| c.is_active())
        }) {
            self.cursor_position_hint = Some((surface.clone(), location));
        }
    }
}

impl<BackendData: Backend + 'static> XdgActivationHandler for KyonState<BackendData> {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn token_created(&mut self, _token: XdgActivationToken, _data: XdgActivationTokenData) -> bool {
        true
    }

    fn request_activation(&mut self, _token: XdgActivationToken, _data: XdgActivationTokenData, surface: WlSurface) {
        if let Some(window) = self.window_for_surface(&surface) {
            let serial = SERIAL_COUNTER.next_serial();
            self.focus_window(Some(window), serial);
        }
    }
}

impl<BackendData: Backend + 'static> FractionalScaleHandler for KyonState<BackendData> {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let root = {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            root
        };
        let output = self
            .window_for_surface(&root)
            .and_then(|w| self.active_workspace().space.outputs_for_element(&w).first().cloned())
            .or_else(|| self.outputs.first().cloned());
        if let Some(output) = output {
            let scale = output.current_scale().fractional_scale();
            with_states(&surface, |states| {
                with_fractional_scale(states, |fractional| {
                    fractional.set_preferred_scale(scale);
                });
            });
        }
    }
}

impl<BackendData: Backend + 'static> XdgDecorationHandler for KyonState<BackendData> {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecorationMode) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn decoration_destroyed(&mut self, toplevel: ToplevelSurface) {
        if let Some(window) = self.window_for_surface(toplevel.wl_surface()) {
            window.meta().ssd = false;
            self.arrange_active();
        }
    }
}

pub trait Backend {
    const HAS_RELATIVE_MOTION: bool = true;
    const HAS_GESTURES: bool = true;

    fn seat_name(&self) -> String;
    fn reset_buffers(&mut self, output: &Output);
    fn early_import(&mut self, _surface: &WlSurface) {}
    fn update_led_state(&mut self, _led_state: LedState) {}

    fn switch_vt(&mut self, _vt: i32) {}

    fn configure_output(&mut self, output: &Output, rule: &kyon_ipc::OutputRule) {
        if !rule.enabled {
            return;
        }
        let transform = match rule.transform.to_ascii_lowercase().as_str() {
            "90" | "left" => Transform::_90,
            "180" | "inverted" => Transform::_180,
            "270" | "right" => Transform::_270,
            "flipped" => Transform::Flipped,
            "flipped-90" => Transform::Flipped90,
            "flipped-180" => Transform::Flipped180,
            "flipped-270" => Transform::Flipped270,
            _ => output.current_transform(),
        };
        let scale = if rule.scale > 0.0 {
            rule.scale.clamp(0.5, 4.0)
        } else {
            output.current_scale().fractional_scale()
        };
        output.change_current_state(
            None,
            Some(transform),
            Some(smithay::output::Scale::Fractional(scale)),
            Some((rule.position_x, rule.position_y).into()),
        );
    }
}

pub fn update_primary_scanout_output(
    space: &Space<WindowElement>,
    output: &Output,
    cursor: &CursorImageStatus,
    states: &RenderElementStates,
) {
    use smithay::backend::renderer::element::default_primary_scanout_output_compare;
    use smithay::desktop::utils::update_surface_primary_scanout_output;
    space.elements().for_each(|window| {
        window.0.with_surfaces(|surface, states_map| {
            update_surface_primary_scanout_output(
                surface,
                output,
                states_map,
                None,
                states,
                default_primary_scanout_output_compare,
            );
        });
    });
    if let CursorImageStatus::Surface(surface) = cursor {
        smithay::desktop::utils::with_surfaces_surface_tree(surface, |surface, states_map| {
            let _ = (surface, states_map);
        });
    }
}

pub fn take_presentation_feedback(
    output: &Output,
    space: &Space<WindowElement>,
    states: &RenderElementStates,
) -> smithay::desktop::utils::OutputPresentationFeedback {
    use smithay::desktop::utils::{
        OutputPresentationFeedback, surface_primary_scanout_output,
        surface_presentation_feedback_flags_from_states,
    };
    let mut feedback = OutputPresentationFeedback::new(output);
    space.elements().for_each(|window| {
        if space.outputs_for_element(window).contains(output) {
            window.0.take_presentation_feedback(
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| surface_presentation_feedback_flags_from_states(surface, None, states),
            );
        }
    });
    let map = layer_map_for_output(output);
    for layer in map.layers() {
        layer.take_presentation_feedback(
            &mut feedback,
            surface_primary_scanout_output,
            |surface, _| surface_presentation_feedback_flags_from_states(surface, None, states),
        );
    }
    feedback
}



impl<BackendData: Backend + 'static> TextInputActivation for KyonState<BackendData> {}

impl<BackendData: crate::state::Backend + 'static> smithay::wayland::virtual_keyboard::VirtualKeyboardHandler
    for KyonState<BackendData>
{
    fn process_virtual_keyboard_event(
        &mut self,
        _event: smithay::backend::input::InputEvent<smithay::wayland::virtual_keyboard::VirtualKeyboardBackend>,
    ) {
    }
}

smithay::delegate_dispatch2!(@<BackendData: Backend + 'static> KyonState<BackendData>);


fn resolve_binary(program: &str) -> std::path::PathBuf {
    if program.contains('/') {
        return std::path::PathBuf::from(program);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sibling = dir.join(program);
        if sibling.exists() {
            return sibling;
        }
    }
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = std::path::Path::new(dir).join(program);
        if candidate.exists() {
            return candidate;
        }
    }
    for fallback in ["/usr/bin", "/usr/local/bin", "/bin"] {
        let candidate = std::path::Path::new(fallback).join(program);
        if candidate.exists() {
            return candidate;
        }
    }
    std::path::PathBuf::from(program)
}
