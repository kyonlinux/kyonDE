use std::sync::{Mutex, OnceLock};

use cosmic_text::{Attrs, Buffer, Color as TextColor, Family, FontSystem, Metrics, Shaping, SwashCache};
use kyon_ipc::{KyonConfig, parse_color, theme};
use smithay::utils::{Logical, Point, Rectangle, Size};
use tiny_skia::{
    Color, FillRule, LinearGradient, Paint, PathBuilder, Pixmap, SpreadMode, Stroke, Transform,
};

use crate::{layout::LayoutMode, util::mix, window::WindowMeta};

static FONT_SYSTEM: OnceLock<Mutex<(FontSystem, SwashCache)>> = OnceLock::new();

fn fonts() -> &'static Mutex<(FontSystem, SwashCache)> {
    FONT_SYSTEM.get_or_init(|| Mutex::new((FontSystem::new(), SwashCache::new())))
}

pub fn titlebar_height(config: &KyonConfig, meta: &WindowMeta, mode: LayoutMode) -> i32 {
    if !config.decor.titlebar.enabled || !meta.ssd || meta.fullscreen || meta.shell {
        return 0;
    }
    if mode == LayoutMode::Floating || meta.floating {
        config.decor.titlebar.height
    } else {
        0
    }
}

pub struct TitlebarImage {
    pub pixels: Vec<u8>,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlebarKind {
    Windows,
    Breeze,
    Adwaita,
    Cosmic,
    Graphite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlebarButton {
    Close,
    Maximize,
    Minimize,
}

pub struct TitlebarStyle {
    pub bar: [f32; 4],
    pub text: [f32; 4],
    pub border: [f32; 4],
    pub accent: [f32; 4],
    pub active: bool,
    pub radius: i32,
    pub kind: TitlebarKind,
}

pub fn titlebar_kind(config: &KyonConfig) -> TitlebarKind {
    match config.shell.theme.as_str() {
        "windows" => TitlebarKind::Windows,
        "adwaita" => TitlebarKind::Adwaita,
        "cosmic" => TitlebarKind::Cosmic,
        "graphite" => TitlebarKind::Graphite,
        _ => TitlebarKind::Breeze,
    }
}

pub fn titlebar_style(config: &KyonConfig, active: bool) -> TitlebarStyle {
    let preset = theme(&config.shell.theme);
    let palette = preset.palette(config.shell.dark);
    let accent = parse_color(&config.shell.accent).unwrap_or_else(|| {
        parse_color(palette.accent).unwrap_or([0.25, 0.65, 0.9, 1.0])
    });
    let surface = parse_color(palette.surface).unwrap_or([0.11, 0.12, 0.15, 1.0]);
    let bg = parse_color(palette.bg).unwrap_or([0.07, 0.08, 0.1, 1.0]);
    let text = parse_color(palette.text).unwrap_or([0.95, 0.96, 0.98, 1.0]);
    let muted = parse_color(palette.text_muted).unwrap_or([0.6, 0.63, 0.7, 1.0]);
    let border = parse_color(&config.decor.active_border).unwrap_or(accent);
    let inactive = parse_color(&config.decor.inactive_border).unwrap_or(mix(bg, surface, 0.4));
    let kind = titlebar_kind(config);
    if active {
        TitlebarStyle {
            bar: [surface[0], surface[1], surface[2], config.decor.titlebar.opacity],
            text,
            border,
            accent,
            active,
            radius: config.decor.rounding,
            kind,
        }
    } else {
        TitlebarStyle {
            bar: [
                mix(bg, surface, 0.55)[0],
                mix(bg, surface, 0.55)[1],
                mix(bg, surface, 0.55)[2],
                config.decor.titlebar.opacity * 0.94,
            ],
            text: muted,
            border: inactive,
            accent,
            active,
            radius: config.decor.rounding,
            kind,
        }
    }
}

pub fn titlebar_button_layout(
    config: &KyonConfig,
    width: i32,
    height: i32,
) -> Vec<(TitlebarButton, Rectangle<i32, Logical>)> {
    let kind = titlebar_kind(config);
    let requested: Vec<String> = config
        .decor
        .titlebar
        .buttons
        .split(',')
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    let has = |name: &str| requested.iter().any(|value| value == name);
    let mut buttons = Vec::new();
    match kind {
        TitlebarKind::Windows => {
            let button_width = 46.min((width / 4).max(28));
            let mut x = width - button_width;
            for (name, kind) in [
                ("close", TitlebarButton::Close),
                ("maximize", TitlebarButton::Maximize),
                ("minimize", TitlebarButton::Minimize),
            ] {
                if has(name) || !requested.iter().any(|value| value == "close" || value == "maximize" || value == "minimize") {
                    buttons.push((kind, Rectangle::new((x, 0).into(), Size::from((button_width, height)))));
                }
                x -= button_width;
            }
        }
        TitlebarKind::Breeze => {
            let button_width = 28;
            let button_height = 24;
            let gap = 3;
            let right_pad = 6;
            let mut x = width - right_pad - button_width;
            let y = (height - button_height) / 2;
            let specs = [
                ("close", TitlebarButton::Close),
                ("maximize", TitlebarButton::Maximize),
                ("minimize", TitlebarButton::Minimize),
            ]
            .into_iter()
            .filter(|(name, _)| has(name));
            for (_, kind) in specs {
                buttons.push((kind, Rectangle::new((x, y).into(), Size::from((button_width, button_height)))));
                x -= button_width + gap;
            }
        }
        _ => {
            let button_width = 34;
            let button_height = 26;
            let gap = 2;
            let right_pad = 6;
            let mut x = width - right_pad - button_width;
            let y = (height - button_height) / 2;
            for (name, kind) in [
                ("close", TitlebarButton::Close),
                ("maximize", TitlebarButton::Maximize),
                ("minimize", TitlebarButton::Minimize),
            ] {
                if has(name) {
                    buttons.push((kind, Rectangle::new((x, y).into(), Size::from((button_width, button_height)))));
                }
                x -= button_width + gap;
            }
        }
    }
    buttons
}

pub fn titlebar_key(
    config: &KyonConfig,
    meta: &WindowMeta,
    width_px: i32,
    scale: f64,
    active: bool,
    mode: LayoutMode,
) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}",
        meta.title,
        meta.app_id,
        width_px,
        (scale * 100.0) as i32,
        active,
        config.shell.theme,
        config.shell.dark,
        config.decor.rounding,
        config.decor.titlebar.height,
        config.decor.titlebar.font_size,
        config.decor.titlebar.buttons,
        mode.as_str(),
        titlebar_kind(config)
    )
}

pub fn render_titlebar(
    config: &KyonConfig,
    meta: &WindowMeta,
    width_px: i32,
    scale: f64,
    active: bool,
) -> Option<TitlebarImage> {
    let style = titlebar_style(config, active);
    let scale = scale.max(1.0);
    let tb_h = ((config.decor.titlebar.height as f64) * scale).round() as i32;
    let overlap = ((style.radius as f64) * scale).round() as i32;
    let height = tb_h + overlap;
    let width = width_px.max(1);
    if width <= 0 || height <= 0 {
        return None;
    }
    let mut pixmap = Pixmap::new(width as u32, height as u32)?;
    let radius = (style.radius as f32) * scale as f32;
    let path = top_rounded_path(width as f32, height as f32, radius);
    let bar_color = Color::from_rgba(style.bar[0], style.bar[1], style.bar[2], style.bar[3])
        .unwrap_or(Color::TRANSPARENT);
    let grad = if matches!(style.kind, TitlebarKind::Breeze | TitlebarKind::Adwaita) {
        LinearGradient::new(
            (0.0, 0.0).into(),
            (0.0, height as f32).into(),
            vec![
                tiny_skia::GradientStop::new(0.0, bar_color),
                tiny_skia::GradientStop::new(
                    1.0,
                    Color::from_rgba(
                        (style.bar[0] * 0.82).max(0.0),
                        (style.bar[1] * 0.82).max(0.0),
                        (style.bar[2] * 0.86).max(0.0),
                        style.bar[3],
                    )
                    .unwrap_or(Color::TRANSPARENT),
                ),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        )
    } else {
        None
    };
    let mut paint = Paint::default();
    if let Some(grad) = grad {
        paint.shader = grad;
    } else {
        paint.set_color(bar_color);
    }
    paint.anti_alias = true;
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);

    let border_paint = {
        let mut p = Paint::default();
        p.set_color_rgba8(
            (style.border[0] * 255.0) as u8,
            (style.border[1] * 255.0) as u8,
            (style.border[2] * 255.0) as u8,
            ((style.border[3]) * 0.8 * 255.0) as u8,
        );
        p.anti_alias = true;
        p
    };
    if !matches!(style.kind, TitlebarKind::Windows) {
        let mut stroke = Stroke::default();
        stroke.width = (scale as f32).max(1.0);
        pixmap.stroke_path(&path, &border_paint, &stroke, Transform::identity(), None);
    } else {
        let mut p = Paint::default();
        p.set_color_rgba8(255, 255, 255, 14);
        pixmap.fill_rect(
            tiny_skia::Rect::from_xywh(0.0, 0.0, width as f32, (scale as f32).max(1.0)).unwrap(),
            &p,
            Transform::identity(),
            None,
        );
    }

    let logical_width = ((width as f64) / scale).round() as i32;
    let logical_tb = (tb_h as f64 / scale).round() as i32;
    let physical_buttons: Vec<(TitlebarButton, Rectangle<i32, smithay::utils::Physical>)> =
        titlebar_button_layout(config, logical_width, logical_tb)
        .into_iter()
        .map(|(kind, rect)| {
            (
                kind,
                Rectangle::new(
                    Point::from((
                        (rect.loc.x as f64 * scale).round() as i32,
                        (rect.loc.y as f64 * scale).round() as i32,
                    )),
                    Size::from((
                        (rect.size.w as f64 * scale).round() as i32,
                        (rect.size.h as f64 * scale).round() as i32,
                    )),
                ),
            )
        })
        .collect::<Vec<_>>();

    let glyph_color = Color::from_rgba(style.text[0], style.text[1], style.text[2], style.text[3])
        .unwrap_or(Color::WHITE);
    for (kind, rect) in &physical_buttons {
        let width_px = rect.size.w as f32;
        let height_px = rect.size.h as f32;
        let cx = rect.loc.x as f32 + width_px * 0.5;
        let cy = rect.loc.y as f32 + height_px * 0.5;
        if style.active {
            let hover = if *kind == TitlebarButton::Close {
                match style.kind {
                    TitlebarKind::Windows => Color::from_rgba8(196, 43, 28, 255),
                    TitlebarKind::Breeze => Color::from_rgba8(220, 70, 70, 200),
                    _ => Color::from_rgba8(220, 70, 70, 190),
                }
            } else {
                Color::from_rgba8(255, 255, 255, 16)
            };
            let _ = hover;
        }
        let mut gp = Paint::default();
        gp.set_color(glyph_color);
        gp.anti_alias = true;
        let mut gs = Stroke::default();
        gs.width = (1.4 * scale as f32).max(1.0);
        let size = (4.5 * scale as f32).max(3.0);
        let mut pb = PathBuilder::new();
        match kind {
            TitlebarButton::Close => {
                pb.move_to(cx - size, cy - size);
                pb.line_to(cx + size, cy + size);
                pb.move_to(cx + size, cy - size);
                pb.line_to(cx - size, cy + size);
            }
            TitlebarButton::Maximize => {
                pb.push_rect(
                    tiny_skia::Rect::from_xywh(cx - size, cy - size, size * 2.0, size * 2.0).unwrap(),
                );
            }
            TitlebarButton::Minimize => {
                pb.move_to(cx - size, cy + size * 0.8);
                pb.line_to(cx + size, cy + size * 0.8);
            }
        }
        if let Some(path) = pb.finish() {
            pixmap.stroke_path(&path, &gp, &gs, Transform::identity(), None);
        }
    }

    let icon = (16.0 * scale as f32).min(tb_h as f32 * 0.46);
    let icon_x = 12.0 * scale as f32;
    let icon_y = (tb_h as f32 - icon) * 0.5;
    let icon_rect = tiny_skia::Rect::from_xywh(icon_x, icon_y, icon, icon).unwrap();
    let mut icon_pb = PathBuilder::new();
    icon_pb.push_rect(icon_rect);
    if let Some(path) = icon_pb.finish() {
        let mut p = Paint::default();
        p.set_color_rgba8(
            (style.accent[0] * 255.0) as u8,
            (style.accent[1] * 255.0) as u8,
            (style.accent[2] * 255.0) as u8,
            235,
        );
        p.anti_alias = true;
        pixmap.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
    }
    let app_name = if meta.app_id.is_empty() { meta.title.clone() } else { meta.app_id.clone() };
    let initial = app_name
        .split('.')
        .next_back()
        .unwrap_or(&app_name)
        .chars()
        .next()
        .unwrap_or('K')
        .to_uppercase()
        .to_string();
    draw_text_at(
        &mut pixmap,
        &initial,
        icon_x + icon * 0.5,
        icon_y + icon * 0.5,
        9.0 * scale as f32,
        [1.0, 1.0, 1.0, 0.95],
    );

    let button_left = physical_buttons
        .iter()
        .map(|(_, rect)| rect.loc.x)
        .min()
        .unwrap_or(width);
    let title_top = if matches!(style.kind, TitlebarKind::Windows) {
        icon_x + icon + 9.0 * scale as f32
    } else {
        icon_x + icon + 12.0 * scale as f32
    };
    let title_right = (button_left as f32 - 8.0 * scale as f32).max(title_top + 20.0);
    let title_width = (title_right - title_top).max(20.0);
    let left_align = matches!(style.kind, TitlebarKind::Windows | TitlebarKind::Graphite);
    if left_align {
        draw_title(
            &mut pixmap,
            &meta.title,
            title_top,
            title_width,
            tb_h as f32,
            scale,
            style.text,
            false,
        );
    } else {
        draw_title(
            &mut pixmap,
            &meta.title,
            title_top,
            title_width,
            tb_h as f32,
            scale,
            style.text,
            true,
        );
    }

    Some(TitlebarImage {
        pixels: pixmap.data().to_vec(),
        width,
        height,
    })
}

fn draw_text_at(
    pixmap: &mut Pixmap,
    text: &str,
    center_x: f32,
    center_y: f32,
    size: f32,
    color: [f32; 4],
) {
    let line_height = size * 1.25;
    let mut guard = match fonts().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    let (font_system, swash) = &mut *guard;
    let mut buffer = Buffer::new(font_system, Metrics::new(size, line_height));
    buffer.set_size(None, None);
    buffer.set_text(text, &Attrs::new().family(Family::SansSerif), Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);
    let line_w = buffer.layout_runs().next().map(|run| run.line_w).unwrap_or(0.0);
    let offset_x = center_x - line_w * 0.5;
    let offset_y = center_y - line_height * 0.5 - size * 0.12;
    let text_color = TextColor::rgba(
        (color[0] * 255.0) as u8,
        (color[1] * 255.0) as u8,
        (color[2] * 255.0) as u8,
        (color[3] * 255.0) as u8,
    );
    let width = pixmap.width() as i32;
    let px_height = pixmap.height() as i32;
    let data = pixmap.data_mut();
    buffer.draw(font_system, swash, text_color, |x, y, w, h, glyph| {
        let [r, g, b, a] = glyph.as_rgba();
        if a == 0 {
            return;
        }
        for dy in 0..h as i32 {
            for dx in 0..w as i32 {
                let px = offset_x.round() as i32 + x + dx;
                let py = offset_y.round() as i32 + y + dy;
                if px < 0 || py < 0 || px >= width || py >= px_height {
                    continue;
                }
                let idx = ((py * width + px) * 4) as usize;
                if idx + 3 >= data.len() {
                    continue;
                }
                let src_a = a as f32 / 255.0;
                let dst = &mut data[idx..idx + 4];
                let inv = 1.0 - src_a;
                dst[0] = ((r as f32 * src_a) + dst[0] as f32 * inv).min(255.0) as u8;
                dst[1] = ((g as f32 * src_a) + dst[1] as f32 * inv).min(255.0) as u8;
                dst[2] = ((b as f32 * src_a) + dst[2] as f32 * inv).min(255.0) as u8;
                dst[3] = ((dst[3] as f32) + (255.0 - dst[3] as f32) * src_a).min(255.0) as u8;
            }
        }
    });
}

fn draw_title(
    pixmap: &mut Pixmap,
    title: &str,
    left: f32,
    avail: f32,
    height: f32,
    scale: f64,
    color: [f32; 4],
    centered: bool,
) {
    let title = if title.is_empty() { "Untitled window" } else { title };
    let size = (12.5 * scale as f32).max(8.0);
    let line_height = size * 1.35;
    let mut guard = match fonts().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    let (font_system, swash) = &mut *guard;
    let mut buffer = Buffer::new(font_system, Metrics::new(size, line_height));
    buffer.set_size(Some(avail), Some(height));
    buffer.set_text(title, &Attrs::new().family(Family::SansSerif), Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);
    let line_w = buffer.layout_runs().next().map(|run| run.line_w).unwrap_or(0.0);
    let offset_x = if centered {
        left + ((avail - line_w) * 0.5).max(0.0)
    } else {
        left
    };
    let offset_y = (height - line_height) * 0.5 - size * 0.12;
    let text_color = TextColor::rgba(
        (color[0] * 255.0) as u8,
        (color[1] * 255.0) as u8,
        (color[2] * 255.0) as u8,
        (color[3] * 255.0) as u8,
    );
    let width = pixmap.width() as i32;
    let px_height = pixmap.height() as i32;
    let data = pixmap.data_mut();
    buffer.draw(font_system, swash, text_color, |x, y, w, h, glyph| {
        let [r, g, b, a] = glyph.as_rgba();
        if a == 0 {
            return;
        }
        for dy in 0..h as i32 {
            for dx in 0..w as i32 {
                let px = offset_x.round() as i32 + x + dx;
                let py = offset_y.round() as i32 + y + dy;
                if px < 0 || py < 0 || px >= width || py >= px_height {
                    continue;
                }
                let idx = ((py * width + px) * 4) as usize;
                if idx + 3 >= data.len() {
                    continue;
                }
                let src_a = a as f32 / 255.0;
                let dst = &mut data[idx..idx + 4];
                let inv = 1.0 - src_a;
                dst[0] = ((r as f32 * src_a) + dst[0] as f32 * inv).min(255.0) as u8;
                dst[1] = ((g as f32 * src_a) + dst[1] as f32 * inv).min(255.0) as u8;
                dst[2] = ((b as f32 * src_a) + dst[2] as f32 * inv).min(255.0) as u8;
                dst[3] = ((dst[3] as f32) + (255.0 - dst[3] as f32) * src_a).min(255.0) as u8;
            }
        }
    });
}

fn top_rounded_path(w: f32, h: f32, r: f32) -> tiny_skia::Path {
    let r = r.min(w / 2.0).min(h);
    let mut pb = PathBuilder::new();
    pb.move_to(r, 0.0);
    pb.line_to(w - r, 0.0);
    pb.quad_to(w, 0.0, w, r);
    pb.line_to(w, h);
    pb.line_to(0.0, h);
    pb.line_to(0.0, r);
    pb.quad_to(0.0, 0.0, r, 0.0);
    pb.close();
    pb.finish().unwrap_or_else(|| {
        let mut fallback = PathBuilder::new();
        fallback.push_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, w, h).unwrap());
        fallback.finish().unwrap()
    })
}


pub fn cursor_image(scale: f64) -> (Vec<u8>, i32, i32) {
    let size = ((22.0 * scale).round() as i32).max(16);
    let mut pixmap = Pixmap::new(size as u32, size as u32).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
    let s = size as f32;
    let mut pb = PathBuilder::new();
    pb.move_to(s * 0.12, s * 0.08);
    pb.line_to(s * 0.12, s * 0.80);
    pb.line_to(s * 0.32, s * 0.62);
    pb.line_to(s * 0.45, s * 0.92);
    pb.line_to(s * 0.58, s * 0.86);
    pb.line_to(s * 0.45, s * 0.57);
    pb.line_to(s * 0.70, s * 0.55);
    pb.close();
    if let Some(path) = pb.finish() {
        let mut p = Paint::default();
        p.set_color_rgba8(255, 255, 255, 255);
        p.anti_alias = true;
        pixmap.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
        let mut sp = Paint::default();
        sp.set_color_rgba8(20, 22, 28, 255);
        sp.anti_alias = true;
        let mut stroke = Stroke::default();
        stroke.width = (1.4 * scale as f32).max(1.0);
        pixmap.stroke_path(&path, &sp, &stroke, Transform::identity(), None);
    }
    let mut pixels = Vec::with_capacity(pixmap.data().len());
    for px in pixmap.data().chunks_exact(4) {
        pixels.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    (pixels, size, size)
}


