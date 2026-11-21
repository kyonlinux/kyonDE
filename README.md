# KyonDE

A Wayland desktop environment built on [Smithay](https://github.com/Smithay/smithay), with a Lua configured compositor and a GTK4 shell.

The project is split into four crates:

| Crate | Purpose |
| --- | --- |
| `kyonde` | Smithay compositor: layouts, decorations, animations, blur, IPC |
| `kyonshell` | Panel, application launcher and settings app |
| `kyonctl` | Small command line client for the compositor IPC socket |
| `kyon-ipc` | Shared configuration model, IPC protocol and theme presets |

## Current status

`kyonde` ships with two backends:

- **winit**: runs nested inside an existing Wayland or X11 session
- **udev/DRM**: runs directly on a TTY using libseat, libinput, DRM and GBM

The TTY backend opens every usable DRM card and drives all connected outputs, including hybrid Intel/NVIDIA laptops. Outputs on different GPUs are rendered by their own EGL/GLES context. For an explicit ordering, set `kyde.gpu({ drm_devices = "card1:card0" })` or use `kyde.env("AQ_DRM_DEVICES", ...)`. The shell is launched through the compositor socket and is found next to the compositor binary or in `PATH`.

Everything else is functional:

- Tiling layout with master/stack and dwindle algorithms
- Niri style scrolling layout with horizontal columns
- Floating layout with compositor drawn title bars
- Theme-aware title bars: Windows 11 uses left-aligned titles and line controls, Breeze uses centered KDE-style titles, and other themes get their own decoration style
- Title bars are hidden for tiled and scrolling layouts; floating windows get rounded decorations
- Multi-monitor tiling, scrolling and floating layouts with per-output window placement
- Displays settings page with drag-to-arrange monitors, position fields, resolution and refresh selection, scaling, primary output and enable/disable
- Rounded title bars, configurable gaps and borders
- Optional wallpaper backdrop blur
- Animated layout changes and window entrances
- Lua configuration with live reload
- Multiple workspaces, layout switching, window movement and fullscreen
- Unix socket IPC and `kyonctl`
- GTK4 panel with pinned application taskbar, workspaces, clock, layout switcher and session menu
- Desktop icons for the Desktop folder, Home and Trash with free positioning and drag to arrange
- Right-click menus for folders, files and the desktop background: open, open in terminal, rename, move to trash, properties, new folder and wallpaper settings
- Application launcher with search, app grid and power actions
- KDE/Windows style settings app with live theme switching, display arrangement, wallpaper picking and live keyboard layout and repeat settings
- Theme presets: Breeze, Windows 11, GNOME Adwaita, COSMIC and Graphite
- Client-side decorations are supported for applications that request them

## Building

System dependencies for the nested build:

- Rust 1.88 or newer
- EGL, GLESv2, GBM and libdrm development files
- libinput, libudev, libseat and xkbcommon development files (needed by the optional Smithay backends)
- GTK 4.12 or newer and gtk4-layer-shell

```sh
cargo build --release -p kyonde -p kyonshell -p kyonctl
```

For a nested-only build without the DRM/TTY backend:

```sh
cargo build --release -p kyonde --no-default-features --features winit
```

Or install into `~/.local/bin` and copy the example configuration:

```sh
./scripts/install.sh
```

## Running

Start the nested compositor from an existing Wayland or X11 session:

```sh
./target/release/kyonde winit
```

Start on a bare TTY with:

```sh
./target/release/kyonde tty
```

Or, after installing:

```sh
kyon-session
```

`kyon-session` selects the TTY backend when no `WAYLAND_DISPLAY` or `DISPLAY` is present, and the nested backend otherwise. The installed Wayland session entry forces the TTY backend.

If the TTY backend reports `failed to open a libseat session`, start it from a free virtual terminal as a logged-in user, not from inside another compositor. The `seatd` service or `systemd-logind` must be available to grant device access.

The compositor prints the Wayland socket name it created (for example `wayland-2`). The default configuration starts `kyonshell session` automatically. The compositor resolves the shell next to its own binary first, then through `PATH`, so this also works from a bare TTY with a minimal environment. To start the shell manually:

```sh
WAYLAND_DISPLAY=wayland-2 kyonshell session
WAYLAND_DISPLAY=wayland-2 kyonshell launcher
WAYLAND_DISPLAY=wayland-2 kyonshell settings
```

Clients can be started through the launcher, the pinned taskbar, a terminal, or IPC:

```sh
kyonctl spawn foot
kyonctl spawn kitty
kyonctl layout floating
kyonctl layout scrolling
kyonctl layout tiling
kyonctl workspace 2
kyonctl window maximize
kyonctl window fullscreen
kyonctl state
```

## Configuration

The compositor reads `~/.config/kyonDE/config.lua` at startup and reloads it when the file changes. Runtime changes made by the settings app are stored in `~/.config/kyonDE/state.json`, and shell appearance is stored in `~/.config/kyonDE/shell.json`.

The Lua API is provided through the global `kyde` table. All functions accept partial tables, so only the options you want to change need to be present.

```lua
kyde.decor({
    rounding = 12,
    gaps_in = 8,
    gaps_out = 16,
    border_width = 1,
    active_border = "#3daee9",
    inactive_border = "#2c3140",
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
    scrolling = { column_width = 0.52, center_focus = true },
})

kyde.input({
    keyboard_layout = "us",
    repeat_rate = 40,
    repeat_delay = 300,
    natural_scroll = true,
})

kyde.shell({
    theme = "breeze",
    dark = true,
    accent = "#3daee9",
    panel_position = "top",
    panel_height = 38,
    panel_floating = false,
    panel_opacity = 0.82,
    workspaces = 5,
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
```

Key bindings use either `(mods, key, action)` or `(combo, action)`:

```lua
kyde.bind("SUPER", "Return", "spawn-terminal")
kyde.bind("SUPER+Q", "kill")
kyde.bind("SUPER+F", "layout floating")
kyde.bind("SUPER+T", "layout tiling")
kyde.bind("SUPER+S", "layout scrolling")
kyde.bind("SUPER+Space", "layout cycle")
kyde.bind("SUPER+1", "workspace 1")
kyde.bind("SUPER+SHIFT+1", "move-workspace 1")
kyde.bind("SUPER+V", "toggle-floating")
kyde.bind("SUPER+M", "fullscreen")
kyde.bind("SUPER+SHIFT+M", "maximize")
kyde.bind("SUPER+SHIFT+Comma", "move-output prev")
kyde.bind("SUPER+SHIFT+Period", "move-output next")
```

Additional DSL functions:

```lua
kyde.rule({ app_id = "pavucontrol", floating = true })
kyde.rule({ title = "Picture-in-Picture", floating = true, workspace = 3 })
kyde.output({
    name = "HDMI-A-1",
    scale = 1.0,
    mode = "2560x1440@144",
    position_x = 1920,
    position_y = 0,
    enabled = true,
    primary = false,
})
kyde.exec("kyonshell session")
kyde.exec("foot")
kyde.include("/path/to/another.lua")
```

Animation leaves accept `windowsIn`, `fadeIn`, `slideIn` and `scaleIn`. Bezier values accept named curves (`default`, `linear`, `easeIn`, `easeOut`, `easeInOut`, `snappy`, `overshoot`) or a four-value table.

Blur currently blurs the wallpaper backdrop behind translucent surfaces. It is intentionally optional and disabled with `kyde.blur({ enabled = false })`.

Environment variables can be set before the GPU stack is initialised:

```lua
kyde.env("AQ_DRM_DEVICES", "/dev/dri/card0:/dev/dri/card1")
kyde.env("WLR_DRM_DEVICES", "/dev/dri/card0:/dev/dri/card1")
kyde.env("__GLX_VENDOR_LIBRARY_NAME", "nvidia")
kyde.env("LIBVA_DRIVER_NAME", "nvidia")
```

`kyde.env` values are visible to the compositor and every process it starts. `AQ_DRM_DEVICES` and `WLR_DRM_DEVICES` are also mirrored to each other so configurations written for Hyprland or wlroots keep working.

The GPU section controls renderer selection and driver hints:

- `backend = "auto"` probes Vulkan devices and uses hardware accelerated OpenGL ES
- `backend = "vulkan"` asks for Vulkan. Smithay 0.7 exposes Vulkan device discovery but not a Vulkan scene renderer, so KyonDE probes the device, logs it and falls back to OpenGL ES
- `backend = "opengl"` forces OpenGL ES
- `backend = "software"` forces llvmpipe and is useful for troubleshooting or very old hardware
- `prefer_nvidia = true` logs NVIDIA presence but leaves EGL vendor selection to the active DRM device, which is required on hybrid laptops where the panel is driven by the integrated GPU
- `drm_devices` and `render_node` select laptop GPU devices for multi-GPU systems
- `damage_tracking` reserves the damage tracking policy switch; the winit backend currently redraws the full frame
- `max_fps` reserves a frame cap for the backend (the nested winit backend follows the host compositor's refresh rate)

## The shell

`kyonshell` has three modes:

```sh
kyonshell session
kyonshell panel
kyonshell desktop
kyonshell launcher
kyonshell settings
```

`kyonshell session` runs the panel and desktop icon surface in a single process to keep memory use low. The default configuration starts it automatically.

The panel is a layer-shell surface with a pinned application taskbar. Pinned entries can use desktop application IDs, display names, or the special shortcuts `@terminal`, `@settings`, `@launcher`. Configure them in `config.lua` with `kyde.shell({ pinned = { ... } })`.

`kyonshell desktop` provides the desktop icon surface. Click once to select, double-click to open, drag to move and right-click for the context menu. Icon positions are stored in `~/.config/kyonDE/desktop.json`.

The settings app applies changes live through IPC:

- **Appearance**: themes, dark mode, accent colour, panel placement, height, floating mode and opacity
- **Desktop**: layout mode, tiling algorithm, master ratio, gaps, rounding, borders, animation speed, scrolling column width and blur radius
- **Wallpaper**: gradient, solid colour or image, gradient stops and angle
- **Shortcuts**: current bindings
- **About**: version and component information

The panel polls `shell.json`, so changing the theme in settings updates the bar without restarting it.

## IPC

`kyonde` listens on `$XDG_RUNTIME_DIR/kyonde-$USER.sock`. Requests are newline-delimited JSON.

`kyonctl` supports:

```sh
kyonctl state
kyonctl layout <tiling|scrolling|floating|cycle>
kyonctl workspace <1-9>
kyonctl spawn <command>
kyonctl apply '{"decor":{"rounding":16}}'
kyonctl reload
kyonctl quit
```

For custom integrations, send an object such as `{"cmd":"state"}` and read the JSON response. The response contains a snapshot of the active layout, workspace, focused title, output size and full configuration.

## Packaging

A Makefile and a top-level Arch `PKGBUILD` are included:

```sh
make
make install
make arch-package
```

`make arch-package` creates a source tarball, builds `kyonde`, `kyonshell` and `kyonctl`, and runs `makepkg` in `dist/arch`. Install the result with:

```sh
sudo pacman -U dist/arch/kyonde-*.pkg.tar.zst
```

The package installs the compositor, shell, control client, session launcher, desktop entries and the Wayland session file.

The shell is also installed next to the compositor, so `kyde.exec("kyonshell session")` works even when a bare TTY starts with a minimal `PATH`.

## Repository layout

```text
kyonde/       compositor crate
kyonshell/    GTK4 panel, launcher and settings app
kyonctl/      IPC command line client
kyon-ipc/     config model, IPC types, theme presets
examples/     example config.lua
data/         desktop entry templates
scripts/      install, session and packaging helpers
smithay/      local Smithay checkout used as a path dependency
```

## Known limitations

- The DRM/KMS backend uses all cards and outputs present at startup. Hotplugged outputs are logged and require a compositor restart. GPU scanout migration and explicit PRIME copy paths are not implemented.
- Panels and wallpapers appear on every enabled output. Desktop icons are currently created on the primary output only.
- Smithay 0.7 does not include a Vulkan renderer. KyonDE can probe Vulkan devices and honours `backend = "vulkan"`, but scene rendering currently falls back to OpenGL ES.
- Client-side decorations are honoured for shell windows and clients that do not negotiate server-side decorations. Title bars drawn by the compositor are used for server-side clients.
- Rounded window corners are generated by compositor-drawn title bars and wallpaper corner patches. Overlapping windows can reveal the wallpaper in the rounded corner area.
- The TTY backend is compiled and its seat, input, DRM/GBM and render paths are implemented, but it should be tested from a free VT. Multi-GPU rendering and per-output panels are early work; hotplugging outputs requires a restart.
- Animations currently cover window entrance, layout position changes and title bar fading. Client surface scaling during resize is not animated.
- The shell's taskbar shows pinned launchers and the focused window title. It does not yet list every open window.

## Credits

Built on [Smithay](https://github.com/Smithay/smithay). The compositor structure was informed by Smithay's Anvil and Smallvil examples. GTK4 and gtk4-layer-shell provide the shell surfaces. The project is licensed under the MIT license.
