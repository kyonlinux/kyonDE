use std::env;

use kyon_ipc::KyonConfig;
use tracing::{info, warn};

pub fn apply(config: &KyonConfig) {
    for item in &config.env {
        if !item.key.is_empty() {
            unsafe { env::set_var(&item.key, &item.value) };
        }
    }

    let mut drm_devices = config.gpu.drm_devices.trim().to_string();
    if drm_devices.is_empty() {
        if let Ok(value) = env::var("AQ_DRM_DEVICES") {
            drm_devices = value;
        } else if let Ok(value) = env::var("WLR_DRM_DEVICES") {
            drm_devices = value;
        }
    }
    if !drm_devices.is_empty() {
        set_if_absent("AQ_DRM_DEVICES", &drm_devices);
        set_if_absent("WLR_DRM_DEVICES", &drm_devices);
        info!(devices = %drm_devices, "DRM device list configured");
    }
    if !config.gpu.render_node.is_empty() {
        set_if_absent("KYONDE_RENDER_NODE", &config.gpu.render_node);
        info!(node = %config.gpu.render_node, "preferred render node configured");
    }

    let gpus = detect_gpus();
    for gpu in &gpus {
        info!(vendor = %gpu.0, device = %gpu.1, "detected GPU");
    }
    let nvidia = gpus.iter().any(|gpu| gpu.0.eq_ignore_ascii_case("0x10de"));
    if nvidia {
        unsafe { env::set_var("KYONDE_GPU_NVIDIA", "1") };
        if config.gpu.prefer_nvidia {
            info!("NVIDIA GPU detected; leaving EGL vendor selection to the active DRM device");
        }
    }

    if config.gpu.backend.eq_ignore_ascii_case("software") {
        unsafe {
            env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
            env::set_var("KYONDE_GPU_BACKEND", "software");
        }
        warn!("software rendering requested; hardware acceleration disabled");
    } else {
        unsafe { env::set_var("KYONDE_GPU_BACKEND", "opengl-es") };
    }

    let vulkan = vulkan_devices();
    match config.gpu.backend.to_ascii_lowercase().as_str() {
        "vulkan" => match &vulkan {
            Some(devices) if !devices.is_empty() => {
                warn!(
                    devices = ?devices,
                    "Vulkan device discovery succeeded, but Smithay 0.7 has no Vulkan scene renderer; falling back to OpenGL ES"
                );
            }
            _ => warn!("Vulkan requested but no Vulkan device was found; falling back to OpenGL ES"),
        },
        "opengl" | "gles" => info!("OpenGL ES renderer selected"),
        "auto" => {
            if let Some(devices) = &vulkan {
                if !devices.is_empty() {
                    info!(
                        devices = ?devices,
                        "Vulkan capable GPU detected; using the OpenGL ES renderer with Vulkan-backed driver stack"
                    );
                }
            }
        }
        other => warn!(backend = other, "unknown GPU backend, using automatic selection"),
    }

    if config.gpu.damage_tracking {

    }
    if config.gpu.max_fps > 0 {
        info!(max_fps = config.gpu.max_fps, "frame cap configured");
    }
}

fn set_if_absent(key: &str, value: &str) {
    if env::var_os(key).is_none() {
        unsafe { env::set_var(key, value) };
    }
}

fn detect_gpus() -> Vec<(String, String)> {
    let mut result = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return result;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let base = entry.path().join("device");
        let vendor = std::fs::read_to_string(base.join("vendor"))
            .map(|v| v.trim().to_string())
            .unwrap_or_default();
        let device = std::fs::read_to_string(base.join("device"))
            .map(|v| v.trim().to_string())
            .unwrap_or_default();
        if !vendor.is_empty() {
            result.push((vendor, device));
        }
    }
    result
}

#[cfg(feature = "vulkan-probe")]
fn vulkan_devices() -> Option<Vec<String>> {
    use smithay::backend::vulkan::version::Version;
    use smithay::backend::vulkan::{Instance, PhysicalDevice};
    let instance = Instance::new(Version::VERSION_1_1, None).ok()?;
    let devices = PhysicalDevice::enumerate(&instance)
        .ok()?
        .map(|device| format!("{} ({:?})", device.name(), device.ty()))
        .collect::<Vec<_>>();
    Some(devices)
}

#[cfg(not(feature = "vulkan-probe"))]
fn vulkan_devices() -> Option<Vec<String>> {
    None
}
