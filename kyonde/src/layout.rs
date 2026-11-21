use std::str::FromStr;

use smithay::utils::{Logical, Rectangle, Size};

use kyon_ipc::{Decor, Layout as LayoutConfig, Scrolling};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Tiling,
    Scrolling,
    Floating,
}

impl LayoutMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tiling => "tiling",
            Self::Scrolling => "scrolling",
            Self::Floating => "floating",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Tiling => Self::Scrolling,
            Self::Scrolling => Self::Floating,
            Self::Floating => Self::Tiling,
        }
    }
}

impl FromStr for LayoutMode {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "tiling" | "tile" | "dwindle" | "master" => Ok(Self::Tiling),
            "scrolling" | "scroll" | "paper" | "niri" => Ok(Self::Scrolling),
            "floating" | "float" => Ok(Self::Floating),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LayoutItem {
    pub floating: bool,
    pub fullscreen: bool,
    pub maximized: bool,
    pub saved: Rectangle<i32, Logical>,
    pub content_height: i32,
}

#[derive(Debug, Clone, Default)]
pub struct LayoutOutput {
    pub rects: Vec<Option<Rectangle<i32, Logical>>>,
    pub scroll_offset: f64,
}

pub fn compute_layout(
    mode: LayoutMode,
    config: &LayoutConfig,
    decor: &Decor,
    area: Rectangle<i32, Logical>,
    items: &[LayoutItem],
    focused_index: Option<usize>,
    current_scroll: f64,
) -> LayoutOutput {
    let mut output = LayoutOutput {
        rects: vec![None; items.len()],
        scroll_offset: current_scroll,
    };

    match mode {
        LayoutMode::Floating => {
            let mut cascade = 0;
            for (idx, item) in items.iter().enumerate() {
                if item.maximized {
                    output.rects[idx] = Some(area);
                    continue;
                }
                let mut rect = item.saved;
                let max_w = area.size.w.saturating_sub(decor.gaps_out * 2);
                let max_h = area.size.h.saturating_sub(decor.gaps_out * 2);
                if rect.size.w > max_w || rect.size.h > max_h || rect.size.w < 120 || rect.size.h < 80 {
                    let w = ((area.size.w as f64) * 0.62) as i32;
                    let h = ((area.size.h as f64) * 0.68) as i32;
                    let offset = cascade * 34;
                    rect = Rectangle::new(
                        (
                            area.loc.x + decor.gaps_out + 40 + offset,
                            area.loc.y + decor.gaps_out + 30 + offset,
                        )
                            .into(),
                        Size::from((w.max(240), h.max(160))),
                    );
                }
                if item.fullscreen {
                    output.rects[idx] = Some(area);
                } else {
                    output.rects[idx] = Some(rect);
                }
                cascade = (cascade + 1) % 8;
            }
        }
        LayoutMode::Tiling => {
            let tiled: Vec<usize> = items
                .iter()
                .enumerate()
                .filter(|(_, item)| !item.floating && !item.fullscreen && !item.maximized)
                .map(|(idx, _)| idx)
                .collect();
            for (idx, item) in items.iter().enumerate() {
                if item.floating {
                    output.rects[idx] = Some(item.saved);
                }
            }
            if tiled.is_empty() {
                return output;
            }
            let usable = deflate(area, decor.gaps_out);
            if config.tiling.eq_ignore_ascii_case("dwindle") {
                dwindle(&tiled, items, decor.gaps_in, usable, &mut output.rects);
            } else {
                master(&tiled, items, decor.gaps_in, usable, config.master_ratio, &mut output.rects);
            }
        }
        LayoutMode::Scrolling => {
            let tiled: Vec<usize> = items
                .iter()
                .enumerate()
                .filter(|(_, item)| !item.floating && !item.fullscreen && !item.maximized)
                .map(|(idx, _)| idx)
                .collect();
            for (idx, item) in items.iter().enumerate() {
                if item.floating {
                    output.rects[idx] = Some(item.saved);
                }
            }
            if tiled.is_empty() {
                return output;
            }
            output.scroll_offset = scrolling(
                &tiled,
                items,
                decor.gaps_in,
                config.scrolling.clone(),
                deflate(area, decor.gaps_out),
                focused_index,
                &mut output.rects,
            );
        }
    }
    output
}

fn master(
    tiled: &[usize],
    items: &[LayoutItem],
    gap: i32,
    usable: Rectangle<i32, Logical>,
    ratio: f32,
    rects: &mut [Option<Rectangle<i32, Logical>>],
) {
    let n = tiled.len();
    if n == 1 {
        rects[tiled[0]] = Some(usable);
        return;
    }
    let ratio = ratio.clamp(0.2, 0.85);
    let master_w = ((usable.size.w - gap) as f32 * ratio).round() as i32;
    let master_rect = Rectangle::new(
        usable.loc,
        Size::from((master_w.max(80), usable.size.h)),
    );
    rects[tiled[0]] = Some(master_rect);

    let stack_x = usable.loc.x + master_w + gap;
    let stack_w = (usable.size.w - master_w - gap).max(80);
    let stack_count = n - 1;
    let total_gap = gap * (stack_count as i32 - 1);
    let stack_h = (usable.size.h - total_gap) / stack_count as i32;
    let mut y = usable.loc.y;
    for idx in &tiled[1..] {
        let last = *idx == tiled[n - 1];
        let h = if last {
            usable.loc.y + usable.size.h - y
        } else {
            stack_h
        };
        rects[*idx] = Some(Rectangle::new(
            (stack_x, y).into(),
            Size::from((stack_w, h.max(40))),
        ));
        y += h + gap;
    }
    let _ = items;
}

fn dwindle(
    tiled: &[usize],
    items: &[LayoutItem],
    gap: i32,
    usable: Rectangle<i32, Logical>,
    rects: &mut [Option<Rectangle<i32, Logical>>],
) {
    fn rec(
        tiled: &[usize],
        gap: i32,
        area: Rectangle<i32, Logical>,
        depth: usize,
        rects: &mut [Option<Rectangle<i32, Logical>>],
    ) {
        if tiled.is_empty() {
            return;
        }
        if tiled.len() == 1 {
            rects[tiled[0]] = Some(area);
            return;
        }
        let horizontal = depth % 2 == 0;
        if horizontal {
            let first_w = ((area.size.w - gap) as f32 * 0.5).round() as i32;
            let first = Rectangle::new(area.loc, Size::from((first_w.max(60), area.size.h)));
            let second = Rectangle::new(
                (area.loc.x + first_w + gap, area.loc.y).into(),
                Size::from(((area.size.w - first_w - gap).max(60), area.size.h)),
            );
            rects[tiled[0]] = Some(first);
            rec(&tiled[1..], gap, second, depth + 1, rects);
        } else {
            let first_h = ((area.size.h - gap) as f32 * 0.5).round() as i32;
            let first = Rectangle::new(area.loc, Size::from((area.size.w, first_h.max(60))));
            let second = Rectangle::new(
                (area.loc.x, area.loc.y + first_h + gap).into(),
                Size::from((area.size.w, (area.size.h - first_h - gap).max(60))),
            );
            rects[tiled[0]] = Some(first);
            rec(&tiled[1..], gap, second, depth + 1, rects);
        }
    }
    let _ = items;
    rec(tiled, gap, usable, 0, rects);
}

fn scrolling(
    tiled: &[usize],
    items: &[LayoutItem],
    gap: i32,
    config: Scrolling,
    usable: Rectangle<i32, Logical>,
    focused_index: Option<usize>,
    rects: &mut [Option<Rectangle<i32, Logical>>],
) -> f64 {
    let n = tiled.len();
    let col_w = ((usable.size.w as f32 * config.column_width).round() as i32)
        .max(260)
        .min(usable.size.w);
    let stride = col_w + gap;
    let total = stride * n as i32 - gap;
    let focus_pos = focused_index
        .and_then(|focused| tiled.iter().position(|idx| *idx == focused))
        .unwrap_or(0);

    let max_scroll = (total - usable.size.w).max(0) as f64;
    let focus_x = focus_pos as i32 * stride;
    let offset = if config.center_focus {
        (focus_x as f64 + col_w as f64 / 2.0 - usable.size.w as f64 / 2.0).clamp(0.0, max_scroll)
    } else {
        focus_x.min(max_scroll as i32) as f64
    };

    for (pos, idx) in tiled.iter().enumerate() {
        let x = usable.loc.x + pos as i32 * stride - offset.round() as i32;
        rects[*idx] = Some(Rectangle::new(
            (x, usable.loc.y).into(),
            Size::from((col_w, usable.size.h)),
        ));
    }
    let _ = items;
    offset
}

pub fn deflate(rect: Rectangle<i32, Logical>, amount: i32) -> Rectangle<i32, Logical> {
    let amount = amount.max(0);
    Rectangle::new(
        (rect.loc.x + amount, rect.loc.y + amount).into(),
        Size::from((
            (rect.size.w - amount * 2).max(1),
            (rect.size.h - amount * 2).max(1),
        )),
    )
}
