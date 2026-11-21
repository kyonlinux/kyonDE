use std::time::{Duration, Instant};

use kyon_ipc::{Bezier, parse_color};

#[derive(Debug, Clone, Copy)]
pub struct Easing {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl Easing {
    pub fn from_spec(spec: &Bezier) -> Self {
        match spec {
            Bezier::Custom(points) => Self {
                x1: points[0],
                y1: points[1],
                x2: points[2],
                y2: points[3],
            },
            Bezier::Named(name) => match name.as_str() {
                "linear" => Self { x1: 0.0, y1: 0.0, x2: 1.0, y2: 1.0 },
                "easeIn" | "ease-in" => Self { x1: 0.42, y1: 0.0, x2: 1.0, y2: 1.0 },
                "easeOut" | "ease-out" => Self { x1: 0.0, y1: 0.0, x2: 0.58, y2: 1.0 },
                "easeInOut" | "ease-in-out" => Self { x1: 0.42, y1: 0.0, x2: 0.58, y2: 1.0 },
                "snappy" => Self { x1: 0.2, y1: 1.0, x2: 0.3, y2: 1.0 },
                "overshoot" => Self { x1: 0.34, y1: 1.56, x2: 0.64, y2: 1.0 },
                _ => Self { x1: 0.2, y1: 0.9, x2: 0.3, y2: 1.0 },
            },
        }
    }

    pub fn sample(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if (self.x1 - self.y1).abs() < f32::EPSILON && (self.x2 - self.y2).abs() < f32::EPSILON {
            return t;
        }
        let mut guess = t;
        for _ in 0..6 {
            let x = bezier(self.x1, self.x2, guess) - t;
            let dx = bezier_derivative(self.x1, self.x2, guess);
            if dx.abs() < 1e-5 {
                break;
            }
            guess = (guess - x / dx).clamp(0.0, 1.0);
        }
        bezier(self.y1, self.y2, guess)
    }
}

fn bezier(a: f32, b: f32, t: f32) -> f32 {
    let mt = 1.0 - t;
    3.0 * mt * mt * t * a + 3.0 * mt * t * t * b + t * t * t
}

fn bezier_derivative(a: f32, b: f32, t: f32) -> f32 {
    let mt = 1.0 - t;
    3.0 * mt * mt * a + 6.0 * mt * t * (b - a) + 3.0 * t * t * (1.0 - b)
}

#[derive(Debug, Clone)]
pub struct Timed {
    pub from: f32,
    pub to: f32,
    pub start: Instant,
    pub duration: Duration,
    pub easing: Easing,
}

impl Timed {
    pub fn new(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            start: Instant::now(),
            duration: Duration::ZERO,
            easing: Easing::from_spec(&Bezier::default()),
        }
    }

    pub fn retarget(&mut self, to: f32, duration: Duration, easing: Easing, now: Instant) {
        let current = self.sample(now);
        if (current - to).abs() < 0.0001 {
            self.from = to;
            self.to = to;
            self.start = now;
            self.duration = Duration::ZERO;
            return;
        }
        self.from = current;
        self.to = to;
        self.start = now;
        self.duration = duration;
        self.easing = easing;
    }

    pub fn snap(&mut self, to: f32) {
        self.from = to;
        self.to = to;
        self.start = Instant::now();
        self.duration = Duration::ZERO;
    }

    pub fn sample(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return self.to;
        }
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32();
        let duration = self.duration.as_secs_f32();
        if elapsed >= duration {
            self.to
        } else {
            let t = self.easing.sample(elapsed / duration);
            self.from + (self.to - self.from) * t
        }
    }

    pub fn done(&self, now: Instant) -> bool {
        self.duration.is_zero() || now.saturating_duration_since(self.start) >= self.duration
    }
}

pub fn color(value: &str, fallback: [f32; 4]) -> [f32; 4] {
    parse_color(value).unwrap_or(fallback)
}

pub fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

pub fn alpha(color: [f32; 4], factor: f32) -> [f32; 4] {
    [color[0], color[1], color[2], color[3] * factor]
}

pub fn with_alpha(color: [f32; 4], a: f32) -> [f32; 4] {
    [color[0], color[1], color[2], a]
}

pub fn luminance(color: [f32; 4]) -> f32 {
    0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2]
}

pub fn screen_rect(size: (i32, i32)) -> smithay::utils::Rectangle<i32, smithay::utils::Logical> {
    smithay::utils::Rectangle::from_size((size.0, size.1).into())
}
