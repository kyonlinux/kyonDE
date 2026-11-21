use std::{cell::RefCell, time::Instant};

use smithay::{
    backend::renderer::{ImportAll, Renderer, element::{AsRenderElements, surface::WaylandSurfaceRenderElement}},
    desktop::{Window, space::SpaceElement},
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{IsAlive, Logical, Point, Rectangle, Scale, Size, user_data::UserDataMap},
    wayland::{compositor::with_states, seat::WaylandFocus, shell::xdg::XdgToplevelSurfaceData},
};

use crate::util::Timed;

#[derive(Debug, Clone, PartialEq)]
pub struct WindowElement(pub Window);

#[derive(Debug, Clone)]
pub struct WindowMeta {
    pub workspace: usize,
    pub output: String,
    pub floating: bool,
    pub fullscreen: bool,
    pub maximized: bool,
    pub ssd: bool,
    pub app_id: String,
    pub title: String,
    pub blur: bool,
    pub opacity: f32,
    pub outer: Rectangle<i32, Logical>,
    pub saved_float: Option<Rectangle<i32, Logical>>,
    pub last_configure: Option<Size<i32, Logical>>,
    pub x: Timed,
    pub y: Timed,
    pub width: Timed,
    pub height: Timed,
    pub fade: Timed,
    pub opened: Instant,
    pub needs_initial_configure: bool,
    pub shell: bool,
    pub placed: bool,
    pub rules_applied: bool,
    pub activated: bool,
}

impl WindowMeta {
    pub fn new(workspace: usize) -> Self {
        Self {
            workspace,
            output: String::new(),
            floating: false,
            fullscreen: false,
            maximized: false,
            ssd: false,
            app_id: String::new(),
            title: String::new(),
            blur: true,
            opacity: 1.0,
            outer: Rectangle::from_size((800, 600).into()),
            saved_float: None,
            last_configure: None,
            x: Timed::new(0.0),
            y: Timed::new(0.0),
            width: Timed::new(800.0),
            height: Timed::new(600.0),
            fade: Timed::new(1.0),
            opened: Instant::now(),
            needs_initial_configure: true,
            shell: false,
            placed: false,
            rules_applied: false,
            activated: false,
        }
    }

    pub fn sampled_rect(&self, now: Instant) -> Rectangle<f64, Logical> {
        Rectangle::new(
            Point::from((
                self.x.sample(now) as f64,
                self.y.sample(now) as f64,
            )),
            Size::from((
                self.width.sample(now).max(1.0) as f64,
                self.height.sample(now).max(1.0) as f64,
            )),
        )
    }

    pub fn target_rect(&self) -> Rectangle<i32, Logical> {
        self.outer
    }
}

impl WindowElement {
    pub fn meta(&self) -> std::cell::RefMut<'_, WindowMeta> {
        self.0.user_data().insert_if_missing(|| RefCell::new(WindowMeta::new(0)));
        self.0.user_data().get::<RefCell<WindowMeta>>().unwrap().borrow_mut()
    }

    pub fn meta_ref(&self) -> std::cell::Ref<'_, WindowMeta> {
        self.0.user_data().insert_if_missing(|| RefCell::new(WindowMeta::new(0)));
        self.0.user_data().get::<RefCell<WindowMeta>>().unwrap().borrow()
    }

    pub fn refresh_identity(&self) -> (String, String) {
        let Some(toplevel) = self.0.toplevel() else {
            return (String::new(), String::new());
        };
        with_states(toplevel.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .map(|data| {
                    let data = data.lock().unwrap();
                    (
                        data.app_id.clone().unwrap_or_default(),
                        data.title.clone().unwrap_or_default(),
                    )
                })
                .unwrap_or_default()
        })
    }

    pub fn send_close(&self) {
        if let Some(toplevel) = self.0.toplevel() {
            toplevel.send_close();
        }
    }

    pub fn wl_surface(&self) -> Option<WlSurface> {
        self.0.wl_surface().map(|s| s.into_owned())
    }

    pub fn configure(&self, size: Size<i32, Logical>, active: bool) {
        let Some(toplevel) = self.0.toplevel() else {
            return;
        };
        toplevel.with_pending_state(|state| {
            state.size = Some(size);
            if active {
                state.states.set(smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Activated);
            } else {
                state.states.unset(smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Activated);
            }
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    pub fn set_activated(&self, active: bool) {
        if let Some(toplevel) = self.0.toplevel() {
            toplevel.with_pending_state(|state| {
                if active {
                    state.states.set(
                        smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Activated,
                    );
                } else {
                    state.states.unset(
                        smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Activated,
                    );
                }
            });
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
    }

    pub fn is_wayland(&self) -> bool {
        self.0.is_wayland()
    }

    pub fn user_data(&self) -> &UserDataMap {
        self.0.user_data()
    }
}

impl IsAlive for WindowElement {
    fn alive(&self) -> bool {
        self.0.alive()
    }
}

impl WaylandFocus for WindowElement {
    fn wl_surface(&self) -> Option<std::borrow::Cow<'_, WlSurface>> {
        self.0.wl_surface()
    }
}

impl SpaceElement for WindowElement {
    fn geometry(&self) -> Rectangle<i32, Logical> {
        SpaceElement::geometry(&self.0)
    }
    fn bbox(&self) -> Rectangle<i32, Logical> {
        SpaceElement::bbox(&self.0)
    }
    fn is_in_input_region(&self, point: &Point<f64, Logical>) -> bool {
        SpaceElement::is_in_input_region(&self.0, point)
    }
    fn z_index(&self) -> u8 {
        SpaceElement::z_index(&self.0)
    }
    fn set_activate(&self, activated: bool) {
        SpaceElement::set_activate(&self.0, activated);
    }
    fn output_enter(&self, output: &Output, overlap: Rectangle<i32, Logical>) {
        SpaceElement::output_enter(&self.0, output, overlap);
    }
    fn output_leave(&self, output: &Output) {
        SpaceElement::output_leave(&self.0, output);
    }
    fn refresh(&self) {
        SpaceElement::refresh(&self.0);
    }
}

impl From<Window> for WindowElement {
    fn from(window: Window) -> Self {
        Self(window)
    }
}

impl From<&WindowElement> for WlSurface {
    fn from(window: &WindowElement) -> Self {
        window.wl_surface().unwrap_or_else(|| panic!("window without surface"))
    }
}


impl<R> AsRenderElements<R> for WindowElement
where
    R: Renderer + ImportAll,
    R::TextureId: Clone + 'static,
{
    type RenderElement = WaylandSurfaceRenderElement<R>;

    fn render_elements<C: From<Self::RenderElement>>(
        &self,
        renderer: &mut R,
        location: Point<i32, smithay::utils::Physical>,
        scale: Scale<f64>,
        alpha: f32,
    ) -> Vec<C> {
        self.0.render_elements(renderer, location, scale, alpha)
    }
}
