use std::{
    collections::HashMap,
    os::fd::OwnedFd,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use kyon_ipc::{DisplayInfo, DisplayMode as DisplayModeInfo, OutputRule};
use smithay::{
    backend::{
        allocator::{
            Fourcc,
            format::FormatSet,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{DrmDevice, DrmDeviceFd, DrmDeviceNotifier, DrmEvent, GbmBufferedSurface},
        egl::{EGLContext, EGLDisplay},
        input::{
            Event, InputBackend, InputEvent, KeyboardKeyEvent, PointerAxisEvent,
            PointerButtonEvent, PointerMotionEvent,
        },
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            Bind, Color32F, Frame, ImportDma, ImportEgl, ImportMem, ImportMemWl, Renderer, Texture,
            damage::OutputDamageTracker,
            element::RenderElement,
            gles::GlesRenderer,
        },
        session::{
            Event as SessionEvent, Session,
            libseat::{LibSeatSession, LibSeatSessionNotifier},
        },
        udev::{UdevBackend, UdevEvent},
    },
    output::{Mode as OutputMode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::EventLoop,
        drm::control::{Mode as DrmMode, connector, crtc},
        input::DeviceCapability,
        rustix::fs::OFlags,
        wayland_server::{
            Display, DisplayHandle, Resource,
            backend::ObjectId,
            protocol::wl_surface,
        },
    },
    utils::{DeviceFd, IsAlive, Size, Transform, SERIAL_COUNTER},
    wayland::{
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        seat::WaylandFocus,
    },
};
use smithay_drm_extras::drm_scanner::{DrmScanEvent, DrmScanner};
use tracing::{error, info, warn};

use crate::{
    config,
    render,
    state::{Backend, KyonState},
};

struct GpuData {
    renderer: GlesRenderer,
    drm: DrmDevice,
    gbm: GbmDevice<DrmDeviceFd>,
    outputs: Vec<OutputData>,
    shaders: Option<render::Shaders>,
}

struct OutputData {
    output: Output,
    surface: Option<GbmBufferedSurface<GbmAllocator<DrmDeviceFd>, ()>>,
    damage_tracker: OutputDamageTracker,
    crtc: crtc::Handle,
    connector: connector::Handle,
    modes: Vec<(DrmMode, DisplayModeInfo)>,
    wallpaper: Option<render::WallpaperCache>,
    cursor: Option<render::CursorTexture>,
    titlebars: HashMap<ObjectId, render::TitlebarCache>,
    enabled: bool,
}

pub struct UdevData {
    gpus: Vec<GpuData>,
    dmabuf: (DmabufState, DmabufGlobal),
    session: LibSeatSession,
    keyboards: Vec<smithay::reexports::input::Device>,
    needs_redraw: bool,
}

impl std::fmt::Debug for UdevData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let outputs: Vec<String> = self
            .gpus
            .iter()
            .flat_map(|gpu| gpu.outputs.iter().map(|output| output.output.name().to_string()))
            .collect();
        f.debug_struct("UdevData").field("outputs", &outputs).finish()
    }
}

impl Backend for UdevData {
    fn seat_name(&self) -> String {
        self.session.seat()
    }

    fn reset_buffers(&mut self, output: &Output) {
        for gpu in &mut self.gpus {
            for item in &mut gpu.outputs {
                if item.output == *output {
                    if let Some(surface) = &mut item.surface {
                        surface.reset_buffers();
                    }
                }
            }
        }
        self.needs_redraw = true;
    }

    fn early_import(&mut self, _surface: &wl_surface::WlSurface) {}

    fn update_led_state(&mut self, led_state: smithay::input::keyboard::LedState) {
        for keyboard in self.keyboards.iter_mut() {
            keyboard.led_update(led_state.into());
        }
    }

    fn switch_vt(&mut self, vt: i32) {
        if let Err(err) = self.session.change_vt(vt) {
            warn!(?err, vt, "failed to switch virtual terminal");
        }
    }

    fn configure_output(&mut self, output: &Output, rule: &OutputRule) {
        let Some(gpu_index) = self
            .gpus
            .iter()
            .position(|gpu| gpu.outputs.iter().any(|item| item.output == *output))
        else {
            return;
        };
        let Some(output_index) = self.gpus[gpu_index]
            .outputs
            .iter()
            .position(|item| item.output == *output)
        else {
            return;
        };
        let (crtc, connector_handle) = {
            let item = &self.gpus[gpu_index].outputs[output_index];
            (item.crtc, item.connector)
        };
        let mut item = self.gpus[gpu_index].outputs.remove(output_index);
        item.enabled = rule.enabled;
        if !rule.enabled {
            if let Some(surface) = &mut item.surface {
                surface.reset_buffers();
            }
            self.gpus[gpu_index].outputs.insert(output_index, item);
            return;
        }
        let transform = transform_for(&rule.transform, item.output.current_transform());
        let scale = if rule.scale > 0.0 {
            rule.scale.clamp(0.5, 4.0)
        } else {
            item.output.current_scale().fractional_scale()
        };
        let selected = if rule.mode.trim().is_empty() {
            None
        } else {
            matching_mode(&item.modes, &rule.mode)
        };
        if let Some((drm_mode, mode_info)) = selected {
            let mode = OutputMode {
                size: (mode_info.width, mode_info.height).into(),
                refresh: (mode_info.refresh * 1000).max(1000),
            };
            let changed = item
                .output
                .current_mode()
                .map(|current| current.size != mode.size || current.refresh != mode.refresh)
                .unwrap_or(true);
            item.output.change_current_state(
                Some(mode),
                Some(transform),
                Some(Scale::Fractional(scale)),
                Some((rule.position_x, rule.position_y).into()),
            );
            item.output.set_preferred(mode);
            if changed {
                item.surface.take();
                let gpu = &mut self.gpus[gpu_index];
                match make_gbm_surface(
                    &mut gpu.drm,
                    &gpu.gbm,
                    &gpu.renderer,
                    crtc,
                    connector_handle,
                    drm_mode,
                ) {
                    Ok(surface) => {
                        item.surface = Some(surface);
                        item.damage_tracker = OutputDamageTracker::from_output(&item.output);
                    }
                    Err(err) => {
                        warn!(?err, output = %output.name(), "failed to switch output mode")
                    }
                }
            }
        } else {
            item.output.change_current_state(
                None,
                Some(transform),
                Some(Scale::Fractional(scale)),
                Some((rule.position_x, rule.position_y).into()),
            );
        }
        item.damage_tracker = OutputDamageTracker::from_output(&item.output);
        self.gpus[gpu_index].outputs.insert(output_index, item);
    }
}

impl DmabufHandler for KyonState<UdevData> {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.backend_data.dmabuf.0
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: smithay::backend::allocator::dmabuf::Dmabuf,
        notifier: ImportNotifier,
    ) {
        let mut imported = false;
        for gpu in &mut self.backend_data.gpus {
            if gpu.renderer.import_dmabuf(&dmabuf, None).is_ok() {
                imported = true;
            }
        }
        if imported {
            let _ = notifier.successful::<Self>();
        } else {
            notifier.failed();
        }
    }
}

pub fn run_udev() -> Result<(), String> {
    let _runtime = ensure_runtime_dir()?;
    let mut event_loop = match EventLoop::try_new() {
        Ok(event_loop) => event_loop,
        Err(err) => {
            error!(?err, "failed to create event loop");
            return Err(format!("failed to create event loop: {err}"));
        }
    };
    let display = match Display::new() {
        Ok(display) => display,
        Err(err) => {
            error!(?err, "failed to create wayland display");
            return Err(format!("failed to create wayland display: {err}"));
        }
    };
    let mut display_handle = display.handle();

    let (mut session, session_notifier) = match LibSeatSession::new() {
        Ok(session) => session,
        Err(err) => {
            error!(?err, "failed to open a libseat session; run from a free tty");
            return Err("failed to open a libseat session; run from a free tty".into());
        }
    };

    let loaded = config::load();
    let candidates = drm_candidates(&session, &loaded);
    if candidates.is_empty() {
        return Err("no usable DRM card devices found".into());
    }
    let mut gpus = Vec::new();
    let mut notifiers: Vec<DrmDeviceNotifier> = Vec::new();
    let mut displays = Vec::new();
    let mut auto_x = 0;
    let mut used_names = Vec::new();
    let mut last_error = String::new();
    for path in candidates {
        match open_gpu(
            &mut session,
            &path,
            &loaded,
            &display.handle(),
            &mut auto_x,
            &mut used_names,
        ) {
            Ok((gpu, notifier, gpu_displays)) => {
                info!(
                    path = %path.display(),
                    outputs = gpu.outputs.len(),
                    "using DRM device"
                );
                gpus.push(gpu);
                notifiers.push(notifier);
                displays.extend(gpu_displays);
            }
            Err(err) => {
                warn!(?err, path = %path.display(), "skipping DRM device");
                last_error = err;
            }
        }
    }
    if gpus.is_empty() {
        return Err(if last_error.is_empty() {
            "no connected DRM outputs found".to_string()
        } else {
            format!("no usable DRM outputs found: {last_error}")
        });
    }

    let mut formats = gpus[0].renderer.dmabuf_formats();
    for gpu in &gpus[1..] {
        let other = gpu.renderer.dmabuf_formats();
        let intersection: FormatSet = formats.intersection(&other).copied().collect();
        if intersection.iter().next().is_some() {
            formats = intersection;
        }
    }
    let mut dmabuf_state = DmabufState::new();
    let dmabuf_global =
        dmabuf_state.create_global::<KyonState<UdevData>>(&display.handle(), formats);

    let data = UdevData {
        gpus,
        dmabuf: (dmabuf_state, dmabuf_global),
        session,
        keyboards: Vec::new(),
        needs_redraw: true,
    };

    let mut state = KyonState::new(
        display,
        event_loop.handle(),
        event_loop.get_signal(),
        data,
        true,
    );
    let shm_formats = state
        .backend_data
        .gpus
        .first_mut()
        .map(|gpu| gpu.renderer.shm_formats());
    if let Some(formats) = shm_formats {
        state.shm_state.update_formats(formats);
    }
    state.displays = displays;
    state.redraw_hook = Some(render_all);
    let outputs: Vec<Output> = state
        .backend_data
        .gpus
        .iter()
        .flat_map(|gpu| gpu.outputs.iter().map(|item| item.output.clone()))
        .collect();
    for output in &outputs {
        state.add_output(output);
    }
    state.apply_output_settings();
    if let Some(output) = state.primary_output() {
        let geometry = state.output_geometry(&output);
        state.pointer_location = (
            geometry.loc.x as f64 + geometry.size.w as f64 / 2.0,
            geometry.loc.y as f64 + geometry.size.h as f64 / 2.0,
        )
            .into();
    }

    unsafe {
        if let Some(socket) = state.socket_name.clone() {
            std::env::set_var("WAYLAND_DISPLAY", socket);
        }
        std::env::set_var("XDG_CURRENT_DESKTOP", "KyonDE");
        std::env::set_var("XDG_SESSION_DESKTOP", "kyonde");
        std::env::set_var("XDG_SESSION_TYPE", "wayland");
    }

    let libinput_session: LibinputSessionInterface<LibSeatSession> =
        state.backend_data.session.clone().into();
    let mut libinput_context = smithay::reexports::input::Libinput::new_with_udev(libinput_session);
    if let Err(err) = libinput_context.udev_assign_seat(&state.backend_data.session.seat()) {
        error!(?err, "failed to assign libinput seat");
        return Err("failed to assign libinput seat".to_string());
    }
    let libinput_backend = LibinputInputBackend::new(libinput_context.clone());
    let libinput_session_for_events = libinput_context.clone();
    event_loop
        .handle()
        .insert_source(libinput_backend, move |event, _, data| {
            if let InputEvent::DeviceAdded { device } = &event
                && device.has_capability(DeviceCapability::Keyboard)
            {
                data.backend_data.keyboards.push(device.clone());
            }
            if let InputEvent::DeviceRemoved { device } = &event
                && device.has_capability(DeviceCapability::Keyboard)
            {
                data.backend_data.keyboards.retain(|item| item != device);
            }
            handle_input(data, event);
        })
        .map_err(|err| format!("failed to add libinput source: {err:?}"))?;

    let mut notifier_libinput = libinput_session_for_events;
    event_loop
        .handle()
        .insert_source(session_notifier, move |event, _, data| match event {
            SessionEvent::PauseSession => {
                data.suppressed_keys.clear();
                let _ = notifier_libinput.suspend();
                for gpu in &mut data.backend_data.gpus {
                    gpu.drm.pause();
                }
            }
            SessionEvent::ActivateSession => {
                data.suppressed_keys.clear();
                data.release_all_keys();
                let _ = notifier_libinput.resume();
                for gpu in &mut data.backend_data.gpus {
                    if let Err(err) = gpu.drm.activate(false) {
                        warn!(?err, "failed to activate DRM device");
                    }
                    for item in &mut gpu.outputs {
                        if let Some(surface) = &mut item.surface {
                            surface.reset_buffers();
                        }
                    }
                }
                render_all(data);
            }
        })
        .map_err(|err| format!("failed to add session source: {err:?}"))?;

    for (gpu_index, notifier) in notifiers.into_iter().enumerate() {
        event_loop
            .handle()
            .insert_source(notifier, move |event, _, data| match event {
                DrmEvent::VBlank(target) => {
                    let Some(output_index) = data.backend_data.gpus[gpu_index]
                        .outputs
                        .iter()
                        .position(|item| item.crtc == target)
                    else {
                        return;
                    };
                    let active = data.backend_data.gpus[gpu_index].drm.is_active();
                    {
                        let item = &mut data.backend_data.gpus[gpu_index].outputs[output_index];
                        let Some(surface) = &mut item.surface else {
                            return;
                        };
                        let _ = surface.frame_submitted();
                        if !item.enabled || !active {
                            return;
                        }
                    }
                    let _ = render_udev(data, gpu_index, output_index);
                }
                DrmEvent::Error(err) => warn!(?err, "DRM event error"),
            })
            .map_err(|err| format!("failed to add DRM event source: {err:?}"))?;
    }

    let udev_backend = UdevBackend::new(&state.backend_data.session.seat());
    if let Ok(udev_backend) = udev_backend {
        let _ = event_loop.handle().insert_source(udev_backend, |event, _, _state| {
            match event {
                UdevEvent::Added { device_id, path } => {
                    info!(device_id, path = %path.display(), "DRM device added; restart to use it")
                }
                UdevEvent::Removed { device_id } => {
                    info!(device_id, "DRM device removed")
                }
                UdevEvent::Changed { device_id } => {
                    info!(device_id, "DRM device changed")
                }
            }
        });
    }

    let execs = state.config.exec.clone();
    for command in execs {
        state.spawn(&command);
    }

    render_all(&mut state);

    if let Err(err) = event_loop.run(None, &mut state, move |_| {}) {
        error!(?err, "udev event loop failed");
    }
    if let Some(path) = &state.ipc_path {
        let _ = std::fs::remove_file(path);
    }
    let _ = display_handle.flush_clients();
    Ok(())
}

fn open_gpu(
    session: &mut LibSeatSession,
    path: &Path,
    config: &kyon_ipc::KyonConfig,
    display: &DisplayHandle,
    auto_x: &mut i32,
    used_names: &mut Vec<String>,
) -> Result<(GpuData, DrmDeviceNotifier, Vec<DisplayInfo>), String> {
    let session_fd = session
        .open(
            path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .map_err(|err| format!("failed to open DRM device: {err}"))?;
    let fd = DrmDeviceFd::new(DeviceFd::from(OwnedFd::from(session_fd)));
    let (mut drm, notifier) = DrmDevice::new(fd.clone(), false)
        .map_err(|err| format!("failed to initialize DRM device: {err}"))?;
    let gbm = GbmDevice::new(fd)
        .map_err(|err| format!("failed to initialize GBM device: {err}"))?;
    relax_egl_vendor(path);
    let egl_display = unsafe { EGLDisplay::new(gbm.clone()) }.map_err(|err| {
        format!(
            "failed to initialize EGL display for {}: {err}",
            path.display()
        )
    })?;
    let egl_context =
        EGLContext::new(&egl_display).map_err(|err| format!("failed to initialize EGL context: {err}"))?;
    let mut renderer = unsafe { GlesRenderer::new(egl_context) }
        .map_err(|err| format!("failed to initialize OpenGL ES renderer: {err}"))?;
    if let Err(err) = renderer.bind_wl_display(display) {
        warn!(?err, "failed to bind EGL to wayland display");
    }

    let mut scanner: DrmScanner = DrmScanner::new();
    let events = scanner
        .scan_connectors(&drm)
        .map_err(|err| format!("failed to scan DRM connectors: {err}"))?;
    let mut outputs = Vec::new();
    let mut displays = Vec::new();
    for event in events {
        let DrmScanEvent::Connected {
            connector,
            crtc: Some(crtc),
        } = event
        else {
            continue;
        };
        if connector.modes().is_empty() {
            continue;
        }
        let mut output_name = format!(
            "{}-{}",
            connector.interface().as_str(),
            connector.interface_id()
        );
        if used_names.contains(&output_name) {
            let card = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("card");
            output_name = format!("{output_name}-{card}");
        }
        used_names.push(output_name.clone());
        let mut modes = Vec::new();
        for (index, mode) in connector.modes().iter().enumerate() {
            let size = mode.size();
            modes.push((
                *mode,
                DisplayModeInfo {
                    width: size.0 as i32,
                    height: size.1 as i32,
                    refresh: mode.vrefresh() as i32,
                    preferred: index == 0,
                },
            ));
        }
        let rule = config.outputs.iter().find(|rule| rule.name == output_name);
        let (drm_mode, mode_info) = matching_mode(&modes, rule.map(|rule| rule.mode.as_str()).unwrap_or(""))
            .unwrap_or_else(|| modes[0].clone());
        let scale = rule
            .map(|rule| rule.scale.clamp(0.5, 4.0))
            .unwrap_or(1.0);
        let transform = transform_for(
            rule.map(|rule| rule.transform.as_str()).unwrap_or(""),
            Transform::Normal,
        );
        let rotated = matches!(
            transform,
            Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270
        );
        let (logical_w, _logical_h) = if rotated {
            (mode_info.height, mode_info.width)
        } else {
            (mode_info.width, mode_info.height)
        };
        let logical_w = (logical_w as f64 / scale).round().max(1.0) as i32;
        let position = rule
            .map(|rule| (rule.position_x, rule.position_y))
            .unwrap_or((*auto_x, 0));
        *auto_x = (*auto_x).max(position.0 + logical_w);
        let surface = match make_gbm_surface(&mut drm, &gbm, &renderer, crtc, connector.handle(), drm_mode) {
            Ok(surface) => surface,
            Err(err) => {
                warn!(?err, %output_name, "failed to create DRM surface");
                continue;
            }
        };
        let output = Output::new(
            output_name.clone(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "KyonDE".into(),
                model: output_name.clone(),
                serial_number: String::new(),
            },
        );
        let output_mode = OutputMode {
            size: (mode_info.width, mode_info.height).into(),
            refresh: (mode_info.refresh * 1000).max(1000),
        };
        output.create_global::<KyonState<UdevData>>(display);
        output.change_current_state(
            Some(output_mode),
            Some(transform),
            Some(Scale::Fractional(scale)),
            Some(position.into()),
        );
        output.set_preferred(output_mode);
        let damage_tracker = OutputDamageTracker::from_output(&output);
        info!(
            output = %output_name,
            width = mode_info.width,
            height = mode_info.height,
            refresh = mode_info.refresh,
            scale,
            x = position.0,
            y = position.1,
            "using DRM output"
        );
        displays.push(DisplayInfo {
            name: output_name.clone(),
            description: output_name,
            width: mode_info.width,
            height: mode_info.height,
            refresh: mode_info.refresh,
            scale,
            x: position.0,
            y: position.1,
            enabled: rule.map(|rule| rule.enabled).unwrap_or(true),
            primary: rule.map(|rule| rule.primary).unwrap_or(false),
            modes: modes.iter().map(|(_, info)| info.clone()).collect(),
        });
        outputs.push(OutputData {
            output,
            surface: Some(surface),
            damage_tracker,
            crtc,
            connector: connector.handle(),
            modes,
            wallpaper: None,
            cursor: None,
            titlebars: HashMap::new(),
            enabled: rule.map(|rule| rule.enabled).unwrap_or(true),
        });
    }
    if outputs.is_empty() {
        return Err("no connected outputs on DRM device".into());
    }
    Ok((
        GpuData {
            renderer,
            drm,
            gbm,
            outputs,
            shaders: None,
        },
        notifier,
        displays,
    ))
}

fn make_gbm_surface(
    drm: &mut DrmDevice,
    gbm: &GbmDevice<DrmDeviceFd>,
    renderer: &GlesRenderer,
    crtc: crtc::Handle,
    connector: connector::Handle,
    mode: DrmMode,
) -> Result<GbmBufferedSurface<GbmAllocator<DrmDeviceFd>, ()>, String> {
    let drm_surface = drm
        .create_surface(crtc, mode, &[connector])
        .map_err(|err| format!("failed to create DRM surface: {err}"))?;
    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    GbmBufferedSurface::new(
        drm_surface,
        allocator,
        &[Fourcc::Abgr8888, Fourcc::Argb8888],
        renderer.dmabuf_formats(),
    )
    .map_err(|err| format!("failed to create GBM buffered surface: {err}"))
}

fn matching_mode(modes: &[(DrmMode, DisplayModeInfo)], spec: &str) -> Option<(DrmMode, DisplayModeInfo)> {
    let spec = spec.trim();
    if spec.is_empty() {
        return modes.first().cloned();
    }
    let (size, refresh) = spec.split_once('@').unwrap_or((spec, ""));
    let (width, height) = size.split_once('x').unwrap_or((size, ""));
    let width = width.trim().parse::<i32>().ok()?;
    let height = height.trim().parse::<i32>().ok()?;
    let refresh = refresh
        .trim()
        .parse::<f32>()
        .ok()
        .map(|value| value.round() as i32);
    modes
        .iter()
        .find(|(_, info)| {
            info.width == width
                && info.height == height
                && refresh
                    .map(|refresh| (info.refresh - refresh).abs() <= 1)
                    .unwrap_or(true)
        })
        .cloned()
}

fn transform_for(value: &str, fallback: Transform) -> Transform {
    match value.to_ascii_lowercase().as_str() {
        "90" | "left" => Transform::_90,
        "180" | "inverted" => Transform::_180,
        "270" | "right" => Transform::_270,
        "flipped" => Transform::Flipped,
        "flipped-90" => Transform::Flipped90,
        "flipped-180" => Transform::Flipped180,
        "flipped-270" => Transform::Flipped270,
        _ => fallback,
    }
}

fn drm_candidates(session: &LibSeatSession, config: &kyon_ipc::KyonConfig) -> Vec<PathBuf> {
    let mut values = Vec::new();
    let mut configured = config.gpu.drm_devices.trim().to_string();
    if configured.is_empty() {
        if let Ok(value) = std::env::var("AQ_DRM_DEVICES") {
            configured = value;
        } else if let Ok(value) = std::env::var("WLR_DRM_DEVICES") {
            configured = value;
        }
    }
    if !configured.trim().is_empty() {
        for value in configured.split(':') {
            if !value.trim().is_empty() {
                values.push(normalize_drm_path(value.trim()));
            }
        }
    } else {
        if let Ok(Some(primary)) = smithay::backend::udev::primary_gpu(session.seat()) {
            values.push(normalize_drm_path(primary));
        }
        if let Ok(gpus) = smithay::backend::udev::all_gpus(session.seat()) {
            for path in gpus {
                values.push(normalize_drm_path(path));
            }
        }
    }
    let mut result: Vec<PathBuf> = Vec::new();
    for path in values {
        let is_card = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with("card"))
            .unwrap_or(false);
        if !is_card || !path.exists() || result.contains(&path) {
            continue;
        }
        result.push(path);
    }
    result
}

fn handle_input<B: InputBackend>(state: &mut KyonState<UdevData>, event: InputEvent<B>) {
    match event {
        InputEvent::Keyboard { event, .. } => state.keyboard_key::<B>(event),
        InputEvent::PointerMotion { event, .. } => state.pointer_motion_relative::<B>(event),
        InputEvent::PointerMotionAbsolute { event, .. } => {
            if let Some(output) = state.primary_output() {
                state.pointer_motion_absolute::<B>(event, &output);
            }
        }
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

fn render_all(state: &mut KyonState<UdevData>) {
    let gpu_count = state.backend_data.gpus.len();
    for gpu_index in 0..gpu_count {
        let output_count = state.backend_data.gpus[gpu_index].outputs.len();
        for output_index in 0..output_count {
            if state.backend_data.gpus[gpu_index].outputs[output_index].enabled {
                let _ = render_udev(state, gpu_index, output_index);
            }
        }
    }
}

fn render_udev(
    state: &mut KyonState<UdevData>,
    gpu_index: usize,
    output_index: usize,
) -> Result<(), String> {
    let config = state.config.clone();
    let output = state.backend_data.gpus[gpu_index].outputs[output_index]
        .output
        .clone();
    let scale = output.current_scale().fractional_scale();
    let output_physical = render::physical_size(&output);
    let output_geometry = state.output_geometry(&output);
    let origin_x = output_geometry.loc.x as f64;
    let origin_y = output_geometry.loc.y as f64;
    let cursor_status = state.cursor_status.clone();
    let cursor_location = state.pointer_location;
    let cursor_here = output_geometry.to_f64().contains(cursor_location);
    let now = Instant::now();
    let mode = state.active_workspace().mode;
    let output_name = output.name().to_string();
    let order: Vec<crate::window::WindowElement> = state
        .active_workspace()
        .order
        .iter()
        .filter(|window| window.meta_ref().output == output_name)
        .cloned()
        .collect();
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

    {
        let GpuData {
            renderer,
            outputs,
            shaders,
            ..
        } = &mut state.backend_data.gpus[gpu_index];
        let item = &mut outputs[output_index];
        if shaders.is_none() {
            *shaders = render::ensure_shaders(renderer);
        }
        render::ensure_wallpaper(
            renderer,
            &config,
            output_physical,
            scale,
            &mut item.wallpaper,
        )
        .ok_or_else(|| "wallpaper unavailable".to_string())?;
        render::ensure_cursor(renderer, scale, &mut item.cursor);

        let wallpaper = item
            .wallpaper
            .as_ref()
            .map(|cache| render::WallpaperElement::new(cache.texture.clone(), output_physical));
        let space_elements = smithay::desktop::space::space_render_elements::<
            GlesRenderer,
            crate::window::WindowElement,
            _,
        >(
            renderer,
            [&state.composer[state.active_workspace].space],
            &output,
            1.0,
        )
        .map_err(|err| format!("space render failed: {err}"))?;

        let mut elements: Vec<render::OutputElement> = Vec::new();
        if cursor_here
            && !matches!(cursor_status, smithay::input::pointer::CursorImageStatus::Hidden)
            && let Some(cursor) = item.cursor.as_ref()
        {
            elements.push(render::OutputElement::Cursor(render::CursorElement::new(
                cursor.texture.clone(),
                smithay::utils::Point::from((
                    ((cursor_location.x - origin_x) * scale) as i32,
                    ((cursor_location.y - origin_y) * scale) as i32,
                )),
            )));
        }

        let mut titlebars = Vec::new();
        for window in order.iter() {
            if !window.alive() {
                continue;
            }
            let meta = window.meta_ref();
            let tb = crate::decor::titlebar_height(&config, &meta, mode);
            if tb <= 0 {
                continue;
            }
            let Some(surface) = window.wl_surface() else {
                continue;
            };
            let active = focused.as_ref() == Some(window);
            let width_px = (meta.outer.size.w as f64 * scale).round().max(1.0) as i32;
            let key = crate::decor::titlebar_key(&config, &meta, width_px, scale, active, mode);
            let id = surface.id();
            let needs = item
                .titlebars
                .get(&id)
                .map(|cache| cache.key != key)
                .unwrap_or(true);
            if needs
                && let Some(image) =
                    crate::decor::render_titlebar(&config, &meta, width_px, scale, active)
                && let Ok(texture) = renderer.import_memory(
                    &image.pixels,
                    Fourcc::Abgr8888,
                    Size::from((image.width, image.height)),
                    false,
                )
            {
                item.titlebars
                    .insert(id.clone(), render::TitlebarCache { key, texture });
            }
            if let Some(cache) = item.titlebars.get(&id) {
                let sampled = meta.sampled_rect(now);
                let image_size = cache.texture.size();
                let dest = smithay::utils::Rectangle::new(
                    smithay::utils::Point::from((
                        ((sampled.loc.x - origin_x) * scale) as i32,
                        ((sampled.loc.y - origin_y) * scale) as i32,
                    )),
                    smithay::utils::Size::from((
                        (sampled.size.w * scale).round().max(1.0) as i32,
                        (image_size.h as f64
                            * (sampled.size.h / meta.outer.size.h.max(1) as f64))
                            .round()
                            .max(1.0) as i32,
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

        let mut corner_patches = Vec::new();
        if config.decor.rounding > 0
            && let Some(wallpaper) = item.wallpaper.as_ref()
        {
            let r = (config.decor.rounding as f64 * scale).round() as i32;
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
                let x = ((sampled.loc.x - origin_x) * scale).round() as i32;
                let y = ((sampled.loc.y - origin_y) * scale).round() as i32;
                let w = (sampled.size.w * scale).round() as i32;
                let h = (sampled.size.h * scale).round() as i32;
                if w <= r * 2 || h <= r * 2 {
                    continue;
                }
                for (cx, cy, idx) in [
                    (x, y, 0usize),
                    (x + w - r, y, 1),
                    (x, y + h - r, 2),
                    (x + w - r, y + h - r, 3),
                ] {
                    if let Some(pixels) = render::corner_patch(wallpaper, cx, cy, r, idx)
                        && let Ok(texture) = renderer.import_memory(
                            &pixels,
                            Fourcc::Abgr8888,
                            Size::from((r, r)),
                            false,
                        )
                    {
                        corner_patches.push(render::TitlebarElement::new(
                            texture,
                            smithay::utils::Point::from((cx, cy)),
                            smithay::utils::Size::from((r, r)),
                            1.0,
                        ));
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

        let (mut dmabuf, _age) = item
            .surface
            .as_mut()
            .ok_or_else(|| "output has no surface".to_string())?
            .next_buffer()
            .map_err(|err| format!("failed to get next buffer: {err:?}"))?;
        let mut target = renderer
            .bind(&mut dmabuf)
            .map_err(|err| format!("failed to bind buffer: {err:?}"))?;
        let result = item
            .damage_tracker
            .render_output(
                renderer,
                &mut target,
                0,
                &elements,
                Color32F::new(0.0, 0.0, 0.0, 1.0),
            )
            .map_err(|err| format!("render failed: {err:?}"))?;
        let damage = result.damage.cloned();
        let sync = result.sync;
        drop(target);
        item.surface
            .as_mut()
            .ok_or_else(|| "output has no surface".to_string())?
            .queue_buffer(Some(sync), damage, ())
            .map_err(|err| format!("failed to queue buffer: {err:?}"))?;
    }

    let time = Duration::from_millis(state.last_tick.elapsed().as_millis() as u64);
    state.post_render(&output, time);
    state.composer[state.active_workspace].space.refresh();
    state.popups.cleanup();
    state.frame_count = state.frame_count.wrapping_add(1);
    if state.frame_count % 60 == 0 {
        state.check_config_reload();
    }
    state.backend_data.needs_redraw = false;
    Ok(())
}

struct RuntimeDir(Option<PathBuf>);

impl Drop for RuntimeDir {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn ensure_runtime_dir() -> Result<RuntimeDir, String> {
    if let Some(value) = std::env::var_os("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(value);
        if path.is_dir() {
            return Ok(RuntimeDir(None));
        }
    }
    let path = std::env::temp_dir().join(format!("kyonde-runtime-{}", std::process::id()));
    std::fs::create_dir_all(&path)
        .map_err(|err| format!("failed to create runtime directory {}: {err}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|err| format!("failed to secure runtime directory {}: {err}", path.display()))?;
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", &path);
    }
    info!(path = %path.display(), "using private runtime directory");
    Ok(RuntimeDir(Some(path)))
}

fn normalize_drm_path(value: impl AsRef<Path>) -> PathBuf {
    let path = value.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new("/dev/dri").join(path)
    }
}

fn relax_egl_vendor(path: &Path) {
    let Some(vendors) = std::env::var_os("__EGL_VENDOR_LIBRARY_FILENAMES") else {
        return;
    };
    let vendors = vendors.to_string_lossy().to_string();
    let card = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let vendor_path = format!("/sys/class/drm/{card}/device/vendor");
    let vendor = std::fs::read_to_string(vendor_path)
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    if !vendor.is_empty()
        && vendor != "0x10de"
        && vendors.contains("nvidia")
        && !vendors.contains("mesa")
    {
        warn!(
            vendors,
            vendor,
            path = %path.display(),
            "clearing NVIDIA-only EGL vendor restriction for a non-NVIDIA DRM device"
        );
        unsafe { std::env::remove_var("__EGL_VENDOR_LIBRARY_FILENAMES") };
    }
}
