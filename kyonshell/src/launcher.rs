use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::theme;

pub fn build(app: &gtk::Application) {
    std::mem::forget(app.hold());
    let app_ref = app.clone();
    let prefs = crate::prefs::load_from_disk();
    let provider = gtk::CssProvider::new();
    theme::install(&provider, &prefs);

    let window = gtk::Window::builder()
        .application(app)
        .decorated(false)
        .build();
    window.add_css_class("shell-window");
    window.init_layer_shell();
    window.set_namespace(Some("kyonshell-launcher"));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    window.set_exclusive_zone(-1);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }

    let overlay = gtk::Overlay::new();
    let backdrop = gtk::Button::new();
    backdrop.add_css_class("launcher-backdrop");
    backdrop.add_css_class("flat");
    backdrop.set_hexpand(true);
    backdrop.set_vexpand(true);
    backdrop.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    overlay.set_child(Some(&backdrop));

    let card = gtk::Box::new(gtk::Orientation::Vertical, 12);
    card.add_css_class("launcher-card");
    card.set_halign(gtk::Align::Center);
    card.set_valign(gtk::Align::Center);
    card.set_width_request(760);
    card.set_height_request(650);

    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let logo = gtk::Label::new(Some("Kyon"));
    logo.add_css_class("about-logo");
    logo.set_markup("<span size='20pt' weight='800'>Kyon</span>");
    let hint = gtk::Label::new(Some("Applications"));
    hint.add_css_class("launcher-section");
    hint.set_xalign(0.0);
    hint.set_hexpand(true);
    let close = gtk::Button::with_label("Esc");
    close.add_css_class("shortcut-key");
    close.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    heading.append(&logo);
    heading.append(&hint);
    heading.append(&close);
    card.append(&heading);

    let search = gtk::SearchEntry::new();
    search.add_css_class("launcher-search");
    search.set_placeholder_text(Some("Search applications…"));
    search.set_hexpand(true);
    card.append(&search);

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));

    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_max_children_per_line(6);
    flow.set_min_children_per_line(4);
    flow.set_row_spacing(6);
    flow.set_column_spacing(6);
    flow.set_margin_top(4);
    flow.set_margin_bottom(4);

    let apps = gio::AppInfo::all();
    let entries: Rc<RefCell<Vec<(gtk::FlowBoxChild, String, String, gio::AppInfo)>>> =
        Rc::new(RefCell::new(Vec::new()));
    for app_info in apps {
        if !app_info.should_show() {
            continue;
        }
        let name = app_info.display_name().to_string();
        let desc = app_info.description().unwrap_or_default().to_string();
        let button = gtk::Button::new();
        button.add_css_class("app-button");
        button.add_css_class("flat");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.set_halign(gtk::Align::Center);
        let icon = gtk::Image::new();
        icon.set_pixel_size(38);
        if let Some(gicon) = app_info.icon() {
            icon.set_from_gicon(&gicon);
        } else {
            icon.set_icon_name(Some("application-x-executable-symbolic"));
        }
        let label = gtk::Label::new(Some(&name));
        label.add_css_class("app-name");
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(12);
        label.set_justify(gtk::Justification::Center);
        content.append(&icon);
        content.append(&label);
        button.set_child(Some(&content));
        button.set_tooltip_text(Some(&desc));
        let info = app_info.clone();
        let window_clone = window.clone();
        button.connect_clicked(move |_| {
            info.launch(&[], None::<&gio::AppLaunchContext>).ok();
            window_clone.close();
        });
        flow.insert(&button, -1);
        let child = flow
            .child_at_index((flow.observe_children().n_items() - 1) as i32)
            .and_downcast::<gtk::FlowBoxChild>()
            .unwrap();
        entries.borrow_mut().push((child, name.to_lowercase(), desc.to_lowercase(), app_info));
    }
    scroller.set_child(Some(&flow));
    card.append(&scroller);

    let power_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    power_row.set_halign(gtk::Align::Center);
    for (icon_name, label, command) in [
        ("system-lock-screen-symbolic", "Lock", "loginctl lock-session"),
        ("system-log-out-symbolic", "Log out", "loginctl terminate-session"),
        ("system-reboot-symbolic", "Reboot", "systemctl reboot"),
        ("system-shutdown-symbolic", "Shutdown", "systemctl poweroff"),
    ] {
        let button = gtk::Button::new();
        button.add_css_class("quick");
        button.add_css_class("flat");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let image = gtk::Image::from_icon_name(icon_name);
        image.set_pixel_size(15);
        let text = gtk::Label::new(Some(label));
        row.append(&image);
        row.append(&text);
        button.set_child(Some(&row));
        let command = command.to_string();
        button.connect_clicked(move |_| {
            let mut parts = command.split_whitespace();
            if let Some(program) = parts.next() {
                let _ = std::process::Command::new(program).args(parts).spawn();
            }
        });
        power_row.append(&button);
    }
    card.append(&power_row);

    overlay.add_overlay(&card);
    window.set_child(Some(&overlay));

    let entries_filter = entries.clone();
    search.connect_search_changed(move |entry| {
        let query = entry.text().to_lowercase();
        for (child, name, desc, _) in entries_filter.borrow().iter() {
            let visible = query.is_empty() || name.contains(&query) || desc.contains(&query);
            child.set_visible(visible);
        }
    });

    let entries_activate = entries.clone();
    let search_activate = search.clone();
    search.connect_activate(move |_| {
        let query = search_activate.text().to_lowercase();
        for (child, name, desc, info) in entries_activate.borrow().iter() {
            if child.is_visible() && (query.is_empty() || name.contains(&query) || desc.contains(&query)) {
                info.launch(&[], None::<&gio::AppLaunchContext>).ok();
                if let Some(window) = child.root().and_downcast::<gtk::Window>() {
                    window.close();
                }
                return;
            }
        }
    });

    let key = gtk::EventControllerKey::new();
    let window_key = window.clone();
    key.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            window_key.close();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(key);

    window.connect_destroy(move |_| app_ref.quit());
    window.present();
    search.grab_focus();
}
