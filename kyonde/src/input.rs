use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, Event, InputBackend,
        KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent, TouchEvent,
    },
    desktop::{WindowSurfaceType, layer_map_for_output},
    input::{
        keyboard::{FilterResult, Keysym, ModifiersState, xkb},
        pointer::{
            AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
            GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
            GestureSwipeUpdateEvent, GrabStartData, MotionEvent, PointerGrab,
            PointerInnerHandle, RelativeMotionEvent,
        },
        touch::{DownEvent, MotionEvent as TouchMotionEvent, TouchGrab, UpEvent},
    },
    reexports::wayland_server::protocol::wl_pointer,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Serial, Size},
    wayland::{
        keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitorSeat,
        shell::wlr_layer::{KeyboardInteractivity, Layer as WlrLayer},
    },
};

use crate::{
    layout::LayoutMode,
    state::{Backend, KyonState},
    window::WindowElement,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorButton {
    Close,
    Maximize,
    Minimize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorHit {
    Titlebar(Option<DecorButton>),
    Edge { top: bool, bottom: bool, left: bool, right: bool },
}

#[derive(Clone)]
struct ParsedBind {
    ctrl: bool,
    alt: bool,
    shift: bool,
    logo: bool,
    key: Keysym,
    ascii: Option<char>,
    action: String,
}

fn parse_mods(value: &str) -> (bool, bool, bool, bool) {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut logo = false;
    for part in value.split('+').map(|p| p.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "ctrl" | "control" | "ctl" => ctrl = true,
            "alt" | "mod1" | "option" => alt = true,
            "shift" => shift = true,
            "super" | "logo" | "meta" | "win" | "mod4" | "cmd" | "command" => logo = true,
            _ => {}
        }
    }
    (ctrl, alt, shift, logo)
}

fn parse_key(value: &str) -> Keysym {
    let direct = xkb::keysym_from_name(value, 0);
    if direct != Keysym::NoSymbol {
        return direct;
    }
    let mut chars = value.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        let lower = xkb::keysym_from_name(&ch.to_lowercase().to_string(), 0);
        if lower != Keysym::NoSymbol {
            return lower;
        }
        let upper = xkb::keysym_from_name(&ch.to_uppercase().to_string(), 0);
        if upper != Keysym::NoSymbol {
            return upper;
        }
    }
    Keysym::NoSymbol
}

impl<BackendData: Backend + 'static> KyonState<BackendData> {
    fn parsed_binds(&self) -> Vec<ParsedBind> {
        self.config
            .binds
            .iter()
            .filter(|bind| !bind.key.is_empty())
            .map(|bind| {
                let (ctrl, alt, shift, logo) = parse_mods(&bind.mods);
                let key = parse_key(&bind.key);
                let ascii = bind
                    .key
                    .chars()
                    .next()
                    .filter(|_| bind.key.chars().count() == 1)
                    .map(|ch| ch.to_ascii_lowercase());
                ParsedBind {
                    ctrl,
                    alt,
                    shift,
                    logo,
                    key,
                    ascii,
                    action: bind.action.clone(),
                }
            })
            .collect()
    }

    pub fn keyboard_key<B: InputBackend>(&mut self, event: B::KeyboardKeyEvent) {
        let keycode = event.key_code();
        let state = event.state();
        let serial = SERIAL_COUNTER.next_serial();
        let time = Event::time(&event);
        let keyboard = self.seat.get_keyboard().unwrap();

        for layer in self.layer_shell_state.layer_surfaces().rev() {
            let exclusive = layer.with_cached_state(|data| {
                data.keyboard_interactivity == KeyboardInteractivity::Exclusive
                    && matches!(data.layer, WlrLayer::Top | WlrLayer::Overlay)
            });
            if exclusive {
                let surface = self.outputs.iter().find_map(|output| {
                    let map = layer_map_for_output(output);
                    map.layers()
                        .find(|l| l.layer_surface() == &layer)
                        .cloned()
                });
                if let Some(surface) = surface {
                    keyboard.set_focus(self, Some(surface.wl_surface().clone()), serial);
                    let _ = keyboard.input::<(), _>(self, keycode, state, serial, time, |_, _, _| {
                        FilterResult::Forward
                    });
                    return;
                }
            }
        }

        let inhibited = self
            .active_workspace()
            .space
            .element_under(self.pointer_location)
            .and_then(|(window, _)| window.wl_surface())
            .and_then(|surface| self.seat.keyboard_shortcuts_inhibitor_for_surface(&surface))
            .map(|inhibitor| inhibitor.is_active())
            .unwrap_or(false);

        let binds = self.parsed_binds();
        let mut suppressed = self.suppressed_keys.clone();
        let mut action: Option<String> = None;
        let mut vt_switch: Option<i32> = None;
        let result = keyboard
            .input(self, keycode, state, serial, time, |_, modifiers, handle| {
                let keysym = handle.modified_sym();
                if state == KeyState::Pressed {
                    if modifiers.ctrl
                        && modifiers.alt
                        && (smithay::input::keyboard::xkb::keysyms::KEY_XF86Switch_VT_1
                            ..=smithay::input::keyboard::xkb::keysyms::KEY_XF86Switch_VT_12)
                            .contains(&keysym.raw())
                    {
                        vt_switch = Some(
                            (keysym.raw() - smithay::input::keyboard::xkb::keysyms::KEY_XF86Switch_VT_1 + 1)
                                as i32,
                        );
                        suppressed.push(keysym);
                        return FilterResult::Intercept(());
                    }
                    if inhibited {
                        return FilterResult::Forward;
                    }
                    if suppressed.contains(&keysym) {
                        return FilterResult::Intercept(());
                    }
                    if let Some(bind) = match_bind(&binds, *modifiers, &handle, keysym) {
                        suppressed.push(keysym);
                        action = Some(bind.action.clone());
                        FilterResult::Intercept(())
                    } else {
                        FilterResult::Forward
                    }
                } else if suppressed.contains(&keysym) {
                    suppressed.retain(|k| k != &keysym);
                    FilterResult::Intercept(())
                } else {
                    FilterResult::Forward
                }
            });
        self.suppressed_keys = suppressed;
        let _ = result;
        if let Some(vt) = vt_switch {
            self.backend_data.switch_vt(vt);
        } else if let Some(action) = action {
            self.process_action(&action);
        }
    }

    pub fn release_all_keys(&mut self) {
        let keyboard = self.seat.get_keyboard().unwrap();
        for keycode in keyboard.pressed_keys() {
            let _ = keyboard.input(
                self,
                keycode,
                KeyState::Released,
                SERIAL_COUNTER.next_serial(),
                smithay::backend::input::InputTime::now(),
                |_, _, _| FilterResult::Forward::<()>,
            );
        }
    }

    pub fn pointer_motion_absolute<B: InputBackend>(
        &mut self,
        event: B::PointerMotionAbsoluteEvent,
        output: &smithay::output::Output,
    ) {
        let geo = self.output_geometry(output);
        let location = event.position_transformed(geo.size) + geo.loc.to_f64();
        self.pointer_motion_to(location, Event::time(&event));
    }

    pub fn pointer_motion_relative<B: InputBackend>(&mut self, event: B::PointerMotionEvent) {
        let mut location = self.pointer_location + event.delta();
        if let Some(output) = self.primary_output() {
            let geo = self.output_geometry(&output);
            location.x = location.x.clamp(geo.loc.x as f64, (geo.loc.x + geo.size.w) as f64 - 1.0);
            location.y = location.y.clamp(geo.loc.y as f64, (geo.loc.y + geo.size.h) as f64 - 1.0);
        }
        self.pointer_location = location;
        let serial = SERIAL_COUNTER.next_serial();
        let under = self.surface_under(location);
        let pointer = self.pointer.clone();
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location,
                serial,
                time: Event::time(&event),
            },
        );
        pointer.frame(self);
    }

    pub fn pointer_motion_to(&mut self, location: Point<f64, Logical>, time: smithay::backend::input::InputTime) {
        self.pointer_location = location;
        let serial = SERIAL_COUNTER.next_serial();
        let under = self.surface_under(location);
        let pointer = self.pointer.clone();
        pointer.motion(
            self,
            under,
            &MotionEvent { location, serial, time },
        );
        pointer.frame(self);
    }

    pub fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(smithay::reexports::wayland_server::protocol::wl_surface::WlSurface, Point<f64, Logical>)> {
        let output = self
            .primary_output()
            .filter(|output| self.output_geometry(output).to_f64().contains(pos))?;
        let output_geo = self.output_geometry(&output);
        let local = pos - output_geo.loc.to_f64();
        let layers = layer_map_for_output(&output);
        if let Some(layer) = layers
            .layer_under(WlrLayer::Overlay, local)
            .or_else(|| layers.layer_under(WlrLayer::Top, local))
        {
            let layer_loc = layers.layer_geometry(layer).unwrap_or_default().loc;
            if let Some((surface, loc)) =
                layer.surface_under(local - layer_loc.to_f64(), WindowSurfaceType::ALL)
            {
                return Some((surface, loc.to_f64() + layer_loc.to_f64() + output_geo.loc.to_f64()));
            }
        }

        if let Some((window, window_loc)) = self.active_workspace().space.element_under(pos) {
            let meta = window.meta_ref();
            let title = crate::decor::titlebar_height(&self.config, &meta, self.active_workspace().mode);
            let outer = meta.outer;
            drop(meta);
            if title > 0 {
                let bar = Rectangle::new(outer.loc, Size::from((outer.size.w, title)));
                if bar.to_f64().contains(pos) {
                    return None;
                }
            }
            if let Some((surface, loc)) = window
                .0
                .surface_under(pos - window_loc.to_f64(), WindowSurfaceType::ALL)
            {
                return Some((surface, loc.to_f64() + window_loc.to_f64()));
            }
        }

        if let Some(layer) = layers
            .layer_under(WlrLayer::Bottom, local)
            .or_else(|| layers.layer_under(WlrLayer::Background, local))
        {
            let layer_loc = layers.layer_geometry(layer).unwrap_or_default().loc;
            if let Some((surface, loc)) =
                layer.surface_under(local - layer_loc.to_f64(), WindowSurfaceType::ALL)
            {
                return Some((surface, loc.to_f64() + layer_loc.to_f64() + output_geo.loc.to_f64()));
            }
        }
        None
    }

    pub fn update_keyboard_focus(&mut self, pos: Point<f64, Logical>) {
        let serial = SERIAL_COUNTER.next_serial();
        if let Some(output) = self.primary_output() {
            let output_geo = self.output_geometry(&output);
            let local = pos - output_geo.loc.to_f64();
            let layers = layer_map_for_output(&output);
            if let Some(layer) = layers
                .layer_under(WlrLayer::Overlay, local)
                .or_else(|| layers.layer_under(WlrLayer::Top, local))
            {
                if layer.can_receive_keyboard_focus() {
                    let surface = layer.wl_surface().clone();
                    self.seat.get_keyboard().unwrap().set_focus(self, Some(surface), serial);
                    return;
                }
            }
        }

        if let Some((window, _)) = self
            .active_workspace()
            .space
            .element_under(pos)
            .map(|(w, l)| (w.clone(), l))
        {
            if self.active_workspace().mode == LayoutMode::Floating {
                self.active_workspace_mut().space.raise_element(&window, true);
            }
            if self.focused.as_ref() != Some(&window) {
                self.focus_window(Some(window), serial);
            } else {
                self.seat
                    .get_keyboard()
                    .unwrap()
                    .set_focus(self, window.wl_surface(), serial);
            }
            return;
        }

        self.seat
            .get_keyboard()
            .unwrap()
            .set_focus(self, Option::<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface>::None, serial);
    }

    pub fn decoration_under(&self, pos: Point<f64, Logical>) -> Option<(WindowElement, DecorHit)> {
        let mode = self.active_workspace().mode;
        for window in self.active_workspace().order.iter().rev() {
            let meta = window.meta_ref();
            if meta.fullscreen || meta.shell {
                continue;
            }
            let title = crate::decor::titlebar_height(&self.config, &meta, mode);
            let outer = meta.outer;
            let floating = meta.floating || mode == LayoutMode::Floating;
            drop(meta);
            if title > 0 {
                let bar = Rectangle::new(outer.loc, Size::from((outer.size.w, title)));
                if bar.to_f64().contains(pos) {
                    let mut button = None;
                    for (kind, rect) in crate::decor::titlebar_button_layout(
                        &self.config,
                        outer.size.w,
                        title,
                    ) {
                        let abs = Rectangle::new(outer.loc + rect.loc, rect.size);
                        if abs.to_f64().contains(pos) {
                            button = Some(match kind {
                                crate::decor::TitlebarButton::Close => DecorButton::Close,
                                crate::decor::TitlebarButton::Maximize => DecorButton::Maximize,
                                crate::decor::TitlebarButton::Minimize => DecorButton::Minimize,
                            });
                            break;
                        }
                    }
                    return Some((window.clone(), DecorHit::Titlebar(button)));
                }
            }
            if !floating {
                continue;
            }
            let edge = 7.0;
            if outer.to_f64().contains(pos) {
                let left = (pos.x - outer.loc.x as f64) <= edge;
                let right = (outer.loc.x + outer.size.w) as f64 - pos.x <= edge;
                let top = (pos.y - outer.loc.y as f64) <= edge;
                let bottom = (outer.loc.y + outer.size.h) as f64 - pos.y <= edge;
                if left || right || top || bottom {
                    let title_zone = title as f64;
                    if top && pos.y - outer.loc.y as f64 > title_zone {
                        return Some((window.clone(), DecorHit::Edge { top, bottom, left, right }));
                    }
                    if title <= 0 {
                        return Some((window.clone(), DecorHit::Edge { top, bottom, left, right }));
                    }
                }
            }
        }
        None
    }

    pub fn pointer_button<B: InputBackend>(&mut self, event: B::PointerButtonEvent) {
        let serial = SERIAL_COUNTER.next_serial();
        let button = event.button_code();
        let button_state = event.state();
        let state = wl_pointer::ButtonState::from(button_state);
        let location = self.pointer_location;

        if state == wl_pointer::ButtonState::Pressed && !self.pointer.is_grabbed() {
            self.update_keyboard_focus(location);
            if let Some((window, hit)) = self.decoration_under(location) {
                let meta = window.meta_ref();
                let floating = meta.floating || self.active_workspace().mode == LayoutMode::Floating;
                let outer = meta.outer;
                drop(meta);
                match hit {
                    DecorHit::Titlebar(Some(DecorButton::Close)) => {
                        window.send_close();
                        return;
                    }
                    DecorHit::Titlebar(Some(DecorButton::Maximize)) => {
                        self.focus_window(Some(window.clone()), serial);
                        self.toggle_maximize();
                        return;
                    }
                    DecorHit::Titlebar(Some(DecorButton::Minimize)) => {
                        return;
                    }
                    DecorHit::Titlebar(None) => {
                        if button == 0x110 {
                            self.focus_window(Some(window.clone()), serial);
                            if floating {
                                self.begin_move(window, serial, location, button, true);
                                return;
                            }
                        }
                    }
                    DecorHit::Edge { .. } => {
                        self.focus_window(Some(window.clone()), serial);
                        if floating {
                            self.begin_resize(window, serial, location, button, outer, hit);
                            return;
                        }
                    }
                }
            }

            let modifiers = self.seat.get_keyboard().unwrap().modifier_state();
            if modifiers.logo && button == 0x110 {
                if let Some((window, _)) = self
                    .active_workspace()
                    .space
                    .element_under(location)
                    .map(|(w, l)| (w.clone(), l))
                {
                    let meta = window.meta_ref();
                    let floating = meta.floating || self.active_workspace().mode == LayoutMode::Floating;
                    let outer = meta.outer;
                    drop(meta);
                    self.focus_window(Some(window.clone()), serial);
                    if floating {
                        let hit = self.decoration_under(location).map(|(_, h)| h);
                        if let Some(hit @ DecorHit::Edge { .. }) = hit {
                            self.begin_resize(window, serial, location, button, outer, hit);
                        } else {
                            self.begin_move(window, serial, location, button, true);
                        }
                        return;
                    }
                }
            }
        }

        let pointer = self.pointer.clone();
        pointer.button(
            self,
            &ButtonEvent {
                button,
                state: button_state,
                serial,
                time: event.time(),
            },
        );
        pointer.frame(self);
    }

    pub fn pointer_axis<B: InputBackend>(&mut self, event: B::PointerAxisEvent) {
        let mut vertical = event
            .amount(Axis::Vertical)
            .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.0);
        let mut horizontal = event
            .amount(Axis::Horizontal)
            .unwrap_or_else(|| event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.0);
        if self.config.input.natural_scroll {
            vertical = -vertical;
            horizontal = -horizontal;
        }
        let modifiers = self.seat.get_keyboard().unwrap().modifier_state();
        if modifiers.logo {
            if vertical.abs() > 0.0 {
                self.scroll_focus(vertical < 0.0);
            } else if horizontal.abs() > 0.0 {
                self.scroll_focus(horizontal > 0.0);
            }
            return;
        }
        let source = event.source();
        let mut frame = AxisFrame::new(event.time()).source(source);
        if horizontal != 0.0 {
            frame = frame.value(Axis::Horizontal, horizontal);
            if let Some(v120) = event.amount_v120(Axis::Horizontal) {
                frame = frame.v120(Axis::Horizontal, v120 as i32);
            }
        }
        if vertical != 0.0 {
            frame = frame.value(Axis::Vertical, vertical);
            if let Some(v120) = event.amount_v120(Axis::Vertical) {
                frame = frame.v120(Axis::Vertical, v120 as i32);
            }
        }
        if source == AxisSource::Finger {
            if event.amount(Axis::Horizontal) == Some(0.0) {
                frame = frame.stop(Axis::Horizontal);
            }
            if event.amount(Axis::Vertical) == Some(0.0) {
                frame = frame.stop(Axis::Vertical);
            }
        }
        let pointer = self.pointer.clone();
        pointer.axis(self, frame);
        pointer.frame(self);
    }

    pub fn touch_down<B: InputBackend>(&mut self, event: B::TouchDownEvent) {
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        let Some(location) = self.touch_location(&event) else {
            return;
        };
        self.update_keyboard_focus(location);
        let serial = SERIAL_COUNTER.next_serial();
        let under = self.surface_under(location);
        touch.down(
            self,
            under,
            &DownEvent {
                slot: event.slot(),
                location,
                serial,
                time: event.time(),
            },
        );
    }

    pub fn touch_up<B: InputBackend>(&mut self, event: B::TouchUpEvent) {
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        touch.up(
            self,
            &UpEvent {
                slot: event.slot(),
                serial,
                time: event.time(),
            },
        );
    }

    pub fn touch_motion<B: InputBackend>(&mut self, event: B::TouchMotionEvent) {
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        let Some(location) = self.touch_location(&event) else {
            return;
        };
        let under = self.surface_under(location);
        touch.motion(
            self,
            under,
            &TouchMotionEvent {
                slot: event.slot(),
                location,
                time: event.time(),
            },
        );
    }

    pub fn touch_frame(&mut self) {
        if let Some(touch) = self.seat.get_touch() {
            touch.frame(self);
        }
    }

    pub fn touch_cancel(&mut self) {
        if let Some(touch) = self.seat.get_touch() {
            touch.cancel(self);
        }
    }

    fn touch_location<B: InputBackend, E: AbsolutePositionEvent<B>>(
        &self,
        event: &E,
    ) -> Option<Point<f64, Logical>> {
        let output = self.primary_output()?;
        let geo = self.output_geometry(&output);
        let transform = output.current_transform();
        let size = transform.invert().transform_size(geo.size);
        Some(
            transform.transform_point_in(event.position_transformed(size), &size.to_f64())
                + geo.loc.to_f64(),
        )
    }

    fn begin_move(
        &mut self,
        window: WindowElement,
        serial: Serial,
        location: Point<f64, Logical>,
        button: u32,
        was_floating: bool,
    ) {
        if !was_floating {
            return;
        }
        let mut meta = window.meta();
        meta.saved_float = Some(meta.outer);
        let start_outer = meta.outer;
        drop(meta);
        let start_data = GrabStartData {
            focus: None,
            button,
            location,
        };
        let pointer = self.pointer.clone();
        pointer.set_grab(
            self,
            MoveGrab::<BackendData> {
                start_data,
                window,
                initial_outer: start_outer,
                manual: true,
            },
            serial,
            smithay::input::pointer::Focus::Clear,
        );
    }

    fn begin_resize(
        &mut self,
        window: WindowElement,
        serial: Serial,
        location: Point<f64, Logical>,
        button: u32,
        outer: Rectangle<i32, Logical>,
        hit: DecorHit,
    ) {
        let DecorHit::Edge { top, bottom, left, right } = hit else {
            return;
        };
        {
            let meta_ref = window.meta_ref();
            let floating = meta_ref.floating || self.active_workspace().mode == LayoutMode::Floating;
            drop(meta_ref);
            if !floating {
                return;
            }
            let mut meta = window.meta();
            meta.saved_float = Some(meta.outer);
        }
        let titlebar = self.config.decor.titlebar.height;
        let start_data = GrabStartData {
            focus: None,
            button,
            location,
        };
        let pointer = self.pointer.clone();
        pointer.set_grab(
            self,
            ResizeGrab::<BackendData> {
                start_data,
                window,
                initial_outer: outer,
                top,
                bottom,
                left,
                right,
                manual: true,
                titlebar,
            },
            serial,
            smithay::input::pointer::Focus::Clear,
        );
    }

    pub fn start_move(
        &mut self,
        surface: smithay::wayland::shell::xdg::ToplevelSurface,
        seat: &smithay::input::Seat<Self>,
        serial: Serial,
    ) {
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        let Some(pointer) = seat.get_pointer() else {
            return;
        };
        if !pointer.has_grab(serial) {
            return;
        }
        let Some(start_data) = pointer.grab_start_data() else {
            return;
        };
        let meta = window.meta_ref();
        let was_floating = meta.floating || self.active_workspace().mode == LayoutMode::Floating;
        let initial_outer = meta.outer;
        drop(meta);
        if !was_floating {
            return;
        }
        window.meta().saved_float = Some(initial_outer);
        let grab = MoveGrab::<BackendData> {
            start_data,
            window,
            initial_outer,
            manual: false,
        };
        pointer.set_grab(self, grab, serial, smithay::input::pointer::Focus::Clear);
    }

    pub fn start_resize(
        &mut self,
        surface: smithay::wayland::shell::xdg::ToplevelSurface,
        seat: &smithay::input::Seat<Self>,
        serial: Serial,
        edges: smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::ResizeEdge,
    ) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::ResizeEdge;
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        let Some(pointer) = seat.get_pointer() else {
            return;
        };
        if !pointer.has_grab(serial) {
            return;
        }
        let Some(start_data) = pointer.grab_start_data() else {
            return;
        };
        let meta = window.meta_ref();
        let floating = meta.floating || self.active_workspace().mode == LayoutMode::Floating;
        let initial_outer = meta.outer;
        drop(meta);
        if !floating {
            return;
        }
        window.meta().saved_float = Some(initial_outer);
        let grab = ResizeGrab::<BackendData> {
            start_data,
            window,
            initial_outer,
            top: matches!(edges, ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight),
            bottom: matches!(edges, ResizeEdge::Bottom | ResizeEdge::BottomLeft | ResizeEdge::BottomRight),
            left: matches!(edges, ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft),
            right: matches!(edges, ResizeEdge::Right | ResizeEdge::TopRight | ResizeEdge::BottomRight),
            manual: false,
            titlebar: self.config.decor.titlebar.height,
        };
        pointer.set_grab(self, grab, serial, smithay::input::pointer::Focus::Clear);
    }
}

fn match_bind<'a>(
    binds: &'a [ParsedBind],
    modifiers: ModifiersState,
    handle: &smithay::input::keyboard::KeysymHandle<'_>,
    keysym: Keysym,
) -> Option<&'a ParsedBind> {
    let raw = handle.raw_latin_sym_or_raw_current_sym().unwrap_or(keysym);
    binds.iter().find(|bind| {
        bind.ctrl == modifiers.ctrl
            && bind.alt == modifiers.alt
            && bind.shift == modifiers.shift
            && bind.logo == modifiers.logo
            && (bind.key == keysym
                || bind.key == raw
                || bind.ascii.is_some_and(|ch| {
                    raw.key_char()
                        .map(|c| c.to_ascii_lowercase() == ch)
                        .unwrap_or(false)
                }))
    })
}

struct MoveGrab<BackendData: Backend + 'static> {
    start_data: GrabStartData<KyonState<BackendData>>,
    window: WindowElement,
    initial_outer: Rectangle<i32, Logical>,
    manual: bool,
}

impl<BackendData: Backend + 'static> PointerGrab<KyonState<BackendData>> for MoveGrab<BackendData> {
    fn motion(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        _focus: Option<(smithay::reexports::wayland_server::protocol::wl_surface::WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let delta = event.location - self.start_data.location;
        let mut new_loc = self.initial_outer.loc.to_f64() + delta;
        if let Some(output) = data.primary_output() {
            let geo = data.output_geometry(&output).to_f64();
            new_loc.x = new_loc.x.clamp(geo.loc.x - 20.0, geo.loc.x + geo.size.w - 60.0);
            new_loc.y = new_loc.y.clamp(geo.loc.y, geo.loc.y + geo.size.h - 30.0);
        }
        let loc = new_loc.to_i32_round();
        let mut meta = self.window.meta();
        meta.outer.loc = loc;
        meta.saved_float = Some(meta.outer);
        meta.x.snap(loc.x as f32);
        meta.y.snap(loc.y as f32);
        let title = crate::decor::titlebar_height(&data.config, &meta, data.active_workspace().mode);
        drop(meta);
        let content_loc = Point::from((loc.x, loc.y + title));
        let space = &mut data.composer[data.active_workspace].space;
        if space.element_location(&self.window).is_some() {
            space.relocate_element(&self.window, content_loc);
        } else {
            space.map_element(self.window.clone(), content_loc, true);
        }
    }

    fn relative_motion(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        focus: Option<(smithay::reexports::wayland_server::protocol::wl_surface::WlSurface, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }

    fn button(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &ButtonEvent,
    ) {
        if self.manual {
            if event.state == smithay::backend::input::ButtonState::Released {
                handle.unset_grab(self, data, event.serial, event.time, false);
                data.arrange_active();
            }
        } else {
            handle.button(data, event);
            if handle.current_pressed().is_empty() {
                handle.unset_grab(self, data, event.serial, event.time, true);
            }
        }
    }

    fn axis(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        details: AxisFrame,
    ) {
        handle.axis(data, details);
    }

    fn frame(&mut self, data: &mut KyonState<BackendData>, handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event);
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event);
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event);
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event);
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event);
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event);
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event);
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event);
    }

    fn start_data(&self) -> &GrabStartData<KyonState<BackendData>> {
        &self.start_data
    }

    fn unset(&mut self, _data: &mut KyonState<BackendData>) {}
}

struct ResizeGrab<BackendData: Backend + 'static> {
    start_data: GrabStartData<KyonState<BackendData>>,
    window: WindowElement,
    initial_outer: Rectangle<i32, Logical>,
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
    manual: bool,
    titlebar: i32,
}

impl<BackendData: Backend + 'static> PointerGrab<KyonState<BackendData>> for ResizeGrab<BackendData> {
    fn motion(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        _focus: Option<(smithay::reexports::wayland_server::protocol::wl_surface::WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let delta = event.location - self.start_data.location;
        let mut x = self.initial_outer.loc.x as f64;
        let mut y = self.initial_outer.loc.y as f64;
        let mut w = self.initial_outer.size.w as f64;
        let mut h = self.initial_outer.size.h as f64;
        if self.left {
            x += delta.x;
            w -= delta.x;
        }
        if self.right {
            w += delta.x;
        }
        if self.top {
            y += delta.y;
            h -= delta.y;
        }
        if self.bottom {
            h += delta.y;
        }
        if w < 160.0 {
            if self.left {
                x -= 160.0 - w;
            }
            w = 160.0;
        }
        if h < 120.0 {
            if self.top {
                y -= 120.0 - h;
            }
            h = 120.0;
        }
        let outer = Rectangle::new(
            (x.round() as i32, y.round() as i32).into(),
            Size::from((w.round() as i32, h.round() as i32)),
        );
        let titlebar = if self.titlebar > 0 { self.titlebar } else { 0 };
        let content = Size::from(((outer.size.w).max(1), (outer.size.h - titlebar).max(1)));
        {
            let mut meta = self.window.meta();
            meta.outer = outer;
            meta.saved_float = Some(outer);
            meta.floating = true;
            meta.x.snap(outer.loc.x as f32);
            meta.y.snap(outer.loc.y as f32);
            meta.width.snap(outer.size.w as f32);
            meta.height.snap(outer.size.h as f32);
            meta.last_configure = Some(content);
        }
        self.window.configure(content, true);
        let content_loc = Point::from((outer.loc.x, outer.loc.y + titlebar));
        let space = &mut data.composer[data.active_workspace].space;
        if space.element_location(&self.window).is_some() {
            space.relocate_element(&self.window, content_loc);
        } else {
            space.map_element(self.window.clone(), content_loc, true);
        }
    }

    fn relative_motion(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        focus: Option<(smithay::reexports::wayland_server::protocol::wl_surface::WlSurface, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }

    fn button(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &ButtonEvent,
    ) {
        if self.manual {
            if event.state == smithay::backend::input::ButtonState::Released {
                handle.unset_grab(self, data, event.serial, event.time, false);
                data.arrange_active();
            }
        } else {
            handle.button(data, event);
            if handle.current_pressed().is_empty() {
                handle.unset_grab(self, data, event.serial, event.time, true);
            }
        }
    }

    fn axis(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        details: AxisFrame,
    ) {
        handle.axis(data, details);
    }

    fn frame(&mut self, data: &mut KyonState<BackendData>, handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event);
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event);
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event);
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event);
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event);
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event);
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event);
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut KyonState<BackendData>,
        handle: &mut PointerInnerHandle<'_, KyonState<BackendData>>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event);
    }

    fn start_data(&self) -> &GrabStartData<KyonState<BackendData>> {
        &self.start_data
    }

    fn unset(&mut self, _data: &mut KyonState<BackendData>) {}
}

