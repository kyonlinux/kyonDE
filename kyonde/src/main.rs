#![allow(dead_code, unused_imports, unused_variables, unused_mut)]

mod config;
mod decor;
mod gpu;
mod handlers;
mod input;
mod layout;
mod render;
mod state;
#[cfg(feature = "udev")]
mod udev;
mod util;
mod window;
#[cfg(feature = "winit")]
mod winit;

use tracing_subscriber::EnvFilter;

#[cfg(feature = "udev")]
fn run_tty() {
    if let Err(err) = udev::run_udev() {
        eprintln!("kyonde: {err}");
        std::process::exit(1);
    }
}

fn main() {
    if let Ok(filter) = EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(filter).compact().init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("info,kyonde=debug,smithay=warn"))
            .compact()
            .init();
    }

    config::ensure_config();
    let loaded = config::load();
    gpu::apply(&loaded);
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("--version") | Some("-V") | Some("version") => {
            println!("kyonde {}", kyon_ipc::VERSION);
        }
        Some("--help") | Some("-h") | Some("help") => {
            println!("kyonde {} - KyonDE compositor", kyon_ipc::VERSION);
            println!("usage: kyonde [winit|tty]");
            println!("  winit   run nested inside another Wayland or X11 session");
            println!("  tty     run directly on DRM/KMS from a tty");
        }
        Some("winit") => {
            #[cfg(feature = "winit")]
            winit::run_winit();
            #[cfg(not(feature = "winit"))]
            println!("kyonde was built without the winit backend");
        }
        None => {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
                #[cfg(feature = "winit")]
                winit::run_winit();
                #[cfg(not(feature = "winit"))]
                println!("kyonde was built without the winit backend");
            } else {
                #[cfg(feature = "udev")]
                run_tty();
                #[cfg(not(feature = "udev"))]
                println!("kyonde was built without the udev backend");
            }
        }
        Some("tty") | Some("udev") | Some("session") => {
            #[cfg(feature = "udev")]
            run_tty();
            #[cfg(not(feature = "udev"))]
            println!("kyonde was built without the udev backend");
        }
        Some(other) => {
            eprintln!("unknown argument: {other}");
            std::process::exit(2);
        }
    }
}
