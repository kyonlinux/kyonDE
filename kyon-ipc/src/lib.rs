use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "kyonDE";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn config_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(dir).join(APP_NAME)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config").join(APP_NAME)
    } else {
        PathBuf::from(".config").join(APP_NAME)
    }
}

pub fn config_path() -> PathBuf {
    config_home().join("config.lua")
}

pub fn state_path() -> PathBuf {
    config_home().join("state.json")
}

pub fn shell_path() -> PathBuf {
    config_home().join("shell.json")
}

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

pub fn socket_path() -> PathBuf {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    runtime_dir().join(format!("kyonde-{user}.sock"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct KyonConfig {
    pub decor: Decor,
    pub blur: Blur,
    pub animations: Animations,
    pub layout: Layout,
    pub input: Input,
    pub shell: Shell,
    pub wallpaper: Wallpaper,
    pub binds: Vec<KeyBind>,
    pub exec: Vec<String>,
    pub outputs: Vec<OutputRule>,
    pub rules: Vec<WindowRule>,
    pub env: Vec<EnvVar>,
    pub gpu: Gpu,
}

impl Default for KyonConfig {
    fn default() -> Self {
        Self {
            decor: Decor::default(),
            blur: Blur::default(),
            animations: Animations::default(),
            layout: Layout::default(),
            input: Input::default(),
            shell: Shell::default(),
            wallpaper: Wallpaper::default(),
            binds: default_binds(),
            exec: Vec::new(),
            outputs: Vec::new(),
            rules: Vec::new(),
            env: Vec::new(),
            gpu: Gpu::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Gpu {
    pub backend: String,
    pub prefer_nvidia: bool,
    pub allow_software: bool,
    pub drm_devices: String,
    pub render_node: String,
    pub max_fps: u32,
    pub damage_tracking: bool,
}

impl Default for Gpu {
    fn default() -> Self {
        Self {
            backend: "auto".into(),
            prefer_nvidia: true,
            allow_software: false,
            drm_devices: String::new(),
            render_node: String::new(),
            max_fps: 0,
            damage_tracking: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Decor {
    pub rounding: i32,
    pub gaps_in: i32,
    pub gaps_out: i32,
    pub border_width: i32,
    pub active_border: String,
    pub inactive_border: String,
    pub active_opacity: f32,
    pub inactive_opacity: f32,
    pub dim_inactive: bool,
    pub dim_strength: f32,
    pub shadow: Shadow,
    pub titlebar: Titlebar,
}

impl Default for Decor {
    fn default() -> Self {
        Self {
            rounding: 12,
            gaps_in: 8,
            gaps_out: 16,
            border_width: 1,
            active_border: "#3daee9".into(),
            inactive_border: "#2c3140".into(),
            active_opacity: 1.0,
            inactive_opacity: 0.97,
            dim_inactive: false,
            dim_strength: 0.15,
            shadow: Shadow::default(),
            titlebar: Titlebar::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Shadow {
    pub enabled: bool,
    pub size: i32,
    pub opacity: f32,
    pub color: String,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            enabled: true,
            size: 22,
            opacity: 0.42,
            color: "#000000".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Titlebar {
    pub enabled: bool,
    pub tiled: bool,
    pub height: i32,
    pub font_size: f32,
    pub opacity: f32,
    pub buttons: String,
    pub button_style: String,
    pub title_align: String,
    pub show_app_icon: bool,
}

impl Default for Titlebar {
    fn default() -> Self {
        Self {
            enabled: true,
            tiled: false,
            height: 34,
            font_size: 13.0,
            opacity: 0.96,
            buttons: "close,maximize,minimize".into(),
            button_style: "circle".into(),
            title_align: "center".into(),
            show_app_icon: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Blur {
    pub enabled: bool,
    pub size: f32,
    pub passes: u32,
    pub opacity: f32,
    pub noise: f32,
    pub wallpaper: bool,
}

impl Default for Blur {
    fn default() -> Self {
        Self {
            enabled: true,
            size: 6.0,
            passes: 2,
            opacity: 0.72,
            noise: 0.0,
            wallpaper: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Animations {
    pub enabled: bool,
    pub leaf: String,
    pub speed: f32,
    pub bezier: Bezier,
    pub workspaces: bool,
    pub borders: bool,
}

impl Default for Animations {
    fn default() -> Self {
        Self {
            enabled: true,
            leaf: "windowsIn".into(),
            speed: 5.0,
            bezier: Bezier::Named("default".into()),
            workspaces: true,
            borders: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Bezier {
    Named(String),
    Custom([f32; 4]),
}

impl Default for Bezier {
    fn default() -> Self {
        Self::Named("default".into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Layout {
    pub mode: String,
    pub tiling: String,
    pub master_ratio: f32,
    pub scrolling: Scrolling,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            mode: "tiling".into(),
            tiling: "master".into(),
            master_ratio: 0.56,
            scrolling: Scrolling::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Scrolling {
    pub column_width: f32,
    pub center_focus: bool,
    pub stack_vertical: bool,
    pub scroll_speed: f32,
}

impl Default for Scrolling {
    fn default() -> Self {
        Self {
            column_width: 0.52,
            center_focus: true,
            stack_vertical: true,
            scroll_speed: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Input {
    pub keyboard_layout: String,
    pub keyboard_variant: String,
    pub repeat_rate: i32,
    pub repeat_delay: i32,
    pub natural_scroll: bool,
    pub tap_to_click: bool,
    pub accel_speed: f32,
    pub accel_profile: String,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            keyboard_layout: "us".into(),
            keyboard_variant: String::new(),
            repeat_rate: 40,
            repeat_delay: 300,
            natural_scroll: true,
            tap_to_click: true,
            accel_speed: 0.0,
            accel_profile: "adaptive".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Shell {
    pub theme: String,
    pub dark: bool,
    pub accent: String,
    pub font: String,
    pub panel_position: String,
    pub panel_height: i32,
    pub panel_floating: bool,
    pub panel_opacity: f32,
    pub clock_seconds: bool,
    pub workspaces: usize,
    pub launcher_style: String,
    pub pinned: Vec<String>,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            theme: "breeze".into(),
            dark: true,
            accent: "#3daee9".into(),
            font: "Inter 10".into(),
            panel_position: "top".into(),
            panel_height: 38,
            panel_floating: false,
            panel_opacity: 0.82,
            clock_seconds: false,
            workspaces: 5,
            launcher_style: "grid".into(),
            pinned: vec![
                "@terminal".into(),
                "firefox".into(),
                "org.gnome.Nautilus".into(),
                "@settings".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Wallpaper {
    pub style: String,
    pub path: String,
    pub color: String,
    pub gradient: Vec<String>,
    pub angle: f32,
}

impl Default for Wallpaper {
    fn default() -> Self {
        Self {
            style: "gradient".into(),
            path: String::new(),
            color: "#0b0e16".into(),
            gradient: vec!["#0a0f1e".into(), "#172033".into(), "#2b1f3d".into()],
            angle: 135.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct KeyBind {
    pub mods: String,
    pub key: String,
    pub action: String,
}

impl Default for KeyBind {
    fn default() -> Self {
        Self {
            mods: String::new(),
            key: String::new(),
            action: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OutputRule {
    pub name: String,
    pub scale: f64,
    pub mode: String,
    pub position_x: i32,
    pub position_y: i32,
    pub transform: String,
    pub enabled: bool,
    pub primary: bool,
}

impl Default for OutputRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            scale: 1.0,
            mode: String::new(),
            position_x: 0,
            position_y: 0,
            transform: String::new(),
            enabled: true,
            primary: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct DisplayMode {
    pub width: i32,
    pub height: i32,
    pub refresh: i32,
    pub preferred: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DisplayInfo {
    pub name: String,
    pub description: String,
    pub width: i32,
    pub height: i32,
    pub refresh: i32,
    pub scale: f64,
    pub x: i32,
    pub y: i32,
    pub enabled: bool,
    pub primary: bool,
    pub modes: Vec<DisplayMode>,
}

impl Default for DisplayInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            width: 0,
            height: 0,
            refresh: 60,
            scale: 1.0,
            x: 0,
            y: 0,
            enabled: true,
            primary: false,
            modes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WindowRule {
    pub app_id: String,
    pub title: String,
    pub floating: Option<bool>,
    pub blur: Option<bool>,
    pub opacity: Option<f32>,
    pub decorations: Option<bool>,
    pub workspace: Option<usize>,
}

impl Default for WindowRule {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            title: String::new(),
            floating: None,
            blur: None,
            opacity: None,
            decorations: None,
            workspace: None,
        }
    }
}

pub fn default_binds() -> Vec<KeyBind> {
    let binds: &[(&str, &str, &str)] = &[
        ("SUPER", "Return", "spawn-terminal"),
        ("SUPER", "Q", "kill"),
        ("SUPER", "F", "layout floating"),
        ("SUPER", "T", "layout tiling"),
        ("SUPER", "S", "layout scrolling"),
        ("SUPER", "Space", "layout cycle"),
        ("SUPER", "V", "toggle-floating"),
        ("SUPER", "M", "fullscreen"),
        ("SUPER+SHIFT", "M", "maximize"),
        ("SUPER", "D", "spawn kyonshell launcher"),
        ("SUPER", "Escape", "spawn kyonshell settings"),
        ("SUPER", "Tab", "focus next"),
        ("SUPER", "Left", "focus left"),
        ("SUPER", "Right", "focus right"),
        ("SUPER", "Up", "focus up"),
        ("SUPER", "Down", "focus down"),
        ("SUPER+SHIFT", "Left", "move left"),
        ("SUPER+SHIFT", "Right", "move right"),
        ("SUPER+SHIFT", "Up", "move up"),
        ("SUPER+SHIFT", "Down", "move down"),
        ("SUPER", "Minus", "scroll left"),
        ("SUPER", "Equal", "scroll right"),
        ("SUPER", "Comma", "workspace prev"),
        ("SUPER", "Period", "workspace next"),
        ("SUPER+SHIFT", "Comma", "move-output prev"),
        ("SUPER+SHIFT", "Period", "move-output next"),
        ("SUPER+SHIFT", "Comma", "move-workspace prev"),
        ("SUPER+SHIFT", "Period", "move-workspace next"),
        ("SUPER+CTRL", "R", "reload"),
        ("SUPER+CTRL", "B", "toggle-blur"),
        ("SUPER+CTRL", "A", "toggle-animations"),
        ("SUPER+SHIFT", "E", "quit"),
    ];
    let mut out = Vec::new();
    for (mods, key, action) in binds {
        out.push(KeyBind {
            mods: (*mods).into(),
            key: (*key).into(),
            action: (*action).into(),
        });
    }
    for i in 1..=9 {
        let action = format!("workspace {i}");
        out.push(KeyBind {
            mods: "SUPER".into(),
            key: i.to_string(),
            action,
        });
        out.push(KeyBind {
            mods: "SUPER+SHIFT".into(),
            key: i.to_string(),
            action: format!("move-workspace {i}"),
        });
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub cmd: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

impl Request {
    pub fn new(cmd: impl Into<String>) -> Self {
        Self {
            cmd: cmd.into(),
            args: serde_json::Value::Null,
        }
    }

    pub fn with_args(cmd: impl Into<String>, args: impl Serialize) -> Self {
        Self {
            cmd: cmd.into(),
            args: serde_json::to_value(args).unwrap_or(serde_json::Value::Null),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<Snapshot>,
}

impl Response {
    pub fn ok(state: Snapshot) -> Self {
        Self {
            ok: true,
            error: None,
            state: Some(state),
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            state: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Snapshot {
    pub version: String,
    pub layout_mode: String,
    pub active_workspace: usize,
    pub workspace_count: usize,
    pub windows: usize,
    pub focused_title: Option<String>,
    pub output_width: i32,
    pub output_height: i32,
    pub gpu_backend: String,
    #[serde(default)]
    pub displays: Vec<DisplayInfo>,
    pub config: KyonConfig,
}

pub fn default_config_lua() -> &'static str {
    r##"kyde.decor({
    rounding = 12,
    gaps_in = 8,
    gaps_out = 16,
    border_width = 1,
    active_border = "#3daee9",
    inactive_border = "#2c3140",
    dim_inactive = false,
    shadow = { enabled = true, size = 22, opacity = 0.42 },
    titlebar = {
        enabled = true,
        tiled = false,
        height = 34,
        font_size = 13,
        buttons = "close,maximize,minimize",
        button_style = "circle",
        title_align = "center",
        opacity = 0.96,
    },
})

kyde.blur({ enabled = true, size = 6, passes = 2, opacity = 0.72, wallpaper = true })

kyde.animations({
    enabled = true,
    leaf = "windowsIn",
    speed = 5,
    bezier = "default",
    workspaces = true,
    borders = true,
})

kyde.layout({
    mode = "tiling",
    tiling = "master",
    master_ratio = 0.56,
    scrolling = { column_width = 0.52, center_focus = true, stack_vertical = true, scroll_speed = 1.0 },
})

kyde.input({
    keyboard_layout = "us",
    keyboard_variant = "",
    repeat_rate = 40,
    repeat_delay = 300,
    natural_scroll = true,
    tap_to_click = true,
})

kyde.shell({
    theme = "breeze",
    dark = true,
    accent = "#3daee9",
    font = "Inter 10",
    panel_position = "top",
    panel_height = 38,
    panel_floating = false,
    panel_opacity = 0.82,
    clock_seconds = false,
    workspaces = 5,
    launcher_style = "grid",
    pinned = { "@terminal", "firefox", "org.gnome.Nautilus", "@settings" },
})

kyde.gpu({
    backend = "auto",
    prefer_nvidia = true,
    allow_software = false,
    drm_devices = "",
    render_node = "",
    max_fps = 0,
    damage_tracking = true,
})

kyde.wallpaper({
    style = "gradient",
    color = "#0b0e16",
    gradient = { "#0a0f1e", "#172033", "#2b1f3d" },
    angle = 135,
})

kyde.bind("SUPER", "Return", "spawn-terminal")
kyde.bind("SUPER", "Q", "kill")
kyde.bind("SUPER", "F", "layout floating")
kyde.bind("SUPER", "T", "layout tiling")
kyde.bind("SUPER", "S", "layout scrolling")
kyde.bind("SUPER", "Space", "layout cycle")
kyde.bind("SUPER", "V", "toggle-floating")
kyde.bind("SUPER", "M", "fullscreen")
kyde.bind("SUPER+SHIFT", "M", "maximize")
kyde.bind("SUPER", "D", "spawn kyonshell launcher")
kyde.bind("SUPER", "Escape", "spawn kyonshell settings")
kyde.bind("SUPER", "Tab", "focus next")
kyde.bind("SUPER", "Left", "focus left")
kyde.bind("SUPER", "Right", "focus right")
kyde.bind("SUPER", "Up", "focus up")
kyde.bind("SUPER", "Down", "focus down")
kyde.bind("SUPER+SHIFT", "Left", "move left")
kyde.bind("SUPER+SHIFT", "Right", "move right")
kyde.bind("SUPER+SHIFT", "Up", "move up")
kyde.bind("SUPER+SHIFT", "Down", "move down")
kyde.bind("SUPER", "Minus", "scroll left")
kyde.bind("SUPER", "Equal", "scroll right")
kyde.bind("SUPER", "Comma", "workspace prev")
kyde.bind("SUPER", "Period", "workspace next")
kyde.bind("SUPER+SHIFT", "Comma", "move-output prev")
kyde.bind("SUPER+SHIFT", "Period", "move-output next")
kyde.bind("SUPER", "1", "workspace 1")
kyde.bind("SUPER", "2", "workspace 2")
kyde.bind("SUPER", "3", "workspace 3")
kyde.bind("SUPER", "4", "workspace 4")
kyde.bind("SUPER", "5", "workspace 5")
kyde.bind("SUPER+SHIFT", "1", "move-workspace 1")
kyde.bind("SUPER+SHIFT", "2", "move-workspace 2")
kyde.bind("SUPER+SHIFT", "3", "move-workspace 3")
kyde.bind("SUPER+SHIFT", "4", "move-workspace 4")
kyde.bind("SUPER+SHIFT", "5", "move-workspace 5")
kyde.bind("SUPER+CTRL", "R", "reload")
kyde.bind("SUPER+CTRL", "B", "toggle-blur")
kyde.bind("SUPER+CTRL", "A", "toggle-animations")

kyde.exec("kyonshell session")
"##
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub bg: &'static str,
    pub surface: &'static str,
    pub surface_alt: &'static str,
    pub surface_high: &'static str,
    pub text: &'static str,
    pub text_muted: &'static str,
    pub border: &'static str,
    pub accent: &'static str,
    pub danger: &'static str,
    pub success: &'static str,
    pub warning: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub font: &'static str,
    pub rounding: i32,
    pub titlebar_style: &'static str,
    pub dark: Palette,
    pub light: Palette,
}

pub const THEMES: &[Theme] = &[
    Theme {
        id: "breeze",
        name: "Kyon Breeze",
        description: "Familiar KDE Plasma inspired workspace",
        font: "Inter 10",
        rounding: 8,
        titlebar_style: "breeze",
        dark: Palette {
            bg: "#12141b",
            surface: "#1b1e27",
            surface_alt: "#242834",
            surface_high: "#2e3342",
            text: "#f2f4f8",
            text_muted: "#9aa3b5",
            border: "#343a4a",
            accent: "#3daee9",
            danger: "#ed5b56",
            success: "#27c06b",
            warning: "#f5b74f",
        },
        light: Palette {
            bg: "#eff2f7",
            surface: "#ffffff",
            surface_alt: "#f4f6fa",
            surface_high: "#e7ebf2",
            text: "#1b1e27",
            text_muted: "#5c6475",
            border: "#d6dbe5",
            accent: "#1d99f3",
            danger: "#da4453",
            success: "#1f9d55",
            warning: "#c98a1b",
        },
    },
    Theme {
        id: "windows",
        name: "Windows 11",
        description: "Fluent surfaces and a centered task bar",
        font: "Inter 10",
        rounding: 10,
        titlebar_style: "windows",
        dark: Palette {
            bg: "#101114",
            surface: "#1c1d22",
            surface_alt: "#25262c",
            surface_high: "#303138",
            text: "#f5f6f8",
            text_muted: "#a2a6af",
            border: "#34363d",
            accent: "#0078d4",
            danger: "#e81123",
            success: "#6ccb5f",
            warning: "#fce100",
        },
        light: Palette {
            bg: "#f3f3f3",
            surface: "#fbfbfb",
            surface_alt: "#f1f1f1",
            surface_high: "#e8e8e8",
            text: "#1b1b1b",
            text_muted: "#606060",
            border: "#d7d7d7",
            accent: "#0067c0",
            danger: "#c42b1c",
            success: "#0f7b0f",
            warning: "#9d5d00",
        },
    },
    Theme {
        id: "adwaita",
        name: "GNOME Adwaita",
        description: "Clean, spacious and touch friendly",
        font: "Inter 10",
        rounding: 12,
        titlebar_style: "adwaita",
        dark: Palette {
            bg: "#1d1d20",
            surface: "#242426",
            surface_alt: "#303033",
            surface_high: "#3a3a3e",
            text: "#ffffff",
            text_muted: "#b5b5ba",
            border: "#3d3d42",
            accent: "#3584e4",
            danger: "#e01b24",
            success: "#2ec27e",
            warning: "#e5a50a",
        },
        light: Palette {
            bg: "#f6f6f6",
            surface: "#ffffff",
            surface_alt: "#f1f1f1",
            surface_high: "#e7e7e7",
            text: "#1e1e1e",
            text_muted: "#6b6b70",
            border: "#dedede",
            accent: "#3584e4",
            danger: "#c01c28",
            success: "#26a269",
            warning: "#b58300",
        },
    },
    Theme {
        id: "cosmic",
        name: "COSMIC",
        description: "Floating panels with a vivid violet accent",
        font: "Inter 10",
        rounding: 14,
        titlebar_style: "cosmic",
        dark: Palette {
            bg: "#16161b",
            surface: "#212128",
            surface_alt: "#2b2b34",
            surface_high: "#363641",
            text: "#f5f5fa",
            text_muted: "#a8a8b8",
            border: "#3a3a47",
            accent: "#8a6cff",
            danger: "#ff5c5c",
            success: "#54d18b",
            warning: "#ffc857",
        },
        light: Palette {
            bg: "#f2f1f7",
            surface: "#ffffff",
            surface_alt: "#f5f4fa",
            surface_high: "#e9e7f2",
            text: "#1d1b26",
            text_muted: "#625e73",
            border: "#dcd9e7",
            accent: "#6a4ef5",
            danger: "#d93434",
            success: "#189a5a",
            warning: "#a76b00",
        },
    },
    Theme {
        id: "graphite",
        name: "Graphite",
        description: "A compact, neutral dark theme",
        font: "Inter 10",
        rounding: 6,
        titlebar_style: "graphite",
        dark: Palette {
            bg: "#0e0f11",
            surface: "#17181b",
            surface_alt: "#202226",
            surface_high: "#2a2d32",
            text: "#e8eaee",
            text_muted: "#8f96a3",
            border: "#2e3238",
            accent: "#8ab4f8",
            danger: "#f07178",
            success: "#7ec699",
            warning: "#e6c07b",
        },
        light: Palette {
            bg: "#eceef1",
            surface: "#ffffff",
            surface_alt: "#f3f5f7",
            surface_high: "#e5e8ec",
            text: "#1c1f24",
            text_muted: "#5d6573",
            border: "#d3d8e0",
            accent: "#2f6fd0",
            danger: "#c94f55",
            success: "#2f8a5f",
            warning: "#9a7418",
        },
    },
];

pub fn theme(id: &str) -> &'static Theme {
    THEMES.iter().find(|t| t.id == id).unwrap_or(&THEMES[0])
}

impl Theme {
    pub fn palette(&self, dark: bool) -> Palette {
        if dark { self.dark } else { self.light }
    }
}

pub fn parse_color(value: &str) -> Option<[f32; 4]> {
    let value = value.trim();
    let hex = value.strip_prefix('#')?;
    let (r, g, b, a) = match hex.len() {
        6 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            255,
        ),
        8 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            u8::from_str_radix(&hex[6..8], 16).ok()?,
        ),
        _ => return None,
    };
    Some([
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ])
}
