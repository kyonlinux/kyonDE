use gtk4 as gtk;
use gtk4::prelude::*;
use kyon_ipc::{parse_color, theme};

use crate::prefs::ShellPrefs;

fn rgba(hex: &str, alpha: f32) -> String {
    let c = parse_color(hex).unwrap_or([0.5, 0.5, 0.5, 1.0]);
    format!(
        "rgba({},{},{},{:.3})",
        (c[0] * 255.0) as u32,
        (c[1] * 255.0) as u32,
        (c[2] * 255.0) as u32,
        alpha
    )
}

fn solid(hex: &str) -> String {
    rgba(hex, 1.0)
}

fn contrast(bg: &str) -> &'static str {
    let c = parse_color(bg).unwrap_or([0.5, 0.5, 0.5, 1.0]);
    let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    if l > 0.6 { "#111318" } else { "#ffffff" }
}

pub fn install(provider: &gtk::CssProvider, prefs: &ShellPrefs) {
    let css = build(prefs);
    provider.load_from_data(&css);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}

pub fn build(prefs: &ShellPrefs) -> String {
    let preset = theme(&prefs.theme);
    let p = preset.palette(prefs.dark);
    let accent = prefs.accent_color();
    let radius = preset.rounding;
    let panel_radius = if prefs.panel_floating { radius + 2 } else { radius.min(10) };
    let panel_alpha = prefs.panel_opacity.clamp(0.45, 1.0);
    let family = prefs.font.split_whitespace().next().unwrap_or("Inter");
    let font_size = prefs
        .font
        .split_whitespace()
        .nth(1)
        .unwrap_or("10")
        .trim_end_matches("pt");
    let accent_text = contrast(&accent);
    let shadow = if prefs.dark { "rgba(0,0,0,0.45)" } else { "rgba(0,0,0,0.18)" };
    let panel_bg = if prefs.theme == "breeze" {
        rgba(p.surface, panel_alpha)
    } else if prefs.theme == "windows" {
        rgba(p.surface, panel_alpha.max(0.72))
    } else {
        rgba(p.surface, panel_alpha)
    };

    format!(
        r#"
* {{
    font-family: "{family}", "Noto Sans", sans-serif;
    font-size: {font_size}pt;
}}
window.shell-window, window.shell-window > contents {{
    background-color: transparent;
    background-image: none;
    box-shadow: none;
}}
.panel-frame {{
    background-color: {panel_bg};
    border: 1px solid {border};
    border-radius: {panel_radius}px;
    padding: 2px 8px;
    box-shadow: 0 6px 22px {shadow};
    color: {text};
}}
.panel-frame button,
.panel-frame menubutton > button {{
    background: transparent;
    background-image: none;
    border: none;
    box-shadow: none;
    border-radius: {small_radius}px;
    padding: 4px 9px;
    min-height: 22px;
    color: {text};
}}
.panel-frame button:hover,
.panel-frame menubutton > button:hover {{
    background-color: {hover};
}}
.panel-frame button:active {{
    background-color: {active};
}}
.panel-title {{
    font-weight: 600;
    color: {text};
    font-size: 9.5pt;
}}
.panel-subtle {{
    color: {muted};
    font-size: 8.5pt;
}}
.panel-brand {{
    color: {accent};
    font-weight: 800;
    letter-spacing: 0.4px;
}}
.taskbar {{
    margin-left: 6px;
    padding-left: 6px;
    border-left: 1px solid {border};
}}
.taskbar-button {{
    min-width: 26px;
    min-height: 24px;
    padding: 2px 4px;
    border-radius: {small_radius}px;
}}
.taskbar-button:hover {{
    background-color: {hover};
}}
.ws {{
    min-width: 22px;
    min-height: 22px;
    padding: 0;
    margin: 0 1px;
    border-radius: 999px;
    font-size: 8.5pt;
    font-weight: 600;
    color: {muted};
}}
.ws:hover {{
    background-color: {hover};
}}
.ws-active {{
    background-color: {accent};
    color: {accent_text};
}}
.ws-active:hover {{
    background-color: {accent};
}}
.clock {{
    font-weight: 600;
    letter-spacing: 0.2px;
}}
.mode-pill {{
    border-radius: 999px;
    padding: 3px 10px;
    background-color: {hover};
    color: {accent};
    font-weight: 700;
    font-size: 8.5pt;
}}
.launcher-backdrop {{
    background-color: rgba(0,0,0,0.42);
}}
.launcher-card {{
    background-color: {surface_high};
    border: 1px solid {border};
    border-radius: {launcher_radius}px;
    padding: 18px;
    box-shadow: 0 18px 60px {shadow};
    color: {text};
}}
.launcher-search {{
    background-color: {surface_alt};
    border: 1px solid {border};
    border-radius: {small_radius}px;
    padding: 8px 12px;
    min-height: 28px;
    color: {text};
    font-size: 11pt;
}}
.launcher-search:focus {{
    border-color: {accent};
}}
.app-button {{
    background: transparent;
    border: none;
    box-shadow: none;
    border-radius: {small_radius}px;
    padding: 8px;
    min-width: 92px;
}}
.app-button:hover {{
    background-color: {hover};
}}
.app-name {{
    font-size: 8.8pt;
    color: {text};
}}
.app-desc {{
    color: {muted};
    font-size: 8pt;
}}
.dim {{
    color: {muted};
}}
.launcher-section {{
    color: {muted};
    font-size: 8.5pt;
    font-weight: 700;
    letter-spacing: 0.8px;
}}
.quick {{
    background-color: {surface_alt};
    border: 1px solid {border};
    border-radius: {small_radius}px;
    padding: 8px 14px;
}}
.quick:hover {{
    background-color: {hover};
}}
window.settings-window {{
    background-color: {bg};
    color: {text};
}}
window.settings-window headerbar {{
    background-color: {surface};
    background-image: none;
    color: {text};
    border-bottom: 1px solid {border};
    box-shadow: none;
    min-height: 42px;
}}
window.settings-window headerbar .title {{
    font-weight: 700;
}}
.settings-body {{
    background-color: {bg};
}}
.sidebar {{
    background-color: {surface};
    border-right: 1px solid {border};
    padding: 14px 10px;
}}
.sidebar-title {{
    font-size: 15pt;
    font-weight: 800;
    color: {text};
    margin: 6px 10px 16px 10px;
}}
.sidebar-sub {{
    color: {muted};
    font-size: 8.5pt;
    margin: 0 10px 16px 10px;
}}
.nav-button {{
    background: transparent;
    background-image: none;
    border: none;
    box-shadow: none;
    border-radius: {small_radius}px;
    padding: 9px 12px;
    color: {text};
}}
.nav-button:hover {{
    background-color: {hover};
}}
.nav-button.active {{
    background-color: {accent};
    color: {accent_text};
}}
.page {{
    padding: 26px 30px;
}}
.page-scroll {{
    background-color: transparent;
}}
.section-title {{
    font-size: 19pt;
    font-weight: 800;
    color: {text};
}}
.section-desc {{
    color: {muted};
    margin-bottom: 18px;
}}
.row {{
    background-color: {surface};
    border: 1px solid {border};
    border-radius: {small_radius}px;
    padding: 12px 14px;
    margin-bottom: 8px;
}}
.row-title {{
    font-weight: 650;
    color: {text};
}}
.row-desc {{
    color: {muted};
    font-size: 8.6pt;
}}
.theme-card {{
    background-color: {surface};
    border: 1px solid {border};
    border-radius: {launcher_radius}px;
    padding: 10px;
    min-width: 180px;
    box-shadow: none;
}}
.theme-card:hover {{
    background-color: {surface_alt};
}}
.theme-card.selected {{
    border: 2px solid {accent};
}}
.theme-name {{
    font-weight: 700;
    color: {text};
}}
.theme-desc {{
    color: {muted};
    font-size: 8.2pt;
}}
.swatch-row {{
    border-radius: {small_radius}px;
    min-height: 34px;
}}
.preview-bar {{
    background-color: {accent};
    border-radius: 3px;
    min-height: 8px;
}}
entry, spinbutton {{
    background-color: {surface_alt};
    border: 1px solid {border};
    border-radius: {small_radius}px;
    color: {text};
    min-height: 28px;
}}
entry:focus {{
    border-color: {accent};
}}
combobox button {{
    background-color: {surface_alt};
    border: 1px solid {border};
    border-radius: {small_radius}px;
    color: {text};
    box-shadow: none;
    min-height: 28px;
}}
switch {{
    background-color: {surface_alt};
    border: 1px solid {border};
    border-radius: 999px;
}}
switch:checked {{
    background-color: {accent};
    border-color: {accent};
}}
switch > slider {{
    background-color: {text};
    border-radius: 999px;
}}
scale trough {{
    background-color: {surface_alt};
    border-radius: 999px;
    min-height: 6px;
}}
scale highlight {{
    background-color: {accent};
    border-radius: 999px;
}}
scale slider {{
    background-color: {text};
    border-radius: 999px;
    min-width: 16px;
    min-height: 16px;
}}
button {{
    box-shadow: none;
}}
.suggested {{
    background-color: {accent};
    color: {accent_text};
    border-radius: {small_radius}px;
    padding: 6px 14px;
}}
.suggested:hover {{
    background-color: {accent_hover};
}}
.power {{
    color: {danger};
}}
.shortcut-key {{
    background-color: {surface_alt};
    border: 1px solid {border};
    border-radius: 5px;
    padding: 2px 7px;
    font-family: monospace;
    font-size: 8.5pt;
    color: {text};
}}
.about-logo {{
    color: {accent};
    font-size: 34pt;
    font-weight: 900;
}}
.about-title {{
    font-size: 22pt;
    font-weight: 800;
    color: {text};
}}
window.desktop-window, window.desktop-window > contents {{
    background-color: transparent;
    background-image: none;
    box-shadow: none;
}}
.desktop-surface {{
    background-color: transparent;
}}
.desktop-icon {{
    background: transparent;
    border-radius: {small_radius}px;
    padding: 6px;
}}
.desktop-icon:hover {{
    background-color: {hover};
}}
.desktop-icon.selected {{
    background-color: {active};
}}
.desktop-label {{
    color: #ffffff;
    background-color: rgba(0,0,0,0.36);
    border-radius: 4px;
    padding: 1px 5px;
    font-size: 8.7pt;
}}
.context-menu {{
    background-color: {surface_high};
    border: 1px solid {border};
    border-radius: {small_radius}px;
    padding: 5px;
    box-shadow: 0 10px 30px {shadow};
}}
.context-menu button, .context-item {{
    background: transparent;
    border: none;
    box-shadow: none;
    border-radius: 5px;
    color: {text};
    padding: 5px 10px;
}}
.context-menu button:hover, .context-item:hover {{
    background-color: {hover};
}}
.about-sub {{
    color: {muted};
}}
.link {{
    color: {accent};
}}
"#,
        family = family,
        font_size = font_size,
        border = solid(p.border),
        text = solid(p.text),
        muted = solid(p.text_muted),
        accent = solid(&accent),
        accent_hover = rgba(&accent, 0.85),
        accent_text = accent_text,
        hover = rgba(&accent, if prefs.dark { 0.22 } else { 0.14 }),
        active = rgba(&accent, 0.32),
        surface = solid(p.surface),
        surface_alt = solid(p.surface_alt),
        surface_high = solid(p.surface_high),
        bg = solid(p.bg),
        danger = solid(p.danger),
        shadow = shadow,
        small_radius = (radius - 2).max(4),
        launcher_radius = radius + 4,
        panel_radius = panel_radius,
        panel_bg = panel_bg,
    )
}

pub fn swatch_css(prefs: &ShellPrefs, theme_id: &str) -> String {
    let preset = theme(theme_id);
    let p = preset.palette(prefs.dark);
    format!(
        ".swatch-{} {{ background-color: {}; border-radius: 5px; min-height: 26px; }}",
        theme_id.replace('-', "_"),
        solid(p.surface)
    )
}
