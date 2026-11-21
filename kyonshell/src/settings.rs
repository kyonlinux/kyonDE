use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::gio;
use gtk4::prelude::*;

use kyon_ipc::{DisplayInfo, KyonConfig, OutputRule, THEMES};

use crate::{ipc, prefs::ShellPrefs, theme};

pub fn build(app: &gtk::Application) {
    let snapshot = ipc::state();
    let displays = snapshot
        .as_ref()
        .map(|s| s.displays.clone())
        .unwrap_or_default();
    let config = Rc::new(RefCell::new(
        snapshot
            .as_ref()
            .map(|s| s.config.clone())
            .unwrap_or_default(),
    ));
    let prefs = Rc::new(RefCell::new(crate::prefs::load_from_disk()));
    let provider = gtk::CssProvider::new();
    theme::install(&provider, &prefs.borrow());

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Kyon Settings")
        .default_width(1120)
        .default_height(760)
        .build();
    window.add_css_class("settings-window");

    let header = gtk::HeaderBar::new();
    let title_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    title_box.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(Some("Kyon Settings"));
    title.add_css_class("title");
    let subtitle = gtk::Label::new(Some("Desktop control center"));
    subtitle.add_css_class("panel-subtle");
    title_box.append(&title);
    title_box.append(&subtitle);
    header.set_title_widget(Some(&title_box));
    window.set_titlebar(Some(&header));

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.add_css_class("settings-body");
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 4);
    sidebar.add_css_class("sidebar");
    sidebar.set_width_request(218);
    let logo = gtk::Label::new(None);
    logo.set_markup("<span size='16pt' weight='800' color='#ffffff'>Kyon</span>");
    logo.add_css_class("sidebar-title");
    let logo_sub = gtk::Label::new(Some("A Smithay desktop"));
    logo_sub.add_css_class("sidebar-sub");
    sidebar.append(&logo);
    sidebar.append(&logo_sub);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(160);
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let nav_buttons: Rc<RefCell<Vec<(gtk::Button, String)>>> = Rc::new(RefCell::new(Vec::new()));

    let pages: [(&str, &str, &str); 6] = [
        ("appearance", "Appearance", "applications-graphics-symbolic"),
        ("desktop", "Desktop", "preferences-desktop-symbolic"),
        ("displays", "Displays", "video-display-symbolic"),
        ("wallpaper", "Wallpaper", "preferences-desktop-wallpaper-symbolic"),
        ("shortcuts", "Shortcuts", "preferences-desktop-keyboard-symbolic"),
        ("about", "About", "help-about-symbolic"),
    ];

    for (id, label, icon_name) in pages {
        let button = gtk::Button::new();
        button.add_css_class("nav-button");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let icon = gtk::Image::from_icon_name(icon_name);
        icon.set_pixel_size(16);
        let text = gtk::Label::new(Some(label));
        text.set_xalign(0.0);
        text.set_hexpand(true);
        row.append(&icon);
        row.append(&text);
        button.set_child(Some(&row));
        sidebar.append(&button);
        nav_buttons.borrow_mut().push((button.clone(), id.to_string()));
        let stack_clone = stack.clone();
        let id_clone = id.to_string();
        let navs = nav_buttons.clone();
        button.connect_clicked(move |_| {
            stack_clone.set_visible_child_name(&id_clone);
            for (nav, nav_id) in navs.borrow().iter() {
                if nav_id == &id_clone {
                    nav.add_css_class("active");
                } else {
                    nav.remove_css_class("active");
                }
            }
        });
    }

    let appearance = make_appearance_page(&config, &prefs, &provider);
    let desktop = make_desktop_page(&config);
    let displays_page = make_displays_page(&config, &displays);
    let wallpaper = make_wallpaper_page(&config, &window);
    let shortcuts = make_shortcuts_page(&config);
    let about = make_about_page();
    stack.add_named(&appearance, Some("appearance"));
    stack.add_named(&desktop, Some("desktop"));
    stack.add_named(&displays_page, Some("displays"));
    stack.add_named(&wallpaper, Some("wallpaper"));
    stack.add_named(&shortcuts, Some("shortcuts"));
    stack.add_named(&about, Some("about"));

    if let Some((first, _)) = nav_buttons.borrow().first() {
        first.add_css_class("active");
    }
    if let Some(start) = std::env::args().nth(2) {
        if let Some((button, _)) = nav_buttons
            .borrow()
            .iter()
            .find(|(_, id)| *id == start)
        {
            button.emit_clicked();
        }
    }

    body.append(&sidebar);
    body.append(&stack);
    window.set_child(Some(&body));
    window.present();
}

fn page(title: &str, description: &str) -> (gtk::ScrolledWindow, gtk::Box) {
    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("page-scroll");
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.add_css_class("page");
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("section-title");
    heading.set_xalign(0.0);
    let desc = gtk::Label::new(Some(description));
    desc.add_css_class("section-desc");
    desc.set_xalign(0.0);
    desc.set_wrap(true);
    content.append(&heading);
    content.append(&desc);
    scroller.set_child(Some(&content));
    (scroller, content)
}

fn row(title: &str, description: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("row");
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    let title = gtk::Label::new(Some(title));
    title.add_css_class("row-title");
    title.set_xalign(0.0);
    let desc = gtk::Label::new(Some(description));
    desc.add_css_class("row-desc");
    desc.set_xalign(0.0);
    desc.set_wrap(true);
    labels.append(&title);
    labels.append(&desc);
    row.append(&labels);
    control.as_ref().set_valign(gtk::Align::Center);
    row.append(control);
    row
}

fn switch(active: bool) -> gtk::Switch {
    let switch = gtk::Switch::new();
    switch.set_active(active);
    switch
}

fn scale(value: f64, min: f64, max: f64, step: f64) -> gtk::Scale {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
    scale.set_value(value);
    scale.set_size_request(220, -1);
    scale.set_draw_value(true);
    scale
}

fn combo(options: &[(&str, &str)], active: &str) -> gtk::ComboBoxText {
    let combo = gtk::ComboBoxText::new();
    for (id, label) in options {
        combo.append(Some(id), label);
    }
    combo.set_active_id(Some(active));
    combo
}

fn send(config: &Rc<RefCell<KyonConfig>>, patch: serde_json::Value) {
    let _ = config;
    ipc::apply(patch);
}

fn apply_shell(prefs: &Rc<RefCell<ShellPrefs>>, provider: &gtk::CssProvider) {
    let p = prefs.borrow().clone();
    crate::prefs::save(&p);
    crate::prefs::apply_to_compositor(&p);
    theme::install(provider, &p);
}

fn make_appearance_page(
    config: &Rc<RefCell<KyonConfig>>,
    prefs: &Rc<RefCell<ShellPrefs>>,
    provider: &gtk::CssProvider,
) -> gtk::ScrolledWindow {
    let (scroller, content) = page(
        "Appearance",
        "Pick a personality for the panel, launcher and title bars. Themes apply instantly.",
    );

    let cards_box = gtk::FlowBox::new();
    cards_box.set_selection_mode(gtk::SelectionMode::None);
    cards_box.set_max_children_per_line(3);
    cards_box.set_min_children_per_line(2);
    cards_box.set_row_spacing(10);
    cards_box.set_column_spacing(10);
    let cards: Rc<RefCell<Vec<(String, gtk::Button)>>> = Rc::new(RefCell::new(Vec::new()));

    for preset in THEMES {
        let palette = preset.palette(prefs.borrow().dark);
        let card = gtk::Button::new();
        card.add_css_class("theme-card");
        let card_box = gtk::Box::new(gtk::Orientation::Vertical, 7);
        let swatches = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        swatches.add_css_class("swatch-row");
        for color in [palette.bg, palette.surface, palette.accent, palette.text] {
            let swatch = gtk::Box::new(gtk::Orientation::Vertical, 0);
            swatch.set_hexpand(true);
            swatch.set_size_request(-1, 30);
            let css = gtk::CssProvider::new();
            css.load_from_data(&format!(
                "* {{ background-color: {}; border-radius: 4px; }}",
                color
            ));
            swatch
                .style_context()
                .add_provider(&css, gtk::STYLE_PROVIDER_PRIORITY_USER);
            swatches.append(&swatch);
        }
        let name = gtk::Label::new(Some(preset.name));
        name.add_css_class("theme-name");
        name.set_xalign(0.0);
        let desc = gtk::Label::new(Some(preset.description));
        desc.add_css_class("theme-desc");
        desc.set_xalign(0.0);
        desc.set_wrap(true);
        desc.set_max_width_chars(24);
        card_box.append(&swatches);
        card_box.append(&name);
        card_box.append(&desc);
        card.set_child(Some(&card_box));
        let id = preset.id.to_string();
        let accent = preset.dark.accent.to_string();
        let prefs_c = prefs.clone();
        let provider_c = provider.clone();
        let cards_c = cards.clone();
        let id_c = id.clone();
        card.connect_clicked(move |_| {
            {
                let mut p = prefs_c.borrow_mut();
                p.theme = id_c.clone();
                p.accent = accent.clone();
            }
            apply_shell(&prefs_c, &provider_c);
            for (card_id, button) in cards_c.borrow().iter() {
                if card_id == &id_c {
                    button.add_css_class("selected");
                } else {
                    button.remove_css_class("selected");
                }
            }
        });
        if prefs.borrow().theme == id {
            card.add_css_class("selected");
        }
        cards.borrow_mut().push((id, card.clone()));
        cards_box.insert(&card, -1);
    }
    content.append(&cards_box);

    let dark = switch(prefs.borrow().dark);
    {
        let prefs = prefs.clone();
        let provider = provider.clone();
        dark.connect_state_set(move |_, state| {
            prefs.borrow_mut().dark = state;
            apply_shell(&prefs, &provider);
            gtk::glib::Propagation::Proceed
        });
    }
    content.append(&row("Dark mode", "Use dark surfaces and a dimmed panel.", &dark));

    let accent_entry = gtk::Entry::new();
    accent_entry.set_text(&prefs.borrow().accent_color());
    accent_entry.set_width_chars(10);
    let accent_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    accent_row.append(&accent_entry);
    let accent_apply = gtk::Button::with_label("Apply");
    accent_apply.add_css_class("suggested");
    accent_row.append(&accent_apply);
    {
        let prefs = prefs.clone();
        let provider = provider.clone();
        let entry = accent_entry.clone();
        accent_apply.connect_clicked(move |_| {
            let text = entry.text().to_string();
            if text.starts_with('#') && (text.len() == 7 || text.len() == 9) {
                prefs.borrow_mut().accent = text;
                apply_shell(&prefs, &provider);
            }
        });
    }
    content.append(&row("Accent colour", "Hex colour used across the shell and title bars.", &accent_row));

    let position = combo(
        &[("top", "Top"), ("bottom", "Bottom")],
        &prefs.borrow().panel_position,
    );
    {
        let prefs = prefs.clone();
        let provider = provider.clone();
        position.connect_changed(move |combo| {
            if let Some(id) = combo.active_id() {
                prefs.borrow_mut().panel_position = id.to_string();
                apply_shell(&prefs, &provider);
            }
        });
    }
    content.append(&row("Panel position", "Dock the bar at the top or bottom edge.", &position));

    let height = scale(prefs.borrow().panel_height as f64, 26.0, 72.0, 1.0);
    {
        let prefs = prefs.clone();
        let provider = provider.clone();
        height.connect_value_changed(move |scale| {
            prefs.borrow_mut().panel_height = scale.value() as i32;
            apply_shell(&prefs, &provider);
        });
    }
    content.append(&row("Panel height", "Base height of the shell panel.", &height));

    let floating = switch(prefs.borrow().panel_floating);
    {
        let prefs = prefs.clone();
        let provider = provider.clone();
        floating.connect_state_set(move |_, state| {
            prefs.borrow_mut().panel_floating = state;
            apply_shell(&prefs, &provider);
            gtk::glib::Propagation::Proceed
        });
    }
    content.append(&row("Floating panel", "Detach the panel from the screen edge with a margin.", &floating));

    let opacity = scale((prefs.borrow().panel_opacity * 100.0) as f64, 40.0, 100.0, 1.0);
    {
        let prefs = prefs.clone();
        let provider = provider.clone();
        opacity.connect_value_changed(move |scale| {
            prefs.borrow_mut().panel_opacity = (scale.value() / 100.0) as f32;
            apply_shell(&prefs, &provider);
        });
    }
    content.append(&row("Panel opacity", "Blend the panel with the wallpaper behind it.", &opacity));

    let config_appearance = config.clone();
    let anim = switch(config.borrow().animations.enabled);
    {
        let config = config_appearance.clone();
        anim.connect_state_set(move |_, state| {
            config.borrow_mut().animations.enabled = state;
            send(&config, serde_json::json!({ "animations": config.borrow().animations }));
            gtk::glib::Propagation::Proceed
        });
    }
    content.append(&row("Window animations", "Enable animated window entrance and layout changes.", &anim));

    let blur = switch(config.borrow().blur.enabled);
    {
        let config = config_appearance;
        blur.connect_state_set(move |_, state| {
            config.borrow_mut().blur.enabled = state;
            send(&config, serde_json::json!({ "blur": config.borrow().blur }));
            gtk::glib::Propagation::Proceed
        });
    }
    content.append(&row("Blur", "Soften the wallpaper behind translucent surfaces.", &blur));

    scroller
}

fn make_desktop_page(config: &Rc<RefCell<KyonConfig>>) -> gtk::ScrolledWindow {
    let (scroller, content) = page(
        "Desktop",
        "Tune the layout engine, spacing and decorations of the compositor.",
    );

    let mode = combo(
        &[
            ("tiling", "Tiling"),
            ("scrolling", "Scrolling"),
            ("floating", "Floating"),
        ],
        &config.borrow().layout.mode,
    );
    {
        let config = config.clone();
        mode.connect_changed(move |combo| {
            if let Some(id) = combo.active_id() {
                config.borrow_mut().layout.mode = id.to_string();
                let mode = config.borrow().layout.mode.clone();
                send(&config, serde_json::json!({ "layout": config.borrow().layout }));
                ipc::send("layout", serde_json::json!(mode));
            }
        });
    }
    content.append(&row("Layout mode", "Windows can tile, scroll in columns, or float freely.", &mode));

    let algo = combo(
        &[("master", "Master and stack"), ("dwindle", "Dwindle")],
        &config.borrow().layout.tiling,
    );
    {
        let config = config.clone();
        algo.connect_changed(move |combo| {
            if let Some(id) = combo.active_id() {
                config.borrow_mut().layout.tiling = id.to_string();
                send(&config, serde_json::json!({ "layout": config.borrow().layout }));
            }
        });
    }
    content.append(&row("Tiling algorithm", "Master keeps one large window; dwindle splits recursively.", &algo));

    let ratio = scale(
        config.borrow().layout.master_ratio as f64 * 100.0,
        25.0,
        80.0,
        1.0,
    );
    {
        let config = config.clone();
        ratio.connect_value_changed(move |scale| {
            config.borrow_mut().layout.master_ratio = (scale.value() / 100.0) as f32;
            send(&config, serde_json::json!({ "layout": config.borrow().layout }));
        });
    }
    content.append(&row("Master ratio", "Percentage of the workspace used by the master window.", &ratio));

    let gaps_in = scale(config.borrow().decor.gaps_in as f64, 0.0, 32.0, 1.0);
    {
        let config = config.clone();
        gaps_in.connect_value_changed(move |scale| {
            config.borrow_mut().decor.gaps_in = scale.value() as i32;
            send(&config, serde_json::json!({ "decor": config.borrow().decor }));
        });
    }
    content.append(&row("Inner gaps", "Space between adjacent tiled windows.", &gaps_in));

    let gaps_out = scale(config.borrow().decor.gaps_out as f64, 0.0, 64.0, 1.0);
    {
        let config = config.clone();
        gaps_out.connect_value_changed(move |scale| {
            config.borrow_mut().decor.gaps_out = scale.value() as i32;
            send(&config, serde_json::json!({ "decor": config.borrow().decor }));
        });
    }
    content.append(&row("Outer gaps", "Padding between windows and the screen edge.", &gaps_out));

    let rounding = scale(config.borrow().decor.rounding as f64, 0.0, 24.0, 1.0);
    {
        let config = config.clone();
        rounding.connect_value_changed(move |scale| {
            config.borrow_mut().decor.rounding = scale.value() as i32;
            send(&config, serde_json::json!({ "decor": config.borrow().decor }));
        });
    }
    content.append(&row("Corner rounding", "Radius for decorated surfaces and title bars.", &rounding));

    let border = scale(config.borrow().decor.border_width as f64, 0.0, 4.0, 1.0);
    {
        let config = config.clone();
        border.connect_value_changed(move |scale| {
            config.borrow_mut().decor.border_width = scale.value() as i32;
            send(&config, serde_json::json!({ "decor": config.borrow().decor }));
        });
    }
    content.append(&row("Border width", "Outline around windows. Set to zero for an edge to edge look.", &border));

    let speed = scale(config.borrow().animations.speed as f64, 1.0, 10.0, 1.0);
    {
        let config = config.clone();
        speed.connect_value_changed(move |scale| {
            config.borrow_mut().animations.speed = scale.value() as f32;
            send(&config, serde_json::json!({ "animations": config.borrow().animations }));
        });
    }
    content.append(&row("Animation speed", "Higher values make transitions faster.", &speed));

    let scrolling = scale(
        config.borrow().layout.scrolling.column_width as f64 * 100.0,
        25.0,
        100.0,
        1.0,
    );
    {
        let config = config.clone();
        scrolling.connect_value_changed(move |scale| {
            config.borrow_mut().layout.scrolling.column_width = (scale.value() / 100.0) as f32;
            send(&config, serde_json::json!({ "layout": config.borrow().layout }));
        });
    }
    content.append(&row("Scrolling column width", "Width of each column in scrolling mode.", &scrolling));

    let blur_size = scale(config.borrow().blur.size as f64, 0.0, 30.0, 1.0);
    {
        let config = config.clone();
        blur_size.connect_value_changed(move |scale| {
            config.borrow_mut().blur.size = scale.value() as f32;
            send(&config, serde_json::json!({ "blur": config.borrow().blur }));
        });
    }
    content.append(&row("Blur radius", "How strongly the wallpaper is softened.", &blur_size));

    scroller
}


fn default_rule(display: &DisplayInfo) -> OutputRule {
    OutputRule {
        name: display.name.clone(),
        scale: display.scale,
        mode: if display.width > 0 && display.height > 0 {
            format!("{}x{}@{}", display.width, display.height, display.refresh)
        } else {
            String::new()
        },
        position_x: display.x,
        position_y: display.y,
        transform: String::new(),
        enabled: display.enabled,
        primary: display.primary,
    }
}

fn set_rule(config: &Rc<RefCell<KyonConfig>>, display: &DisplayInfo, update: impl FnOnce(&mut OutputRule)) {
    let mut config = config.borrow_mut();
    let index = config
        .outputs
        .iter()
        .position(|rule| rule.name == display.name);
    let index = match index {
        Some(index) => index,
        None => {
            config.outputs.push(default_rule(display));
            config.outputs.len() - 1
        }
    };
    update(&mut config.outputs[index]);
}

fn send_outputs(config: &Rc<RefCell<KyonConfig>>) {
    let outputs = config.borrow().outputs.clone();
    let _ = ipc::apply(serde_json::json!({ "outputs": outputs }));
}

fn display_size(display: &DisplayInfo) -> (f64, f64) {
    let scale = display.scale.max(0.1);
    (
        display.width as f64 / scale,
        display.height as f64 / scale,
    )
}

fn canvas_view(displays: &[DisplayInfo], width: i32, height: i32) -> (f64, f64, f64, f64, f64) {
    if displays.is_empty() {
        return (1.0, 0.0, 0.0, 0.0, 0.0);
    }
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;
    for display in displays {
        let (w, h) = display_size(display);
        min_x = min_x.min(display.x as f64);
        min_y = min_y.min(display.y as f64);
        max_x = max_x.max(display.x as f64 + w);
        max_y = max_y.max(display.y as f64 + h);
    }
    let total_w = (max_x - min_x).max(1.0);
    let total_h = (max_y - min_y).max(1.0);
    let padding = 28.0;
    let scale = ((width as f64 - padding * 2.0) / total_w)
        .min((height as f64 - padding * 2.0) / total_h)
        .min(0.6)
        .max(0.02);
    let offset_x = (width as f64 - total_w * scale) / 2.0 - min_x * scale;
    let offset_y = (height as f64 - total_h * scale) / 2.0 - min_y * scale;
    (scale, offset_x, offset_y, min_x, min_y)
}

fn display_canvas(
    config: &Rc<RefCell<KyonConfig>>,
    displays: &Rc<RefCell<Vec<DisplayInfo>>>,
    selected: &Rc<Cell<usize>>,
    spinners: &Rc<RefCell<Vec<(gtk::SpinButton, gtk::SpinButton)>>>,
    updating: &Rc<Cell<bool>>,
) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(560);
    area.set_content_height(300);
    area.set_hexpand(true);
    area.add_css_class("display-canvas");
    {
        let displays = displays.clone();
        let selected = selected.clone();
        area.set_draw_func(move |area, cr, width, height| {
            let displays = displays.borrow();
            cr.set_source_rgb(0.04, 0.05, 0.08);
            cr.paint().ok();
            let (scale, offset_x, offset_y, _, _) = canvas_view(&displays, width, height);
            for (index, display) in displays.iter().enumerate() {
                let (w, h) = display_size(display);
                let x = offset_x + display.x as f64 * scale;
                let y = offset_y + display.y as f64 * scale;
                let w = (w * scale).max(42.0);
                let h = (h * scale).max(30.0);
                let is_selected = index == selected.get();
                if display.enabled {
                    cr.set_source_rgba(0.14, 0.17, 0.22, 1.0);
                } else {
                    cr.set_source_rgba(0.09, 0.1, 0.13, 1.0);
                }
                cr.rectangle(x, y, w, h);
                cr.fill().ok();
                if is_selected {
                    cr.set_source_rgba(0.24, 0.68, 0.91, 1.0);
                } else {
                    cr.set_source_rgba(0.35, 0.39, 0.47, 1.0);
                }
                cr.set_line_width(if is_selected { 3.0 } else { 1.5 });
                cr.rectangle(x, y, w, h);
                cr.stroke().ok();
                if display.primary {
                    cr.set_source_rgba(0.24, 0.68, 0.91, 1.0);
                    cr.rectangle(x + 8.0, y + 8.0, 7.0, 7.0);
                    cr.fill().ok();
                }
                cr.set_source_rgba(0.92, 0.94, 0.98, 1.0);
                cr.set_font_size(13.0);
                cr.move_to(x + 10.0, y + 22.0);
                cr.show_text(&display.name).ok();
                cr.set_source_rgba(0.65, 0.7, 0.8, 1.0);
                cr.set_font_size(10.5);
                let text = format!(
                    "{}x{}  {} Hz",
                    display.width, display.height, display.refresh
                );
                cr.move_to(x + 10.0, y + h - 10.0);
                cr.show_text(&text).ok();
            }
        });
    }
    let click = gtk::GestureClick::new();
    click.set_button(0);
    {
        let displays = displays.clone();
        let selected = selected.clone();
        let area = area.clone();
        click.connect_pressed(move |gesture, _, x, y| {
            let displays = displays.borrow();
            let (scale, offset_x, offset_y, _, _) =
                canvas_view(&displays, area.width(), area.height());
            let world_x = (x - offset_x) / scale;
            let world_y = (y - offset_y) / scale;
            for (index, display) in displays.iter().enumerate() {
                let (w, h) = display_size(display);
                if world_x >= display.x as f64
                    && world_x <= display.x as f64 + w
                    && world_y >= display.y as f64
                    && world_y <= display.y as f64 + h
                {
                    selected.set(index);
                    area.queue_draw();
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    return;
                }
            }
        });
    }
    let drag = gtk::GestureDrag::new();
    let drag_scale = Rc::new(Cell::new(1.0));
    let drag_start_x = Rc::new(Cell::new(0.0));
    let drag_start_y = Rc::new(Cell::new(0.0));
    {
        let displays = displays.clone();
        let selected = selected.clone();
        let area = area.clone();
        let drag_scale = drag_scale.clone();
        let drag_start_x = drag_start_x.clone();
        let drag_start_y = drag_start_y.clone();
        drag.connect_drag_begin(move |gesture, x, y| {
            let displays_ref = displays.borrow();
            let (scale, offset_x, offset_y, _, _) =
                canvas_view(&displays_ref, area.width(), area.height());
            let world_x = (x - offset_x) / scale;
            let world_y = (y - offset_y) / scale;
            for (index, display) in displays_ref.iter().enumerate() {
                let (w, h) = display_size(display);
                if world_x >= display.x as f64
                    && world_x <= display.x as f64 + w
                    && world_y >= display.y as f64
                    && world_y <= display.y as f64 + h
                {
                    selected.set(index);
                    drag_start_x.set(display.x as f64);
                    drag_start_y.set(display.y as f64);
                    drag_scale.set(scale);
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    area.queue_draw();
                    return;
                }
            }
            selected.set(usize::MAX);
            area.queue_draw();
        });
    }
    {
        let displays = displays.clone();
        let selected = selected.clone();
        let area = area.clone();
        let config = config.clone();
        let spinners = spinners.clone();
        let updating = updating.clone();
        let drag_scale = drag_scale.clone();
        let drag_start_x = drag_start_x.clone();
        let drag_start_y = drag_start_y.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            let index = selected.get();
            let mut displays = displays.borrow_mut();
            let Some(display) = displays.get_mut(index) else {
                return;
            };
            let scale = drag_scale.get().max(0.01);
            display.x = (drag_start_x.get() + dx / scale).round() as i32;
            display.y = (drag_start_y.get() + dy / scale).round() as i32;
            let rule_display = display.clone();
            set_rule(&config, &rule_display, |rule| {
                rule.position_x = rule_display.x;
                rule.position_y = rule_display.y;
            });
            if let Some((spin_x, spin_y)) = spinners.borrow().get(index) {
                updating.set(true);
                spin_x.set_value(rule_display.x as f64);
                spin_y.set_value(rule_display.y as f64);
                updating.set(false);
            }
            area.queue_draw();
        });
    }
    {
        let config = config.clone();
        let area = area.clone();
        drag.connect_drag_end(move |_, _, _| {
            send_outputs(&config);
            area.queue_draw();
        });
    }
    area.add_controller(drag);
    area.add_controller(click);
    area
}

fn display_card(
    config: &Rc<RefCell<KyonConfig>>,
    displays: &Rc<RefCell<Vec<DisplayInfo>>>,
    area: &gtk::DrawingArea,
    index: usize,
    spinners: &Rc<RefCell<Vec<(gtk::SpinButton, gtk::SpinButton)>>>,
    updating: &Rc<Cell<bool>>,
) -> gtk::Box {
    let display = displays.borrow()[index].clone();
    let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
    card.add_css_class("display-card");
    let header = gtk::Label::new(None);
    header.set_markup(&format!(
        "<span weight='700' size='12pt'>{}</span>",
        glib_escape(&display.name)
    ));
    header.set_xalign(0.0);
    card.append(&header);
    let subtitle = gtk::Label::new(Some(&format!(
        "{}x{} at {} Hz, scale {}",
        display.width, display.height, display.refresh, display.scale
    )));
    subtitle.add_css_class("row-desc");
    subtitle.set_xalign(0.0);
    card.append(&subtitle);

    let toggles = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    let enabled = switch(display.enabled);
    {
        let config = config.clone();
        let displays = displays.clone();
        let area = area.clone();
        let display_name = display.name.clone();
        enabled.connect_state_set(move |_, state| {
            if let Some(item) = displays.borrow_mut().iter_mut().find(|item| item.name == display_name) {
                item.enabled = state;
            }
            let display = displays
                .borrow()
                .iter()
                .find(|item| item.name == display_name)
                .cloned()
                .unwrap_or_default();
            set_rule(&config, &display, |rule| rule.enabled = state);
            send_outputs(&config);
            area.queue_draw();
            gtk::glib::Propagation::Proceed
        });
    }
    let primary = switch(display.primary);
    {
        let config = config.clone();
        let displays = displays.clone();
        let area = area.clone();
        let display_name = display.name.clone();
        primary.connect_state_set(move |_, state| {
            {
                let mut items = displays.borrow_mut();
                if state {
                    for item in items.iter_mut() {
                        item.primary = item.name == display_name;
                    }
                } else if let Some(item) = items.iter_mut().find(|item| item.name == display_name) {
                    item.primary = false;
                }
            }
            let mut config_mut = config.borrow_mut();
            if state {
                for rule in &mut config_mut.outputs {
                    rule.primary = rule.name == display_name;
                }
            } else if let Some(rule) = config_mut.outputs.iter_mut().find(|rule| rule.name == display_name) {
                rule.primary = false;
            }
            drop(config_mut);
            send_outputs(&config);
            area.queue_draw();
            gtk::glib::Propagation::Proceed
        });
    }
    toggles.append(&row("Enabled", "Render the desktop on this output.", &enabled));
    toggles.append(&row("Primary", "Place new windows and the main layout here.", &primary));
    card.append(&toggles);

    let modes = gtk::ComboBoxText::new();
    let mut seen = Vec::new();
    for mode in &display.modes {
        let id = format!("{}x{}@{}", mode.width, mode.height, mode.refresh);
        if seen.contains(&id) {
            continue;
        }
        seen.push(id.clone());
        modes.append(
            Some(&id),
            &format!("{} x {} @ {} Hz", mode.width, mode.height, mode.refresh),
        );
    }
    let active_mode = format!("{}x{}@{}", display.width, display.height, display.refresh);
    if seen.is_empty() {
        modes.append(Some(&active_mode), &format!("{} x {} @ {} Hz", display.width, display.height, display.refresh));
    }
    if seen.contains(&active_mode) {
        modes.set_active_id(Some(&active_mode));
    } else if !seen.is_empty() {
        modes.set_active(Some(0));
    }
    {
        let config = config.clone();
        let displays = displays.clone();
        let area = area.clone();
        let display_name = display.name.clone();
        modes.connect_changed(move |combo| {
            let Some(id) = combo.active_id() else {
                return;
            };
            let mut parts = id.split('@');
            let size = parts.next().unwrap_or_default();
            let refresh = parts.next().and_then(|value| value.parse::<i32>().ok()).unwrap_or(60);
            let mut size_parts = size.split('x');
            let width = size_parts.next().and_then(|value| value.parse::<i32>().ok()).unwrap_or(0);
            let height = size_parts.next().and_then(|value| value.parse::<i32>().ok()).unwrap_or(0);
            if width <= 0 || height <= 0 {
                return;
            }
            {
                let mut items = displays.borrow_mut();
                if let Some(item) = items.get_mut(index) {
                    item.width = width;
                    item.height = height;
                    item.refresh = refresh;
                }
            }
            let display = displays.borrow()[index].clone();
            set_rule(&config, &display, |rule| rule.mode = id.to_string());
            send_outputs(&config);
            area.queue_draw();
        });
    }
    card.append(&row(
        "Resolution and refresh",
        "Choose a mode supported by this connector.",
        &modes,
    ));

    let scale_combo = gtk::ComboBoxText::new();
    for value in ["0.75", "1", "1.25", "1.5", "1.75", "2", "2.5", "3"] {
        scale_combo.append(Some(value), &format!("{value}x"));
    }
    let scale_text = format!("{}", display.scale);
    let scale_text = if scale_text == "1.0" || scale_text == "1" { "1" } else { &scale_text };
    scale_combo.set_active_id(Some(scale_text));
    {
        let config = config.clone();
        let displays = displays.clone();
        let area = area.clone();
        let display_name = display.name.clone();
        scale_combo.connect_changed(move |combo| {
            let Some(id) = combo.active_id() else {
                return;
            };
            let Ok(value) = id.parse::<f64>() else {
                return;
            };
            {
                let mut items = displays.borrow_mut();
                if let Some(item) = items.iter_mut().find(|item| item.name == display_name) {
                    item.scale = value;
                }
            }
            let display = displays
                .borrow()
                .iter()
                .find(|item| item.name == display_name)
                .cloned()
                .unwrap_or_default();
            set_rule(&config, &display, |rule| rule.scale = value);
            send_outputs(&config);
            area.queue_draw();
        });
    }
    card.append(&row("Scale", "Logical size and text scaling for this output.", &scale_combo));

    let position = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let spin_x = gtk::SpinButton::with_range(-20000.0, 20000.0, 10.0);
    spin_x.set_value(display.x as f64);
    spin_x.set_width_chars(6);
    let spin_y = gtk::SpinButton::with_range(-20000.0, 20000.0, 10.0);
    spin_y.set_value(display.y as f64);
    spin_y.set_width_chars(6);
    position.append(&spin_x);
    position.append(&spin_y);
    spinners.borrow_mut().push((spin_x.clone(), spin_y.clone()));
    {
        let config = config.clone();
        let displays = displays.clone();
        let area = area.clone();
        let display_name = display.name.clone();
        let updating_guard = updating.clone();
        spin_x.connect_value_changed(move |spin| {
            if updating_guard.get() {
                return;
            }
            let value = spin.value() as i32;
            if let Some(item) = displays.borrow_mut().iter_mut().find(|item| item.name == display_name) {
                item.x = value;
            }
            let display = displays
                .borrow()
                .iter()
                .find(|item| item.name == display_name)
                .cloned()
                .unwrap_or_default();
            set_rule(&config, &display, |rule| rule.position_x = value);
            send_outputs(&config);
            area.queue_draw();
        });
    }
    {
        let config = config.clone();
        let displays = displays.clone();
        let area = area.clone();
        let display_name = display.name.clone();
        let updating_guard = updating.clone();
        spin_y.connect_value_changed(move |spin| {
            if updating_guard.get() {
                return;
            }
            let value = spin.value() as i32;
            if let Some(item) = displays.borrow_mut().iter_mut().find(|item| item.name == display_name) {
                item.y = value;
            }
            let display = displays
                .borrow()
                .iter()
                .find(|item| item.name == display_name)
                .cloned()
                .unwrap_or_default();
            set_rule(&config, &display, |rule| rule.position_y = value);
            send_outputs(&config);
            area.queue_draw();
        });
    }
    card.append(&row(
        "Position",
        "X and Y logical coordinates. You can also drag monitors above.",
        &position,
    ));
    card
}

fn glib_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn make_displays_page(
    config: &Rc<RefCell<KyonConfig>>,
    displays: &[DisplayInfo],
) -> gtk::ScrolledWindow {
    let (scroller, content) = page(
        "Displays",
        "Arrange monitors, choose modes and refresh rates, and set the primary output.",
    );
    if displays.is_empty() {
        let label = gtk::Label::new(Some(
            "No displays were reported by the compositor. Start KyonDE and reopen settings.",
        ));
        label.add_css_class("section-desc");
        label.set_xalign(0.0);
        content.append(&label);
        return scroller;
    }
    let displays = Rc::new(RefCell::new(displays.to_vec()));
    for display in displays.borrow().iter() {
        set_rule(config, display, |_| {});
    }
    let selected = Rc::new(Cell::new(0));
    let spinners: Rc<RefCell<Vec<(gtk::SpinButton, gtk::SpinButton)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let updating = Rc::new(Cell::new(false));
    let area = display_canvas(config, &displays, &selected, &spinners, &updating);
    content.append(&area);
    let cards = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let count = displays.borrow().len();
    for index in 0..count {
        cards.append(&display_card(
            config, &displays, &area, index, &spinners, &updating,
        ));
    }
    content.append(&cards);
    scroller
}

fn make_wallpaper_page(config: &Rc<RefCell<KyonConfig>>, window: &gtk::ApplicationWindow) -> gtk::ScrolledWindow {
    let (scroller, content) = page(
        "Wallpaper",
        "Choose a procedural gradient, a solid colour, or an image from disk.",
    );
    let style = combo(
        &[("gradient", "Gradient"), ("solid", "Solid colour"), ("image", "Image")],
        &config.borrow().wallpaper.style,
    );
    {
        let config = config.clone();
        style.connect_changed(move |combo| {
            if let Some(id) = combo.active_id() {
                config.borrow_mut().wallpaper.style = id.to_string();
                send(&config, serde_json::json!({ "wallpaper": config.borrow().wallpaper }));
            }
        });
    }
    content.append(&row("Wallpaper style", "Gradients are generated on the GPU and always fit the output.", &style));

    let color = gtk::Entry::new();
    color.set_text(&config.borrow().wallpaper.color);
    {
        let config = config.clone();
        color.connect_changed(move |entry| {
            config.borrow_mut().wallpaper.color = entry.text().to_string();
        });
    }
    content.append(&row("Base colour", "Used by solid wallpapers and as a fallback.", &color));

    let gradient = gtk::Entry::new();
    gradient.set_text(&config.borrow().wallpaper.gradient.join(", "));
    {
        let config = config.clone();
        gradient.connect_changed(move |entry| {
            let colors = entry
                .text()
                .split(',')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect::<Vec<_>>();
            if !colors.is_empty() {
                config.borrow_mut().wallpaper.gradient = colors;
            }
        });
    }
    content.append(&row("Gradient stops", "Comma separated hex colours, from first to last.", &gradient));

    let path = gtk::Entry::new();
    path.set_text(&config.borrow().wallpaper.path);
    path.set_placeholder_text(Some("/home/user/Pictures/wallpaper.jpg"));
    path.set_hexpand(true);
    {
        let config = config.clone();
        path.connect_changed(move |entry| {
            config.borrow_mut().wallpaper.path = entry.text().to_string();
        });
    }
    let browse = gtk::Button::with_label("Browse…");
    browse.add_css_class("suggested");
    {
        let path = path.clone();
        let config = config.clone();
        let window = window.clone();
        browse.connect_clicked(move |_| {
            let dialog = gtk::FileDialog::builder()
                .title("Choose a wallpaper")
                .modal(true)
                .build();
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Images"));
            filter.add_mime_type("image/png");
            filter.add_mime_type("image/jpeg");
            filter.add_mime_type("image/webp");
            filter.add_mime_type("image/bmp");
            filter.add_pattern("*.png");
            filter.add_pattern("*.jpg");
            filter.add_pattern("*.jpeg");
            filter.add_pattern("*.webp");
            filter.add_pattern("*.bmp");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            dialog.set_filters(Some(&filters));
            dialog.set_default_filter(Some(&filter));
            let path_entry = path.clone();
            let config = config.clone();
            dialog.open(Some(&window), None::<&gio::Cancellable>, move |result| {
                let Ok(file) = result else {
                    return;
                };
                let Some(file_path) = file.path() else {
                    return;
                };
                let Some(text) = file_path.to_str() else {
                    return;
                };
                path_entry.set_text(text);
                {
                    let mut config = config.borrow_mut();
                    config.wallpaper.style = "image".into();
                    config.wallpaper.path = text.to_string();
                }
                send(&config, serde_json::json!({ "wallpaper": config.borrow().wallpaper }));
            });
        });
    }
    let path_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    path_row.append(&path);
    path_row.append(&browse);
    content.append(&row("Image path", "Pick a PNG, JPEG, WebP or BMP image, or type a path.", &path_row));

    let angle = scale(config.borrow().wallpaper.angle as f64, 0.0, 360.0, 1.0);
    {
        let config = config.clone();
        angle.connect_value_changed(move |scale| {
            config.borrow_mut().wallpaper.angle = scale.value() as f32;
        });
    }
    content.append(&row("Gradient angle", "Direction of the generated gradient.", &angle));

    let apply = gtk::Button::with_label("Apply wallpaper");
    apply.add_css_class("suggested");
    apply.set_halign(gtk::Align::Start);
    {
        let config = config.clone();
        apply.connect_clicked(move |_| {
            send(&config, serde_json::json!({ "wallpaper": config.borrow().wallpaper }));
        });
    }
    content.append(&apply);
    scroller
}

fn make_shortcuts_page(config: &Rc<RefCell<KyonConfig>>) -> gtk::ScrolledWindow {
    let (scroller, content) = page(
        "Keyboard & Shortcuts",
        "Keyboard layout settings apply live. Bindings can be overridden in ~/.config/kyonDE/config.lua with kyde.bind().",
    );

    let layout = gtk::Entry::new();
    layout.set_text(&config.borrow().input.keyboard_layout);
    layout.set_placeholder_text(Some("us"));
    layout.set_width_chars(8);
    let variant = gtk::Entry::new();
    variant.set_text(&config.borrow().input.keyboard_variant);
    variant.set_placeholder_text(Some("intl"));
    variant.set_width_chars(10);
    let layout_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    layout_row.append(&layout);
    layout_row.append(&variant);
    let apply_keys = gtk::Button::with_label("Apply");
    apply_keys.add_css_class("suggested");
    layout_row.append(&apply_keys);
    {
        let config = config.clone();
        let layout = layout.clone();
        let variant = variant.clone();
        apply_keys.connect_clicked(move |_| {
            {
                let mut config = config.borrow_mut();
                config.input.keyboard_layout = layout.text().to_string();
                config.input.keyboard_variant = variant.text().to_string();
            }
            send(&config, serde_json::json!({ "input": config.borrow().input }));
        });
    }
    {
        let config = config.clone();
        let layout_entry = layout.clone();
        let variant_entry = variant.clone();
        layout.connect_activate(move |_| {
            {
                let mut config = config.borrow_mut();
                config.input.keyboard_layout = layout_entry.text().to_string();
                config.input.keyboard_variant = variant_entry.text().to_string();
            }
            send(&config, serde_json::json!({ "input": config.borrow().input }));
        });
    }
    content.append(&row("Keyboard layout", "XKB layout and variant, for example us, de, fr or us with intl.", &layout_row));

    let repeat_rate = gtk::SpinButton::with_range(10.0, 200.0, 1.0);
    repeat_rate.set_value(config.borrow().input.repeat_rate as f64);
    {
        let config = config.clone();
        repeat_rate.connect_value_changed(move |spin| {
            config.borrow_mut().input.repeat_rate = spin.value() as i32;
            send(&config, serde_json::json!({ "input": config.borrow().input }));
        });
    }
    content.append(&row("Repeat rate", "Key repeats per second.", &repeat_rate));

    let repeat_delay = gtk::SpinButton::with_range(100.0, 1200.0, 10.0);
    repeat_delay.set_value(config.borrow().input.repeat_delay as f64);
    {
        let config = config.clone();
        repeat_delay.connect_value_changed(move |spin| {
            config.borrow_mut().input.repeat_delay = spin.value() as i32;
            send(&config, serde_json::json!({ "input": config.borrow().input }));
        });
    }
    content.append(&row("Repeat delay", "Delay in milliseconds before key repeat starts.", &repeat_delay));

    let natural = switch(config.borrow().input.natural_scroll);
    {
        let config = config.clone();
        natural.connect_state_set(move |_, state| {
            config.borrow_mut().input.natural_scroll = state;
            send(&config, serde_json::json!({ "input": config.borrow().input }));
            gtk::glib::Propagation::Proceed
        });
    }
    content.append(&row("Natural scrolling", "Invert scroll direction for pointer wheels and touchpads.", &natural));

    let heading = gtk::Label::new(Some("Bindings"));
    heading.add_css_class("section-title");
    heading.set_xalign(0.0);
    heading.set_margin_top(16);
    content.append(&heading);
    let bindings = config.borrow().binds.clone();
    for bind in bindings {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("row");
        let key = gtk::Label::new(Some(&format!("{} {}", bind.mods, bind.key)));
        key.add_css_class("shortcut-key");
        key.set_xalign(0.0);
        key.set_width_chars(22);
        let action = gtk::Label::new(Some(&bind.action));
        action.add_css_class("row-title");
        action.set_xalign(0.0);
        action.set_hexpand(true);
        row.append(&key);
        row.append(&action);
        content.append(&row);
    }
    scroller
}

fn make_about_page() -> gtk::ScrolledWindow {
    let (scroller, content) = page("About", "KyonDE is a Wayland desktop environment built on Smithay.");
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("row");
    let logo = gtk::Label::new(Some("Kyon"));
    logo.add_css_class("about-logo");
    logo.set_xalign(0.0);
    let title = gtk::Label::new(Some(&format!("KyonDE {}", kyon_ipc::VERSION)));
    title.add_css_class("about-title");
    title.set_xalign(0.0);
    let text = gtk::Label::new(Some(
        "Compositor: kyonde\nShell: kyonshell (GTK4 + layer-shell)\nConfiguration: ~/.config/kyonDE/config.lua\nRendering: Smithay with OpenGL ES",
    ));
    text.add_css_class("about-sub");
    text.set_xalign(0.0);
    text.set_selectable(true);
    card.append(&logo);
    card.append(&title);
    card.append(&text);
    content.append(&card);
    let card2 = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card2.add_css_class("row");
    let ideas = gtk::Label::new(None);
    ideas.set_markup("<b>Included</b>\n• Tiling, scrolling and floating layouts\n• Lua configuration with live reload\n• Rounded title bars, shadows and animations\n• Launcher, settings and themable panel");
    ideas.set_xalign(0.0);
    ideas.set_wrap(true);
    card2.append(&ideas);
    content.append(&card2);
    scroller
}
