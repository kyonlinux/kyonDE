use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use kyon_ipc::{KyonConfig, Snapshot};

use crate::{ipc, prefs::ShellPrefs, theme};

pub fn build(app: &gtk::Application) {
    let prefs = Rc::new(RefCell::new(crate::prefs::load_from_disk()));
    let provider = gtk::CssProvider::new();
    theme::install(&provider, &prefs.borrow());
    let Some(display) = gtk::gdk::Display::default() else {
        build_panel(app, None, prefs, provider);
        return;
    };
    let monitors = display.monitors();
    if monitors.n_items() == 0 {
        build_panel(app, None, prefs, provider);
        return;
    }
    for index in 0..monitors.n_items() {
        if let Some(monitor) = monitors.item(index).and_downcast::<gtk::gdk::Monitor>() {
            build_panel(app, Some(&monitor), prefs.clone(), provider.clone());
        }
    }
}

fn build_panel(
    app: &gtk::Application,
    monitor: Option<&gtk::gdk::Monitor>,
    prefs: Rc<RefCell<ShellPrefs>>,
    provider: gtk::CssProvider,
) {
    let window = gtk::Window::builder()
        .application(app)
        .decorated(false)
        .build();
    window.add_css_class("shell-window");
    window.init_layer_shell();
    if let Some(monitor) = monitor {
        window.set_monitor(Some(monitor));
    }
    window.set_namespace(Some("kyonshell-panel"));
    window.set_layer(Layer::Top);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);
    apply_geometry(&window, &prefs.borrow());

    let frame = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    frame.add_css_class("panel-frame");
    frame.set_height_request(prefs.borrow().panel_height - 4);
    frame.set_hexpand(true);
    frame.set_halign(gtk::Align::Fill);

    let center_box = gtk::CenterBox::new();
    center_box.set_hexpand(true);
    frame.append(&center_box);

    let start = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    start.set_valign(gtk::Align::Center);
    let launcher = gtk::Button::new();
    launcher.add_css_class("flat");
    let launcher_box = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    launcher_box.set_valign(gtk::Align::Center);
    let logo = gtk::Image::from_icon_name("view-app-grid-symbolic");
    logo.set_pixel_size(17);
    let brand = gtk::Label::new(Some("Kyon"));
    brand.add_css_class("panel-brand");
    launcher_box.append(&logo);
    launcher_box.append(&brand);
    launcher.set_child(Some(&launcher_box));
    launcher.set_tooltip_text(Some("Open applications"));
    launcher.connect_clicked(|_| ipc::spawn_self("launcher"));
    start.append(&launcher);

    let taskbar = gtk::Box::new(gtk::Orientation::Horizontal, 1);
    taskbar.add_css_class("taskbar");
    let all_apps = Rc::new(gio::AppInfo::all());
    for pin in prefs.borrow().pinned.clone() {
        taskbar.append(&pinned_button(&pin, &all_apps));
    }
    start.append(&taskbar);

    let title = gtk::Label::new(None);
    title.add_css_class("panel-title");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(40);
    title.set_xalign(0.0);
    start.append(&title);
    center_box.set_start_widget(Some(&start));

    let workspaces = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    workspaces.set_valign(gtk::Align::Center);
    workspaces.set_halign(gtk::Align::Center);
    let ws_buttons: Rc<RefCell<Vec<gtk::Button>>> = Rc::new(RefCell::new(Vec::new()));
    for i in 0..prefs.borrow().workspaces.clamp(1, 9) {
        let button = gtk::Button::with_label(&(i + 1).to_string());
        button.add_css_class("ws");
        button.set_tooltip_text(Some(&format!("Workspace {}", i + 1)));
        button.connect_clicked(move |_| {
            ipc::send("workspace", serde_json::json!(i));
        });
        workspaces.append(&button);
        ws_buttons.borrow_mut().push(button);
    }
    center_box.set_center_widget(Some(&workspaces));

    let end = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    end.set_valign(gtk::Align::Center);

    let layout_button = gtk::Button::with_label("Tiling");
    layout_button.add_css_class("mode-pill");
    layout_button.set_tooltip_text(Some("Cycle layout: tiling, scrolling, floating"));
    layout_button.connect_clicked(|_| {
        ipc::send("layout.cycle", serde_json::Value::Null);
    });
    end.append(&layout_button);

    let clock = gtk::Label::new(None);
    clock.add_css_class("clock");
    end.append(&clock);

    let settings = gtk::Button::new();
    settings.add_css_class("flat");
    let settings_icon = gtk::Image::from_icon_name("preferences-system-symbolic");
    settings_icon.set_pixel_size(16);
    settings.set_child(Some(&settings_icon));
    settings.set_tooltip_text(Some("Kyon Settings"));
    settings.connect_clicked(|_| ipc::spawn_self("settings"));
    end.append(&settings);

    let power = gtk::MenuButton::new();
    power.add_css_class("flat");
    let power_icon = gtk::Image::from_icon_name("system-shutdown-symbolic");
    power_icon.set_pixel_size(16);
    power.set_child(Some(&power_icon));
    power.set_tooltip_text(Some("Session"));
    let popover = gtk::Popover::new();
    popover.add_css_class("launcher-card");
    let power_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    for (label, icon_name, command) in [
        ("Lock", "system-lock-screen-symbolic", "lock"),
        ("Log out", "system-log-out-symbolic", "logout"),
        ("Reboot", "system-reboot-symbolic", "reboot"),
        ("Shut down", "system-shutdown-symbolic", "shutdown"),
    ] {
        let button = gtk::Button::new();
        button.add_css_class("flat");
        button.add_css_class("quick");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let icon = gtk::Image::from_icon_name(icon_name);
        icon.set_pixel_size(16);
        let text = gtk::Label::new(Some(label));
        text.set_xalign(0.0);
        row.append(&icon);
        row.append(&text);
        button.set_child(Some(&row));
        let cmd = command.to_string();
        button.connect_clicked(move |button| {
            if let Some(popover) = button.ancestor(gtk::Popover::static_type()).and_downcast::<gtk::Popover>() {
                popover.popdown();
            }
            session_action(&cmd);
        });
        power_box.append(&button);
    }
    popover.set_child(Some(&power_box));
    power.set_popover(Some(&popover));
    end.append(&power);
    center_box.set_end_widget(Some(&end));

    window.set_child(Some(&frame));
    window.present();

    update_clock(&clock, &prefs.borrow());
    let clock_tick = clock.clone();
    let prefs_clock = prefs.clone();
    glib::timeout_add_seconds_local(1, move || {
        update_clock(&clock_tick, &prefs_clock.borrow());
        glib::ControlFlow::Continue
    });
    let clock_ref = clock.clone();

    let title_ref = title.clone();
    let layout_ref = layout_button.clone();
    let ws_ref = ws_buttons.clone();
    let provider_ref = provider.clone();
    let window_ref = window.clone();
    let prefs_poll = prefs.clone();
    let mut last_mtime = crate::prefs::mtime();
    let mut last_state: Option<Snapshot> = None;
    glib::timeout_add_seconds_local(1, move || {
        if let Some(mtime) = crate::prefs::mtime() {
            if Some(mtime) != last_mtime {
                last_mtime = Some(mtime);
                let new_prefs = crate::prefs::load_from_disk();
                *prefs_poll.borrow_mut() = new_prefs.clone();
                theme::install(&provider_ref, &new_prefs);
                apply_geometry(&window_ref, &new_prefs);
                update_clock(&clock_ref, &new_prefs);
            }
        }
        if let Some(snapshot) = ipc::state() {
            apply_state(&snapshot, &title_ref, &layout_ref, &ws_ref);
            last_state = Some(snapshot);
        } else if let Some(snapshot) = &last_state {
            apply_state(snapshot, &title_ref, &layout_ref, &ws_ref);
        }
        glib::ControlFlow::Continue
    });
}

fn apply_geometry(window: &gtk::Window, prefs: &ShellPrefs) {
    let floating = prefs.panel_floating || matches!(prefs.theme.as_str(), "cosmic");
    let margin = if floating { 8 } else { 0 };
    let top = !prefs.panel_position.eq_ignore_ascii_case("bottom");
    window.set_anchor(Edge::Top, top);
    window.set_anchor(Edge::Bottom, !top);
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        window.set_margin(edge, 0);
    }
    if floating {
        window.set_margin(Edge::Left, margin);
        window.set_margin(Edge::Right, margin);
        window.set_margin(if top { Edge::Top } else { Edge::Bottom }, margin);
    }
    window.set_exclusive_zone(prefs.panel_height + margin * 2);
}

fn update_clock(label: &gtk::Label, prefs: &ShellPrefs) {
    let now = glib::DateTime::now_local().unwrap_or_else(|_| glib::DateTime::now_utc().unwrap());
    let fmt = if prefs.clock_seconds { "%H:%M:%S" } else { "%H:%M" };
    let text = now.format(fmt).map(|s| s.to_string()).unwrap_or_else(|_| "--:--".into());
    label.set_label(&text);
}

fn apply_state(
    snapshot: &Snapshot,
    title: &gtk::Label,
    layout: &gtk::Button,
    ws_buttons: &Rc<RefCell<Vec<gtk::Button>>>,
) {
    let focused = snapshot
        .focused_title
        .clone()
        .unwrap_or_else(|| format!("Workspace {}", snapshot.active_workspace + 1));
    title.set_label(&focused);
    let mode = match snapshot.layout_mode.as_str() {
        "scrolling" => "Scrolling",
        "floating" => "Floating",
        _ => "Tiling",
    };
    layout.set_label(mode);
    for (idx, button) in ws_buttons.borrow().iter().enumerate() {
        if idx == snapshot.active_workspace {
            button.add_css_class("ws-active");
        } else {
            button.remove_css_class("ws-active");
        }
    }
}

fn session_action(action: &str) {
    match action {
        "lock" => {
            ipc::run_command("loginctl", &["lock-session"]);
        }
        "logout" => {
            if std::process::Command::new("loginctl")
                .arg("terminate-session")
                .arg(std::env::var("XDG_SESSION_ID").unwrap_or_default())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
            {
                return;
            }
            ipc::run_command("hyprctl", &["dispatch", "exit"]);
        }
        "reboot" => {
            ipc::run_command("systemctl", &["reboot"]);
        }
        "shutdown" => {
            ipc::run_command("systemctl", &["poweroff"]);
        }
        _ => {}
    }
}

#[allow(dead_code)]
fn _config(_: KyonConfig) {}

fn pinned_button(pin: &str, all_apps: &[gio::AppInfo]) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("taskbar-button");
    button.add_css_class("flat");
    let icon = gtk::Image::new();
    icon.set_pixel_size(18);
    let mut label = pin.to_string();
    match pin {
        "@terminal" => {
            icon.set_icon_name(Some("utilities-terminal-symbolic"));
            label = "Terminal".into();
            button.connect_clicked(|_| launch_terminal());
        }
        "@settings" => {
            icon.set_icon_name(Some("preferences-system-symbolic"));
            label = "Kyon Settings".into();
            button.connect_clicked(|_| ipc::spawn_self("settings"));
        }
        "@launcher" => {
            icon.set_icon_name(Some("view-app-grid-symbolic"));
            label = "Applications".into();
            button.connect_clicked(|_| ipc::spawn_self("launcher"));
        }
        _ => {
            if let Some(app) = find_app(pin, all_apps) {
                label = app.display_name().to_string();
                if let Some(gicon) = app.icon() {
                    icon.set_from_gicon(&gicon);
                } else {
                    icon.set_icon_name(Some("application-x-executable-symbolic"));
                }
                button.connect_clicked(move |_| {
                    let _ = app.launch(&[], None::<&gio::AppLaunchContext>);
                });
            } else {
                icon.set_icon_name(Some("application-x-executable-symbolic"));
                let pin = pin.to_string();
                button.connect_clicked(move |_| {
                    let _ = std::process::Command::new(&pin).spawn();
                });
            }
        }
    }
    button.set_child(Some(&icon));
    button.set_tooltip_text(Some(&label));
    button
}

fn find_app(pin: &str, all_apps: &[gio::AppInfo]) -> Option<gio::AppInfo> {
    let needle = pin.to_ascii_lowercase();
    all_apps.iter().cloned().find(|app| {
        app.id()
            .map(|id| {
                let id = id.to_ascii_lowercase();
                id == needle || id == format!("{needle}.desktop")
            })
            .unwrap_or(false)
            || app.display_name().to_ascii_lowercase().contains(&needle)
    })
}

fn launch_terminal() {
    for candidate in ["foot", "kitty", "alacritty", "wezterm", "konsole", "gnome-terminal", "xterm"] {
        if std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {candidate}"))
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            let _ = std::process::Command::new(candidate).spawn();
            return;
        }
    }
}
