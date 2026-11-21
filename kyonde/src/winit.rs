use std::{
    sync::atomic::Ordering,
    time::Duration,
};

use smithay::{
    utils::IsAlive,
    reexports::wayland_server::Resource,
    desktop::space::space_render_elements,
    backend::renderer::damage::OutputDamageTracker,
    backend::allocator::Fourcc,
    backend::{
        allocator::dmabuf::Dmabuf,
        input::{
            AbsolutePositionEvent, InputBackend, InputEvent,
        },
        renderer::{
            Color32F, Frame, ImportDma, ImportEgl, ImportMem, ImportMemWl, Texture,
            gles::GlesRenderer,
        },
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    input::pointer::CursorImageStatus,
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::EventLoop,
        wayland_server::{Display, protocol::wl_surface},
        winit::event_loop::pump_events::PumpStatus,
    },
    utils::Transform,
    wayland::{
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        shm::ShmHandler,
    },
};

use crate::{
    render,
    state::{Backend, KyonState},
};

pub struct WinitData {
    backend: WinitGraphicsBackend<GlesRenderer>,
    dmabuf: (DmabufState, DmabufGlobal),
    damage_tracker: OutputDamageTracker,
}

impl Backend for WinitData {
    fn seat_name(&self) -> String {
        "kyon-winit".into()
    }

    fn reset_buffers(&mut self, _output: &Output) {}

    fn early_import(&mut self, _surface: &wl_surface::WlSurface) {}
}

impl DmabufHandler for KyonState<WinitData> {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.backend_data.dmabuf.0
    }

    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, dmabuf: Dmabuf, notifier: ImportNotifier) {
        if self
            .backend_data
            .backend
            .renderer()
            .import_dmabuf(&dmabuf, None)
            .is_ok()
        {
            let _ = notifier.successful::<Self>();
        } else {
            notifier.failed();
        }
    }
}

pub fn run_winit() {
    let mut event_loop = match EventLoop::try_new() {
        Ok(loop_) => loop_,
        Err(err) => {
            tracing::error!(?err, "failed to create event loop");
            return;
        }
    };
    let display = match Display::new() {
        Ok(display) => display,
        Err(err) => {
            tracing::error!(?err, "failed to create wayland display");
            return;
        }
    };
    let mut display_handle = display.handle();

    let (mut backend, mut winit) = match winit::init::<GlesRenderer>() {
        Ok(pair) => pair,
        Err(err) => {
            tracing::error!(?err, "failed to initialize winit");
            return;
        }
    };
    let window_size = backend.window_size();
    let system_scale = backend.scale_factor();
    let mode = Mode {
        size: window_size,
        refresh: 60_000,
    };
    let output = Output::new(
        "kyon-winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "KyonDE".into(),
            model: "Winit".into(),
            serial_number: "0".into(),
        },
    );
    output.create_global::<KyonState<WinitData>>(&display.handle());
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        Some(Scale::Fractional(system_scale)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);

    let _ = backend.renderer().bind_wl_display(&display.handle());
    let mut dmabuf_state = DmabufState::new();
    let dmabuf_global = dmabuf_state
        .create_global::<KyonState<WinitData>>(&display.handle(), backend.renderer().dmabuf_formats());

    let loop_signal = event_loop.get_signal();
    let data = WinitData {
        backend,
        dmabuf: (dmabuf_state, dmabuf_global),
        damage_tracker: OutputDamageTracker::from_output(&output),
    };
    let mut state = KyonState::new(display, event_loop.handle(), loop_signal, data, true);
    state.shm_state.update_formats(state.backend_data.backend.renderer().shm_formats());
    state.add_output(&output);
    state.update_display(&output);
    state.apply_output_settings();
    state.pointer_location = (window_size.w as f64 / 2.0 / system_scale, window_size.h as f64 / 2.0 / system_scale).into();
    unsafe {
        if let Some(socket) = state.socket_name.clone() {
            std::env::set_var("WAYLAND_DISPLAY", socket);
        }
        std::env::set_var("XDG_CURRENT_DESKTOP", "KyonDE");
        std::env::set_var("XDG_SESSION_DESKTOP", "kyonde");
        std::env::set_var("XDG_SESSION_TYPE", "wayland");
    }
    tracing::info!(socket = ?state.socket_name, "kyonde winit backend ready");
    let execs = state.config.exec.clone();
    for command in execs {
        state.spawn(&command);
    }

    let mut last_config_check = std::time::Instant::now();

    while state.running.load(Ordering::SeqCst) {
        let status = winit.dispatch_new_events(|event| match event {
            WinitEvent::Resized { size, scale_factor } => {
                let mode = Mode {
                    size,
                    refresh: 60_000,
                };
                output.change_current_state(
                    Some(mode),
                    Some(Transform::Flipped180),
                    Some(Scale::Fractional(scale_factor)),
                    None,
                );
                output.set_preferred(mode);
                state.update_display(&output);
                state.wallpaper_cache = None;
                state.titlebars.clear();
                state.arrange_active();
            }
            WinitEvent::Input(event) => handle_input(&mut state, event, &output),
            WinitEvent::CloseRequested => {
                state.running.store(false, Ordering::SeqCst);
            }
            WinitEvent::Focus(focused) => {
                if !focused {
                    state.release_all_keys();
                }
            }
            _ => {}
        });

        if let PumpStatus::Exit(_) = status {
            break;
        }

        if let Err(err) = render_frame(&mut state, &output) {
            tracing::warn!(err, "render error");
        }

        state.composer[state.active_workspace].space.refresh();
        state.popups.cleanup();
        let _ = display_handle.flush_clients();

        if last_config_check.elapsed() > Duration::from_millis(900) {
            last_config_check = std::time::Instant::now();
            state.check_config_reload();
        }

        if let Err(err) = event_loop.dispatch(Some(Duration::from_millis(2)), &mut state) {
            tracing::error!(?err, "event loop error");
            break;
        }
    }

    if let Some(path) = &state.ipc_path {
        let _ = std::fs::remove_file(path);
    }
}

fn handle_input<B: InputBackend>(state: &mut KyonState<WinitData>, event: InputEvent<B>, output: &Output) {
    match event {
        InputEvent::Keyboard { event, .. } => state.keyboard_key::<B>(event),
        InputEvent::PointerMotionAbsolute { event, .. } => state.pointer_motion_absolute::<B>(event, output),
        InputEvent::PointerButton { event, .. } => state.pointer_button::<B>(event),
        InputEvent::PointerAxis { event, .. } => state.pointer_axis::<B>(event),
        InputEvent::TouchDown { event, .. } => state.touch_down::<B>(event),
        InputEvent::TouchUp { event, .. } => state.touch_up::<B>(event),
        InputEvent::TouchMotion { event, .. } => state.touch_motion::<B>(event),
        InputEvent::TouchFrame { .. } => state.touch_frame(),
        InputEvent::TouchCancel { .. } => state.touch_cancel(),
        _ => {}
    }
}

fn render_frame(state: &mut KyonState<WinitData>, output: &Output) -> Result<(), String> {
    if !state.output_enabled(output) {
        return Ok(());
    }
    let config = state.config.clone();
    let scale = output.current_scale().fractional_scale();
    let output_physical = render::physical_size(output);
    let cursor_status = state.cursor_status.clone();
    let cursor_location = state.pointer_location;
    let now = std::time::Instant::now();
    let mode = state.active_workspace().mode;
    let order = state.active_workspace().order.clone();
    let focused = state.focused.clone();

    for window in &order {
        let meta = window.meta_ref();
        let tb = crate::decor::titlebar_height(&config, &meta, mode);
        let sampled = meta.sampled_rect(now);
        drop(meta);
        let content_loc = smithay::utils::Point::from((
            sampled.loc.x.round() as i32,
            (sampled.loc.y + tb as f64).round() as i32,
        ));
        let space = &mut state.composer[state.active_workspace].space;
        if space.element_location(window).is_some() {
            space.relocate_element(window, content_loc);
        }
    }

    let renderer = state.backend_data.backend.renderer();
    if state.shaders.is_none() {
        state.shaders = render::ensure_shaders(renderer);
    }
    render::ensure_wallpaper(renderer, &config, output_physical, scale, &mut state.wallpaper_cache)
        .ok_or_else(|| "wallpaper unavailable".to_string())?;
    render::ensure_cursor(renderer, scale, &mut state.cursor_texture);

    let wallpaper = state
        .wallpaper_cache
        .as_ref()
        .map(|cache| render::WallpaperElement::new(cache.texture.clone(), output_physical));
    let space_elements = space_render_elements::<GlesRenderer, crate::window::WindowElement, _>(
        renderer,
        [&state.composer[state.active_workspace].space],
        output,
        1.0,
    )
    .map_err(|err| format!("space render failed: {err}"))?;

    let mut titlebars = Vec::new();
    if config.decor.titlebar.enabled {
        for window in order.iter() {
            if !window.alive() {
                continue;
            }
            let meta = window.meta_ref();
            let tb = crate::decor::titlebar_height(&config, &meta, mode);
            if tb <= 0 {
                continue;
            }
            let Some(surface) = window.wl_surface() else { continue; };
            let active = focused.as_ref() == Some(window);
            let width_px = (meta.outer.size.w as f64 * scale).round().max(1.0) as i32;
            let key = crate::decor::titlebar_key(&config, &meta, width_px, scale, active, mode);
            let id = surface.id();
            let needs = state.titlebars.get(&id).map(|cache| cache.key != key).unwrap_or(true);
            if needs {
                if let Some(image) = crate::decor::render_titlebar(&config, &meta, width_px, scale, active) {
                    if let Ok(texture) = renderer.import_memory(
                        &image.pixels,
                        Fourcc::Abgr8888,
                        smithay::utils::Size::from((image.width, image.height)),
                        false,
                    ) {
                        state.titlebars.insert(id.clone(), render::TitlebarCache { key, texture });
                    }
                }
            }
            if let Some(cache) = state.titlebars.get(&id) {
                let sampled = meta.sampled_rect(now);
                let image_size = cache.texture.size();
                let dest = smithay::utils::Rectangle::new(
                    smithay::utils::Point::from((
                        (sampled.loc.x * scale) as i32,
                        (sampled.loc.y * scale) as i32,
                    )),
                    smithay::utils::Size::from((
                        (sampled.size.w * scale).round().max(1.0) as i32,
                        (image_size.h as f64 * (sampled.size.h / meta.outer.size.h.max(1) as f64)).round().max(1.0) as i32,
                    )),
                );
                let alpha = meta.fade.sample(now) * meta.opacity;
                titlebars.push(render::TitlebarElement::new(
                    cache.texture.clone(),
                    dest.loc,
                    dest.size,
                    alpha,
                ));
            }
        }
    }

    let mut elements: Vec<render::OutputElement> = Vec::new();
    if !matches!(cursor_status, CursorImageStatus::Hidden) {
        if let Some(cursor) = state.cursor_texture.as_ref() {
            elements.push(render::OutputElement::Cursor(render::CursorElement::new(
                cursor.texture.clone(),
                smithay::utils::Point::from((
                    (cursor_location.x * scale) as i32,
                    (cursor_location.y * scale) as i32,
                )),
            )));
        }
    }
    let mut corner_patches = Vec::new();
    if config.decor.rounding > 0 {
        if let Some(wallpaper) = state.wallpaper_cache.as_ref() {
            let r = (config.decor.rounding as f64 * scale).round() as i32;
            let texture = wallpaper.texture.clone();
            for window in order.iter() {
                if !window.alive() {
                    continue;
                }
                let meta = window.meta_ref();
                if meta.fullscreen {
                    continue;
                }
                let sampled = meta.sampled_rect(now);
                drop(meta);
                let x = (sampled.loc.x * scale).round() as i32;
                let y = (sampled.loc.y * scale).round() as i32;
                let w = (sampled.size.w * scale).round() as i32;
                let h = (sampled.size.h * scale).round() as i32;
                if w <= r * 2 || h <= r * 2 {
                    continue;
                }
                let corners = [
                    (x, y, 0usize),
                    (x + w - r, y, 1),
                    (x, y + h - r, 2),
                    (x + w - r, y + h - r, 3),
                ];
                for (cx, cy, idx) in corners {
                    if let Some(pixels) = render::corner_patch(wallpaper, cx, cy, r, idx) {
                        if let Ok(tex) = renderer.import_memory(
                            &pixels,
                            Fourcc::Abgr8888,
                            smithay::utils::Size::from((r, r)),
                            false,
                        ) {
                            let _ = &texture;
                            corner_patches.push(render::TitlebarElement::new(
                                tex,
                                smithay::utils::Point::from((cx, cy)),
                                smithay::utils::Size::from((r, r)),
                                1.0,
                            ));
                        }
                    }
                }
            }
        }
    }
    elements.extend(corner_patches.into_iter().map(render::OutputElement::Titlebar));
    elements.extend(titlebars.into_iter().map(render::OutputElement::Titlebar));
    elements.extend(space_elements.into_iter().map(render::OutputElement::Space));
    if let Some(wallpaper) = wallpaper {
        elements.push(render::OutputElement::Wallpaper(wallpaper));
    }

    let age = 0;
    let (renderer, mut framebuffer) = state
        .backend_data
        .backend
        .bind()
        .map_err(|err| format!("bind failed: {err}"))?;
    let result = state
        .backend_data
        .damage_tracker
        .render_output(
            renderer,
            &mut framebuffer,
            age,
            &elements,
            Color32F::new(0.0, 0.0, 0.0, 1.0),
        )
        .map_err(|err| format!("render failed: {err:?}"))?;
    let damage = result.damage.clone();
    drop(framebuffer);
    if let Some(damage) = damage {
        state
            .backend_data
            .backend
            .submit(Some(&damage))
            .map_err(|err| format!("submit failed: {err}"))?;
    }
    let time = Duration::from_millis(state.last_tick.elapsed().as_millis() as u64);
    state.post_render(output, time);
    state.frame_count = state.frame_count.wrapping_add(1);
    Ok(())
}
