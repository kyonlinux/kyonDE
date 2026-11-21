use std::{
    collections::HashMap,
    time::Instant,
};

use kyon_ipc::{KyonConfig, parse_color, theme};
use smithay::{
    utils::IsAlive,
    backend::{
        allocator::Fourcc,
        renderer::{
            Frame, ImportMem, Texture,
            element::{
                AsRenderElements, Element, Kind, RenderElement,
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
            gles::{
                GlesFrame, GlesPixelProgram, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName,
                UniformType,
            },
        },
    },
    desktop::{PopupManager, space::SpaceElement},
    output::Output,
    reexports::wayland_server::{Resource, backend::ObjectId, protocol::wl_surface::WlSurface},
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform},
    wayland::shell::wlr_layer::Layer,
};

use crate::{
    decor::{self, titlebar_height, titlebar_key},
    layout::LayoutMode,
    util::mix,
    window::WindowElement,
};

const WINDOW_SHADER: &str = r#"
//_DEFINES
precision mediump float;
uniform sampler2D tex;
uniform float alpha;
uniform vec2 u_size;
uniform float u_radius;
uniform float u_border;
uniform vec4 u_border_color;
varying vec2 v_coords;
float sd_round(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - (half_size - vec2(r));
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}
void main() {
    vec2 size = max(u_size, vec2(1.0));
    vec2 p = v_coords * size;
    float d = sd_round(p - size * 0.5, size * 0.5, u_radius);
    float edge = 1.0 - smoothstep(-1.0, 1.0, d);
    if (edge <= 0.001) discard;
    vec4 color = texture2D(tex, v_coords);
    float inner = smoothstep(-u_border - 1.0, -u_border + 1.0, d);
    vec3 border = u_border_color.rgb;
    color = vec4(mix(border, color.rgb, inner), color.a);
    gl_FragColor = color * alpha * edge;
}
"#;

const CORNER_SHADER: &str = r#"
//_DEFINES
precision mediump float;
uniform sampler2D tex;
uniform float alpha;
uniform vec2 u_size;
uniform float u_radius;
uniform vec2 u_dest;
varying vec2 v_coords;
float sd_round(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - (half_size - vec2(r));
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}
void main() {
    vec2 p = gl_FragCoord.xy - u_dest;
    float d = sd_round(p - u_size * 0.5, u_size * 0.5, u_radius);
    if (d < 0.0) discard;
    vec4 color = texture2D(tex, v_coords);
    gl_FragColor = color * alpha;
}
"#;

const SHADOW_SHADER: &str = r#"
precision mediump float;
uniform float alpha;
uniform vec2 u_quad;
uniform vec2 u_inner;
uniform float u_radius;
uniform float u_margin;
uniform vec4 u_color;
varying vec2 v_coords;
float sd_round(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - (half_size - vec2(r));
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}
void main() {
    vec2 p = v_coords * u_quad;
    float d = sd_round(p - u_quad * 0.5, u_inner * 0.5, u_radius);
    float a = 1.0 - smoothstep(-u_margin * 0.65, u_margin, d);
    a *= u_color.a * alpha;
    gl_FragColor = vec4(u_color.rgb * a, a);
}
"#;

pub struct Shaders {
    pub window: GlesTexProgram,
    pub shadow: GlesPixelProgram,
    pub corner: GlesTexProgram,
}

impl Shaders {
    pub fn compile(renderer: &mut GlesRenderer) -> Result<Self, String> {
        let uniforms = [
            UniformName {
                name: "u_size".into(),
                type_: UniformType::_2f,
            },
            UniformName {
                name: "u_radius".into(),
                type_: UniformType::_1f,
            },
            UniformName {
                name: "u_border".into(),
                type_: UniformType::_1f,
            },
            UniformName {
                name: "u_border_color".into(),
                type_: UniformType::_4f,
            },
        ];
        let window = renderer
            .compile_custom_texture_shader(WINDOW_SHADER, &uniforms)
            .map_err(|err| format!("{err:?}"))?;
        let corner_uniforms = [
            UniformName { name: "u_size".into(), type_: UniformType::_2f },
            UniformName { name: "u_radius".into(), type_: UniformType::_1f },
            UniformName { name: "u_dest".into(), type_: UniformType::_2f },
        ];
        let corner = renderer
            .compile_custom_texture_shader(CORNER_SHADER, &corner_uniforms)
            .map_err(|err| format!("{err:?}"))?;
        let uniforms = [
            UniformName {
                name: "u_quad".into(),
                type_: UniformType::_2f,
            },
            UniformName {
                name: "u_inner".into(),
                type_: UniformType::_2f,
            },
            UniformName {
                name: "u_radius".into(),
                type_: UniformType::_1f,
            },
            UniformName {
                name: "u_margin".into(),
                type_: UniformType::_1f,
            },
            UniformName {
                name: "u_color".into(),
                type_: UniformType::_4f,
            },
        ];
        let shadow = renderer
            .compile_custom_pixel_shader(SHADOW_SHADER, &uniforms)
            .map_err(|err| format!("{err:?}"))?;
        Ok(Self { window, shadow, corner })
    }
}

pub struct WallpaperCache {
    pub key: String,
    pub texture: GlesTexture,
    pub size: Size<i32, Buffer>,
    pub pixels: Vec<u8>,
}

pub struct TitlebarCache {
    pub key: String,
    pub texture: GlesTexture,
}

pub struct CursorTexture {
    pub scale: i32,
    pub texture: GlesTexture,
    pub hotspot: Point<i32, Physical>,
}

pub struct PreparedWindow {
    pub elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
    pub content_dest: Rectangle<i32, Physical>,
    pub titlebar: Option<GlesTexture>,
    pub titlebar_dest: Option<Rectangle<i32, Physical>>,
    pub outer: Rectangle<f64, Logical>,
    pub rounding: f32,
    pub border: [f32; 4],
    pub border_width: f32,
    pub alpha: f32,
    pub shadow: Option<[f32; 4]>,
    pub shadow_size: f32,
}

pub fn physical_size(output: &Output) -> Size<i32, Physical> {
    let mode = output
        .current_mode()
        .unwrap_or(smithay::output::Mode {
            size: (1280, 720).into(),
            refresh: 60_000,
        });
    output.current_transform().transform_size(mode.size)
}

fn draw_elements(frame: &mut GlesFrame<'_, '_>, elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>>, scale: f64) {
    for element in elements {
        let dst = element.geometry(Scale::from(scale));
        let src = element.src();
        let _ = element.draw(frame, src, dst, &[dst], &[], None);
    }
}

pub fn collect_layers(
    renderer: &mut GlesRenderer,
    output: &Output,
    layer: Layer,
    scale: f64,
) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
    let map = smithay::desktop::layer_map_for_output(output);
    let mut elements = Vec::new();
    for surface in map.layers_on(layer) {
        let Some(geometry) = map.layer_geometry(surface) else {
            continue;
        };
        elements.extend(AsRenderElements::<GlesRenderer>::render_elements::<
            WaylandSurfaceRenderElement<GlesRenderer>,
        >(
            surface,
            renderer,
            geometry.loc.to_physical_precise_round(scale),
            Scale::from(scale),
            1.0,
        ));
    }
    elements
}

pub fn collect_popups(
    renderer: &mut GlesRenderer,
    windows: &[WindowElement],
    scale: f64,
) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
    let mut elements = Vec::new();
    for window in windows {
        let Some(surface) = window.wl_surface() else {
            continue;
        };
        let geometry = window.0.geometry();
        for (popup, offset) in PopupManager::popups_for_surface(&surface) {
            let location = (geometry.loc + offset - popup.geometry().loc)
                .to_physical_precise_round(scale);
            elements.extend(render_elements_from_surface_tree(
                renderer,
                popup.wl_surface(),
                location,
                scale,
                1.0,
                Kind::Unspecified,
            ));
        }
    }
    elements
}

pub fn draw_wallpaper(
    frame: &mut GlesFrame<'_, '_>,
    wallpaper: &WallpaperCache,
    output_size: Size<i32, Physical>,
) {
    let _ = frame.render_texture_from_to(
        &wallpaper.texture,
        Rectangle::from_size(wallpaper.size).to_f64(),
        Rectangle::from_size(output_size),
        &[Rectangle::from_size(output_size)],
        &[],
        Transform::Normal,
        1.0,
        None,
        &[],
    );
}

pub fn ensure_shaders(renderer: &mut GlesRenderer) -> Option<Shaders> {
    match Shaders::compile(renderer) {
        Ok(shaders) => Some(shaders),
        Err(err) => {
            tracing::error!(err, "failed to compile shaders");
            None
        }
    }
}

pub fn ensure_cursor(
    renderer: &mut GlesRenderer,
    scale: f64,
    cache: &mut Option<CursorTexture>,
) -> Option<()> {
    let wanted = (scale.max(1.0) * 100.0) as i32;
    if cache.as_ref().is_some_and(|c| c.scale == wanted) {
        return Some(());
    }
    let (pixels, w, h) = decor::cursor_image(scale);
    let texture = renderer
        .import_memory(
            &pixels,
            Fourcc::Argb8888,
            Size::from((w, h)),
            true,
        )
        .ok()?;
    *cache = Some(CursorTexture {
        scale: wanted,
        texture,
        hotspot: Point::from((0, 0)),
    });
    Some(())
}

pub fn draw_cursor(
    frame: &mut GlesFrame<'_, '_>,
    cursor: &CursorTexture,
    location: Point<f64, Logical>,
    scale: f64,
) {
    let buffer_size = Size::<i32, Buffer>::from((cursor.texture.size().w, cursor.texture.size().h));
    let size = Size::<i32, Physical>::from((cursor.texture.size().w, cursor.texture.size().h));
    let dest = Rectangle::new(
        Point::from((
            (location.x * scale) as i32 + cursor.hotspot.x,
            (location.y * scale) as i32 + cursor.hotspot.y,
        )),
        size,
    );
    let _ = frame.render_texture_from_to(
        &cursor.texture,
        Rectangle::from_size(buffer_size).to_f64(),
        dest,
        &[dest],
        &[],
        Transform::Normal,
        1.0,
        None,
        &[],
    );
}

pub fn ensure_wallpaper(
    renderer: &mut GlesRenderer,
    config: &KyonConfig,
    size: Size<i32, Physical>,
    scale: f64,
    cache: &mut Option<WallpaperCache>,
) -> Option<()> {
    let key = format!(
        "{}|{}x{}|{}|{}|{}",
        serde_json::to_string(&config.wallpaper).unwrap_or_default(),
        size.w,
        size.h,
        config.blur.enabled,
        config.blur.wallpaper,
        scale
    );
    if cache.as_ref().is_some_and(|c| c.key == key) {
        return Some(());
    }
    let (pixels, width, height) = build_wallpaper(config, size.w, size.h, scale);
    let texture = renderer
        .import_memory(&pixels, Fourcc::Argb8888, Size::from((width, height)), true)
        .ok()?;
    *cache = Some(WallpaperCache {
        key,
        texture,
        size: Size::from((width, height)),
        pixels,
    });
    Some(())
}

fn build_wallpaper(config: &KyonConfig, width: i32, height: i32, scale: f64) -> (Vec<u8>, i32, i32) {
    if width <= 0 || height <= 0 {
        return (vec![0; 4], 1, 1);
    }
    let wallpaper = &config.wallpaper;
    let mut rgba = if wallpaper.style.eq_ignore_ascii_case("image") && !wallpaper.path.is_empty() {
        let path = expand_path(&wallpaper.path);
        match image::open(&path) {
            Ok(img) => {
                let img = img.to_rgba8();
                let (iw, ih) = (img.width() as i64, img.height() as i64);
                let (tw, th) = (width as i64, height as i64);
                let scale_f = (tw as f64 / iw as f64).max(th as f64 / ih as f64);
                let nw = (iw as f64 * scale_f).round().max(1.0) as u32;
                let nh = (ih as f64 * scale_f).round().max(1.0) as u32;
                let resized = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3);
                let ox = (nw.saturating_sub(width as u32)) / 2;
                let oy = (nh.saturating_sub(height as u32)) / 2;
                let mut out = Vec::with_capacity((width * height * 4) as usize);
                for y in 0..height as u32 {
                    for x in 0..width as u32 {
                        let px = resized.get_pixel((x + ox).min(nw - 1), (y + oy).min(nh - 1));
                        out.extend_from_slice(&px.0);
                    }
                }
                if config.blur.enabled && config.blur.wallpaper {
                    let sigma = (config.blur.size as f64 * scale * 0.9).clamp(0.5, 48.0);
                    let img: image::RgbaImage =
                        image::ImageBuffer::from_raw(width as u32, height as u32, out).unwrap();
                    let blurred = image::imageops::blur(&img, sigma as f32);
                    blurred.into_raw()
                } else {
                    out
                }
            }
            Err(err) => {
                tracing::warn!(?err, path = %path.display(), "failed to load wallpaper");
                gradient_wallpaper(wallpaper, width, height)
            }
        }
    } else if wallpaper.style.eq_ignore_ascii_case("solid") {
        let color = parse_color(&wallpaper.color).unwrap_or([0.04, 0.05, 0.08, 1.0]);
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            out.extend_from_slice(&[
                (color[0] * 255.0) as u8,
                (color[1] * 255.0) as u8,
                (color[2] * 255.0) as u8,
                255,
            ]);
        }
        out
    } else {
        gradient_wallpaper(wallpaper, width, height)
    };

    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    (rgba, width, height)
}

fn gradient_wallpaper(wallpaper: &kyon_ipc::Wallpaper, width: i32, height: i32) -> Vec<u8> {
    let mut stops: Vec<[f32; 4]> = wallpaper
        .gradient
        .iter()
        .filter_map(|color| parse_color(color))
        .collect();
    if stops.is_empty() {
        stops.push(parse_color(&wallpaper.color).unwrap_or([0.03, 0.04, 0.07, 1.0]));
    }
    let angle = wallpaper.angle.to_radians();
    let (dx, dy) = (angle.cos(), angle.sin());
    let w = width as f32;
    let h = height as f32;
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let nx = x as f32 / w;
            let ny = y as f32 / h;
            let t = ((nx - 0.5) * dx + (ny - 0.5) * dy + 0.5).clamp(0.0, 1.0);
            let scaled = t * (stops.len() - 1) as f32;
            let idx = scaled.floor() as usize;
            let frac = scaled - idx as f32;
            let a = stops[idx.min(stops.len() - 1)];
            let b = stops[(idx + 1).min(stops.len() - 1)];
            let mut color = mix(a, b, frac);
            let vx = (nx - 0.5) * 1.7;
            let vy = (ny - 0.5) * 1.7;
            let vignette = (1.0 - (vx * vx + vy * vy).clamp(0.0, 1.0) * 0.45).clamp(0.0, 1.0);
            color[0] *= vignette;
            color[1] *= vignette;
            color[2] *= vignette;
            out.extend_from_slice(&[
                (color[0] * 255.0) as u8,
                (color[1] * 255.0) as u8,
                (color[2] * 255.0) as u8,
                255,
            ]);
        }
    }
    out
}

fn expand_path(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home).join(rest);
        }
    }
    std::path::PathBuf::from(path)
}

pub fn prepare_windows(
    renderer: &mut GlesRenderer,
    windows: &[WindowElement],
    focused: Option<&WindowElement>,
    config: &KyonConfig,
    mode: LayoutMode,
    scale: f64,
    now: Instant,
    titlebars: &mut HashMap<ObjectId, TitlebarCache>,
    prepared: &mut Vec<PreparedWindow>,
) {
    prepared.clear();
    let preset = theme(&config.shell.theme);
    let palette = preset.palette(config.shell.dark);
    let accent = parse_color(&config.shell.accent)
        .or_else(|| parse_color(palette.accent))
        .unwrap_or([0.25, 0.65, 0.9, 1.0]);
    let inactive_border = parse_color(&config.decor.inactive_border).unwrap_or([0.2, 0.23, 0.3, 1.0]);
    let shadow_color = parse_color(&config.decor.shadow.color).unwrap_or([0.0, 0.0, 0.0, 1.0]);

    for window in windows {
        if !window.alive() {
            continue;
        }
        let Some(surface) = window.wl_surface() else {
            continue;
        };
        let meta = window.meta_ref();
        let active = focused.is_some_and(|f| f == window);
        let fullscreen = meta.fullscreen;
        let tb = if fullscreen {
            0
        } else {
            titlebar_height(config, &meta, mode)
        };
        let outer_target = meta.outer;
        if outer_target.size.w <= 0 || outer_target.size.h <= 0 {
            continue;
        }
        let sampled = meta.sampled_rect(now);
        let factor = (sampled.size.h / outer_target.size.h.max(1) as f64).clamp(0.2, 2.0);
        let fade = meta.fade.sample(now).clamp(0.0, 1.0);
        let opacity = meta.opacity.clamp(0.1, 1.0);
        let content_target_w = outer_target.size.w.max(1);
        let content_target_h = (outer_target.size.h - tb).max(1);
        let _ = (content_target_w, content_target_h);
        let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
            render_elements_from_surface_tree(renderer, &surface, (0, 0), scale, 1.0, Kind::Unspecified);

        let content_dest = Rectangle::new(
            Point::from((
                (sampled.loc.x * scale).round() as i32,
                ((sampled.loc.y + tb as f64 * factor) * scale).round() as i32,
            )),
            Size::from((
                (sampled.size.w * scale).round().max(1.0) as i32,
                ((sampled.size.h - tb as f64 * factor).max(1.0) * scale).round() as i32,
            )),
        );

        let mut titlebar_texture = None;
        let mut titlebar_dest = None;
        if tb > 0 {
            let width_px = (outer_target.size.w as f64 * scale).round().max(1.0) as i32;
            let key = titlebar_key(config, &meta, width_px, scale, active, mode);
            let id = surface.id();
            let needs = titlebars.get(&id).map(|c| c.key != key).unwrap_or(true);
            if needs {
                if let Some(image) = decor::render_titlebar(config, &meta, width_px, scale, active) {
                    if let Ok(texture) = renderer.import_memory(
                        &image.pixels,
                        Fourcc::Argb8888,
                        Size::from((image.width, image.height)),
                        true,
                    ) {
                        titlebars.insert(
                            id.clone(),
                            TitlebarCache {
                                key,
                                texture,
                            },
                        );
                    }
                }
            }
            if let Some(cache) = titlebars.get(&id) {
                let image_h = cache.texture.size().h as f64;
                let dest_h = (image_h * factor).round().max(1.0) as i32;
                titlebar_texture = Some(cache.texture.clone());
                titlebar_dest = Some(Rectangle::new(
                    Point::from((
                        (sampled.loc.x * scale).round() as i32,
                        (sampled.loc.y * scale).round() as i32,
                    )),
                    Size::from(((sampled.size.w * scale).round().max(1.0) as i32, dest_h)),
                ));
            }
        }

        let radius = if fullscreen { 0.0 } else { config.decor.rounding as f32 * scale as f32 * factor as f32 };
        let border = if active { accent } else { inactive_border };
        let border_width = config.decor.border_width as f32 * scale as f32;
        let alpha = fade * opacity;
        let shadow = if config.decor.shadow.enabled && !fullscreen && config.decor.shadow.size > 0 {
            Some([
                shadow_color[0],
                shadow_color[1],
                shadow_color[2],
                config.decor.shadow.opacity * fade,
            ])
        } else {
            None
        };
        prepared.push(PreparedWindow {
            elements,
            content_dest,
            titlebar: titlebar_texture,
            titlebar_dest,
            outer: sampled,
            rounding: radius,
            border,
            border_width,
            alpha,
            shadow,
            shadow_size: config.decor.shadow.size as f32 * scale as f32,
        });
    }
}

pub fn draw_prepared(
    frame: &mut GlesFrame<'_, '_>,
    shaders: &Shaders,
    windows: &[PreparedWindow],
) {
    for window in windows {
        if let Some(shadow) = window.shadow {
            let margin = window.shadow_size;
            let area = Rectangle::new(
                Point::from((
                    (window.outer.loc.x - margin as f64).round() as i32,
                    (window.outer.loc.y - margin as f64).round() as i32,
                )),
                Size::from((
                    (window.outer.size.w + margin as f64 * 2.0).round() as i32,
                    (window.outer.size.h + margin as f64 * 2.0).round() as i32,
                )),
            );
            let quad = Size::<f32, Logical>::from((area.size.w as f32, area.size.h as f32));
            let inner = Size::<f32, Logical>::from((window.outer.size.w as f32, window.outer.size.h as f32));
            let uniforms = [
                Uniform::new("u_quad", smithay::backend::renderer::gles::UniformValue::_2f(quad.w, quad.h)),
                Uniform::new("u_inner", smithay::backend::renderer::gles::UniformValue::_2f(inner.w, inner.h)),
                Uniform::new("u_radius", smithay::backend::renderer::gles::UniformValue::_1f(window.rounding)),
                Uniform::new("u_margin", smithay::backend::renderer::gles::UniformValue::_1f(margin.max(1.0))),
                Uniform::new(
                    "u_color",
                    smithay::backend::renderer::gles::UniformValue::_4f(
                        shadow[0],
                        shadow[1],
                        shadow[2],
                        shadow[3],
                    ),
                ),
            ];
            let _ = frame.render_pixel_shader_to(
                &shaders.shadow,
                Rectangle::from_size(Size::<i32, Buffer>::from((area.size.w, area.size.h))).to_f64(),
                area,
                Size::from((area.size.w, area.size.h)),
                None,
                1.0,
                &uniforms,
            );
        }
        for element in &window.elements {
            let geometry = element.geometry(Scale::from(1.0));
            let dst = Rectangle::new(
                window.content_dest.loc + geometry.loc,
                geometry.size,
            );
            let src = element.src();
            if let Err(err) = element.draw(frame, src, dst, &[dst], &[], Some(&Default::default())) {
                tracing::warn!(?err, "window element draw failed");
            }
        }
        let border = window.border;
        let bw = (window.border_width.round() as i32).max(1);
        if window.border_width >= 0.5 {
            let color = smithay::backend::renderer::Color32F::new(border[0], border[1], border[2], border[3] * window.alpha);
            let outer = window.content_dest;
            let _ = frame.draw_solid(Rectangle::new(outer.loc, Size::from((outer.size.w, bw))), &[outer], color);
            let _ = frame.draw_solid(Rectangle::new((outer.loc.x, outer.loc.y + outer.size.h - bw).into(), Size::from((outer.size.w, bw))), &[outer], color);
            let _ = frame.draw_solid(Rectangle::new(outer.loc, Size::from((bw, outer.size.h))), &[outer], color);
            let _ = frame.draw_solid(Rectangle::new((outer.loc.x + outer.size.w - bw, outer.loc.y).into(), Size::from((bw, outer.size.h))), &[outer], color);
        }
        if let (Some(titlebar), Some(dest)) = (&window.titlebar, window.titlebar_dest) {
            let _ = frame.render_texture_from_to(
                titlebar,
                Rectangle::from_size(Size::from((titlebar.size().w, titlebar.size().h))).to_f64(),
                dest,
                &[dest],
                &[],
                Transform::Normal,
                window.alpha,
                None,
                &[],
            );
        }
    }
}

pub fn draw_layer_elements(
    frame: &mut GlesFrame<'_, '_>,
    scale: f64,
    elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
) {
    draw_elements(frame, elements, scale);
}

pub fn draw_popup_elements(
    frame: &mut GlesFrame<'_, '_>,
    scale: f64,
    elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
) {
    draw_elements(frame, elements, scale);
}

pub fn draw_cursor_surface(
    frame: &mut GlesFrame<'_, '_>,
    scale: f64,
    elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
) {
    draw_elements(frame, elements, scale);
}

pub fn window_surface_elements(
    renderer: &mut GlesRenderer,
    surface: &WlSurface,
    location: Point<i32, Physical>,
    scale: f64,
) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
    render_elements_from_surface_tree(renderer, surface, location, scale, 1.0, Kind::Unspecified)
}

pub fn cursor_surface_elements(
    renderer: &mut GlesRenderer,
    surface: &WlSurface,
    location: Point<i32, Physical>,
    scale: f64,
) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
    render_elements_from_surface_tree(renderer, surface, location, scale, 1.0, Kind::Cursor)
}



#[derive(Debug)]
pub struct WallpaperElement {
    id: smithay::backend::renderer::element::Id,
    commit: smithay::backend::renderer::utils::CommitCounter,
    texture: GlesTexture,
    size: Size<i32, Physical>,
}

impl WallpaperElement {
    pub fn new(texture: GlesTexture, size: Size<i32, Physical>) -> Self {
        Self {
            id: smithay::backend::renderer::element::Id::new(),
            commit: smithay::backend::renderer::utils::CommitCounter::default(),
            texture,
            size,
        }
    }
}

impl Element for WallpaperElement {
    fn id(&self) -> &smithay::backend::renderer::element::Id { &self.id }
    fn current_commit(&self) -> smithay::backend::renderer::utils::CommitCounter { self.commit }
    fn src(&self) -> Rectangle<f64, Buffer> { Rectangle::from_size(Size::<i32, Buffer>::from((self.size.w, self.size.h))).to_f64() }
    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> { Rectangle::from_size(self.size) }
}

impl RenderElement<GlesRenderer> for WallpaperElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
        _cache: Option<&smithay::utils::user_data::UserDataMap>,
    ) -> Result<(), smithay::backend::renderer::gles::GlesError> {
        frame.render_texture_from_to(&self.texture, src, dst, damage, &[], Transform::Normal, 1.0, None, &[])
    }
}

#[derive(Debug)]
pub struct TitlebarElement {
    id: smithay::backend::renderer::element::Id,
    commit: smithay::backend::renderer::utils::CommitCounter,
    texture: GlesTexture,
    location: Point<i32, Physical>,
    size: Size<i32, Physical>,
    alpha: f32,
}

impl TitlebarElement {
    pub fn new(texture: GlesTexture, location: Point<i32, Physical>, size: Size<i32, Physical>, alpha: f32) -> Self {
        Self {
            id: smithay::backend::renderer::element::Id::new(),
            commit: smithay::backend::renderer::utils::CommitCounter::default(),
            texture,
            location,
            size,
            alpha,
        }
    }
}

impl Element for TitlebarElement {
    fn id(&self) -> &smithay::backend::renderer::element::Id { &self.id }
    fn current_commit(&self) -> smithay::backend::renderer::utils::CommitCounter { self.commit }
    fn src(&self) -> Rectangle<f64, Buffer> { Rectangle::from_size(Size::<i32, Buffer>::from((self.size.w, self.size.h))).to_f64() }
    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> { Rectangle::new(self.location, self.size) }
    fn alpha(&self) -> f32 { self.alpha }
}

impl RenderElement<GlesRenderer> for TitlebarElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
        _cache: Option<&smithay::utils::user_data::UserDataMap>,
    ) -> Result<(), smithay::backend::renderer::gles::GlesError> {
        frame.render_texture_from_to(&self.texture, src, dst, damage, &[], Transform::Normal, self.alpha, None, &[])
    }
}

#[derive(Debug)]
pub struct CursorElement {
    id: smithay::backend::renderer::element::Id,
    commit: smithay::backend::renderer::utils::CommitCounter,
    texture: GlesTexture,
    location: Point<i32, Physical>,
    size: Size<i32, Physical>,
}

impl CursorElement {
    pub fn new(texture: GlesTexture, location: Point<i32, Physical>) -> Self {
        let size = Size::from((texture.size().w, texture.size().h));
        Self {
            id: smithay::backend::renderer::element::Id::new(),
            commit: smithay::backend::renderer::utils::CommitCounter::default(),
            texture,
            location,
            size,
        }
    }
}

impl Element for CursorElement {
    fn id(&self) -> &smithay::backend::renderer::element::Id { &self.id }
    fn current_commit(&self) -> smithay::backend::renderer::utils::CommitCounter { self.commit }
    fn src(&self) -> Rectangle<f64, Buffer> { Rectangle::from_size(Size::<i32, Buffer>::from((self.size.w, self.size.h))).to_f64() }
    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> { Rectangle::new(self.location, self.size) }
}

impl RenderElement<GlesRenderer> for CursorElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
        _cache: Option<&smithay::utils::user_data::UserDataMap>,
    ) -> Result<(), smithay::backend::renderer::gles::GlesError> {
        frame.render_texture_from_to(&self.texture, src, dst, damage, &[], Transform::Normal, 1.0, None, &[])
    }
}

#[derive(Debug)]
pub struct CornerMaskElement {
    id: smithay::backend::renderer::element::Id,
    commit: smithay::backend::renderer::utils::CommitCounter,
    texture: GlesTexture,
    src: Rectangle<f64, Buffer>,
    dest: Rectangle<i32, Physical>,
    radius: f32,
    dest_f: (f32, f32),
    program: GlesTexProgram,
}

impl CornerMaskElement {
    pub fn new(
        program: GlesTexProgram,
        texture: GlesTexture,
        dest: Rectangle<i32, Physical>,
        radius: f32,
        wallpaper_size: Size<i32, Buffer>,
    ) -> Self {
        let src = Rectangle::<i32, Buffer>::new(
            Point::from((dest.loc.x, dest.loc.y)),
            Size::from((dest.size.w, dest.size.h)),
        );
        let _ = wallpaper_size;
        Self {
            id: smithay::backend::renderer::element::Id::new(),
            commit: smithay::backend::renderer::utils::CommitCounter::default(),
            texture,
            src: src.to_f64(),
            dest,
            radius,
            dest_f: (dest.loc.x as f32, dest.loc.y as f32),
            program,
        }
    }
}

impl Element for CornerMaskElement {
    fn id(&self) -> &smithay::backend::renderer::element::Id { &self.id }
    fn current_commit(&self) -> smithay::backend::renderer::utils::CommitCounter { self.commit }
    fn src(&self) -> Rectangle<f64, Buffer> { self.src }
    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> { self.dest }
}

impl RenderElement<GlesRenderer> for CornerMaskElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
        _cache: Option<&smithay::utils::user_data::UserDataMap>,
    ) -> Result<(), smithay::backend::renderer::gles::GlesError> {
        let uniforms = [
            Uniform::new("u_size", smithay::backend::renderer::gles::UniformValue::_2f(dst.size.w as f32, dst.size.h as f32)),
            Uniform::new("u_radius", smithay::backend::renderer::gles::UniformValue::_1f(self.radius)),
            Uniform::new("u_dest", smithay::backend::renderer::gles::UniformValue::_2f(self.dest_f.0, self.dest_f.1)),
        ];
        frame.render_texture_from_to(&self.texture, src, dst, damage, &[], Transform::Normal, 1.0, Some(&self.program), &uniforms)
    }
}

pub enum OutputElement {
    Wallpaper(WallpaperElement),
    Space(smithay::desktop::space::SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>),
    Titlebar(TitlebarElement),
    CornerMask(CornerMaskElement),
    Cursor(CursorElement),
}

impl std::fmt::Debug for OutputElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wallpaper(v) => f.debug_tuple("Wallpaper").field(v).finish(),
            Self::Space(v) => f.debug_tuple("Space").field(v).finish(),
            Self::Titlebar(v) => f.debug_tuple("Titlebar").field(v).finish(),
            Self::CornerMask(v) => f.debug_tuple("CornerMask").field(v).finish(),
            Self::Cursor(v) => f.debug_tuple("Cursor").field(v).finish(),
        }
    }
}

impl Element for OutputElement {
    fn id(&self) -> &smithay::backend::renderer::element::Id {
        match self {
            Self::Wallpaper(v) => v.id(),
            Self::Space(v) => v.id(),
            Self::Titlebar(v) => v.id(),
            Self::CornerMask(v) => v.id(),
            Self::Cursor(v) => v.id(),
        }
    }
    fn current_commit(&self) -> smithay::backend::renderer::utils::CommitCounter {
        match self {
            Self::Wallpaper(v) => v.current_commit(),
            Self::Space(v) => v.current_commit(),
            Self::Titlebar(v) => v.current_commit(),
            Self::CornerMask(v) => v.current_commit(),
            Self::Cursor(v) => v.current_commit(),
        }
    }
    fn src(&self) -> Rectangle<f64, Buffer> {
        match self {
            Self::Wallpaper(v) => v.src(),
            Self::Space(v) => v.src(),
            Self::Titlebar(v) => v.src(),
            Self::CornerMask(v) => v.src(),
            Self::Cursor(v) => v.src(),
        }
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        match self {
            Self::Wallpaper(v) => v.geometry(scale),
            Self::Space(v) => v.geometry(scale),
            Self::Titlebar(v) => v.geometry(scale),
            Self::CornerMask(v) => v.geometry(scale),
            Self::Cursor(v) => v.geometry(scale),
        }
    }
    fn alpha(&self) -> f32 {
        match self {
            Self::Wallpaper(v) => v.alpha(),
            Self::Space(v) => v.alpha(),
            Self::Titlebar(v) => v.alpha(),
            Self::CornerMask(v) => v.alpha(),
            Self::Cursor(v) => v.alpha(),
        }
    }
    fn kind(&self) -> Kind {
        match self {
            Self::Wallpaper(v) => v.kind(),
            Self::Space(v) => v.kind(),
            Self::Titlebar(v) => v.kind(),
            Self::CornerMask(v) => v.kind(),
            Self::Cursor(v) => v.kind(),
        }
    }
    fn opaque_regions(&self, scale: Scale<f64>) -> smithay::backend::renderer::utils::OpaqueRegions<i32, Physical> {
        match self {
            Self::Wallpaper(v) => v.opaque_regions(scale),
            Self::Space(v) => v.opaque_regions(scale),
            Self::Titlebar(v) => v.opaque_regions(scale),
            Self::CornerMask(v) => v.opaque_regions(scale),
            Self::Cursor(v) => v.opaque_regions(scale),
        }
    }
}

impl RenderElement<GlesRenderer> for OutputElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&smithay::utils::user_data::UserDataMap>,
    ) -> Result<(), smithay::backend::renderer::gles::GlesError> {
        match self {
            Self::Wallpaper(v) => v.draw(frame, src, dst, damage, opaque_regions, cache),
            Self::Space(v) => v.draw(frame, src, dst, damage, opaque_regions, cache),
            Self::Titlebar(v) => v.draw(frame, src, dst, damage, opaque_regions, cache),
            Self::CornerMask(v) => v.draw(frame, src, dst, damage, opaque_regions, cache),
            Self::Cursor(v) => v.draw(frame, src, dst, damage, opaque_regions, cache),
        }
    }
}

pub fn corner_patch(
    wallpaper: &WallpaperCache,
    origin_x: i32,
    origin_y: i32,
    radius: i32,
    corner: usize,
) -> Option<Vec<u8>> {
    if radius <= 0 {
        return None;
    }
    let r = radius;
    let w = wallpaper.size.w;
    let h = wallpaper.size.h;
    if w <= 0 || h <= 0 {
        return None;
    }
    let axis = match corner {
        0 => (r as f32, r as f32),
        1 => (0.0, r as f32),
        2 => (r as f32, 0.0),
        _ => (0.0, 0.0),
    };
    let mut out = vec![0u8; (r * r * 4) as usize];
    for py in 0..r {
        for px in 0..r {
            let sx = (origin_x + px).clamp(0, w - 1);
            let sy = (origin_y + py).clamp(0, h - 1);
            let src = ((sy * w + sx) * 4) as usize;
            let dst = ((py * r + px) * 4) as usize;
            if src + 3 >= wallpaper.pixels.len() || dst + 3 >= out.len() {
                continue;
            }
            let px_center = px as f32 + 0.5;
            let py_center = py as f32 + 0.5;
            let dx = px_center - axis.0;
            let dy = py_center - axis.1;
            let distance = (dx * dx + dy * dy).sqrt();
            let coverage = ((distance - (r as f32 - 1.2)) / 1.2).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            let alpha = (coverage * 255.0) as u8;
            out[dst] = wallpaper.pixels[src + 2];
            out[dst + 1] = wallpaper.pixels[src + 1];
            out[dst + 2] = wallpaper.pixels[src];
            out[dst + 3] = alpha;
        }
    }
    Some(out)
}
