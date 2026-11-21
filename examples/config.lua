kyde.decor({
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
