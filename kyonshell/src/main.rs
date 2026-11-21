#![allow(deprecated)]
#![allow(dead_code, unused_imports, unused_variables, unused_mut)]

mod ipc;
mod desktop;
mod launcher;
mod panel;
mod prefs;
mod session;
mod settings;
mod theme;

use gtk4 as gtk;
use gtk4::gio;
use gtk4::prelude::*;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "panel".into());
    let (app_id, run): (&str, fn(&gtk::Application)) = match mode.as_str() {
        "launcher" => ("io.github.kyon.kyonshell.launcher", launcher::build),
        "settings" => ("io.github.kyon.kyonshell.settings", settings::build),
        "session" => ("io.github.kyon.kyonshell.session", session::build),
        _ => ("io.github.kyon.kyonshell.panel", panel::build),
    };
    let app = gtk::Application::builder()
        .application_id(app_id)
        .flags(gio::ApplicationFlags::NON_UNIQUE | gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.connect_activate(move |app| run(app));
    app.connect_command_line(|app, _| {
        app.activate();
        gtk::glib::ExitCode::SUCCESS
    });
    app.run();
}
