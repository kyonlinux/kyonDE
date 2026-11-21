use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::{ipc, prefs::ShellPrefs, theme};

const CELL_W: f64 = 104.0;
const CELL_H: f64 = 112.0;
const MARGIN: f64 = 18.0;

#[derive(Clone)]
struct Entry {
    key: String,
    name: String,
    path: PathBuf,
    is_dir: bool,
    icon: &'static str,
}

pub fn build(app: &gtk::Application) {
    let prefs = crate::prefs::load_from_disk();
    let provider = gtk::CssProvider::new();
    theme::install(&provider, &prefs);

    let window = gtk::Window::builder()
        .application(app)
        .decorated(false)
        .build();
    window.add_css_class("shell-window");
    window.add_css_class("desktop-window");
    window.init_layer_shell();
    if let Some(display) = gtk::gdk::Display::default() {
        let monitors = display.monitors();
        let primary = crate::ipc::state().and_then(|snapshot| {
            snapshot
                .displays
                .into_iter()
                .find(|item| item.primary)
                .map(|item| item.name)
        });
        let mut selected = None;
        for index in 0..monitors.n_items() {
            if let Some(monitor) = monitors.item(index).and_downcast::<gtk::gdk::Monitor>() {
                if selected.is_none() {
                    selected = Some(monitor.clone());
                }
                if let Some(primary) = &primary {
                    if monitor.connector().as_deref() == Some(primary.as_str()) {
                        selected = Some(monitor);
                        break;
                    }
                }
            }
        }
        if let Some(monitor) = selected {
            window.set_monitor(Some(&monitor));
        }
    }
    window.set_namespace(Some("kyonshell-desktop"));
    window.set_layer(Layer::Background);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_exclusive_zone(-1);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }

    let fixed = gtk::Fixed::new();
    fixed.set_hexpand(true);
    fixed.set_vexpand(true);
    fixed.add_css_class("desktop-surface");
    window.set_child(Some(&fixed));

    let positions_path = kyon_ipc::config_home().join("desktop.json");
    let positions: Rc<RefCell<HashMap<String, (f64, f64)>>> =
        Rc::new(RefCell::new(load_positions(&positions_path)));

    let refresh_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let refresh: Rc<dyn Fn()> = {
        let slot = refresh_slot.clone();
        let fixed = fixed.clone();
        let positions = positions.clone();
        let positions_path = positions_path.clone();
        let window = window.clone();
        Rc::new(move || {
            let Some(cb) = slot.borrow().clone() else {
                return;
            };
            rebuild(&fixed, &window, &positions, &positions_path, cb);
        })
    };
    *refresh_slot.borrow_mut() = Some(refresh.clone());
    refresh();

    let background_click = gtk::GestureClick::new();
    background_click.set_button(3);
    let fixed_bg = fixed.clone();
    let window_bg = window.clone();
    let refresh_bg = refresh.clone();
    background_click.connect_pressed(move |gesture, _, x, y| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        let children = fixed_bg.observe_children();
        for index in 0..children.n_items() {
            if let Some(child) = children.item(index).and_downcast::<gtk::Widget>() {
                child.remove_css_class("selected");
            }
        }
        show_background_menu(&window_bg, &fixed_bg, x, y, refresh_bg.clone());
    });
    fixed.add_controller(background_click);

    let provider_ref = provider.clone();
    let mut last_mtime = crate::prefs::mtime();
    glib::timeout_add_seconds_local(2, move || {
        if let Some(mtime) = crate::prefs::mtime() {
            if Some(mtime) != last_mtime {
                last_mtime = Some(mtime);
                theme::install(&provider_ref, &crate::prefs::load_from_disk());
            }
        }
        glib::ControlFlow::Continue
    });

    window.present();
}

fn rebuild(
    fixed: &gtk::Fixed,
    window: &gtk::Window,
    positions: &Rc<RefCell<HashMap<String, (f64, f64)>>>,
    positions_path: &Path,
    refresh: Rc<dyn Fn()>,
) {
    let children = fixed.observe_children();
    for index in 0..children.n_items() {
        if let Some(child) = children.item(index).and_downcast::<gtk::Widget>() {
            child.unparent();
        }
    }
    let desktop_dir = desktop_directory();
    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&desktop_dir) {
        let mut sorted: Vec<_> = read_dir.flatten().collect();
        sorted.sort_by_key(|entry| entry.file_name());
        for entry in sorted {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let is_dir = path.is_dir();
            entries.push(Entry {
                key: name.clone(),
                name,
                path: path.clone(),
                is_dir,
                icon: file_icon(&path, is_dir),
            });
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    entries.push(Entry {
        key: "@home".into(),
        name: "Home".into(),
        path: home,
        is_dir: true,
        icon: "user-home",
    });
    let trash = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".local/share/Trash/files");
    entries.push(Entry {
        key: "@trash".into(),
        name: "Trash".into(),
        path: trash,
        is_dir: true,
        icon: "user-trash",
    });

    for (index, entry) in entries.into_iter().enumerate() {
        let default_col = index / 10;
        let default_row = index % 10;
        let fallback = (
            MARGIN + default_col as f64 * CELL_W,
            MARGIN + default_row as f64 * CELL_H,
        );
        let (mut x, mut y) = positions
            .borrow()
            .get(&entry.key)
            .copied()
            .unwrap_or(fallback);
        x = x.max(0.0);
        y = y.max(0.0);
        let icon = make_icon(
            window,
            &entry,
            positions.clone(),
            positions_path.to_path_buf(),
            refresh.clone(),
        );
        fixed.put(&icon, x, y);
        let start = Rc::new(Cell::new((x, y)));
        let fixed_drag = fixed.clone();
        let icon_drag = icon.clone();
        let start_begin = start.clone();
        let start_update = start.clone();
        let drag = gtk::GestureDrag::new();
        let fixed_begin = fixed_drag.clone();
        drag.connect_drag_begin(move |_, _, _| {
            let (cx, cy) = fixed_begin.child_position(&icon_drag);
            start_begin.set((cx, cy));
        });
        let icon_update = icon.clone();
        drag.connect_drag_update(move |_, ox, oy| {
            let (sx, sy) = start_update.get();
            let nx = (sx + ox).max(0.0);
            let ny = (sy + oy).max(0.0);
            fixed_drag.move_(&icon_update, nx, ny);
        });
        let key = entry.key.clone();
        let positions_end = positions.clone();
        let positions_path = positions_path.to_path_buf();
        let fixed_end = fixed.clone();
        let icon_end = icon.clone();
        drag.connect_drag_end(move |_, ox, oy| {
            let (sx, sy) = start.get();
            let nx = ((sx + ox) / CELL_W).round() * CELL_W + MARGIN;
            let ny = ((sy + oy) / CELL_H).round() * CELL_H + MARGIN;
            let nx = nx.max(0.0);
            let ny = ny.max(0.0);
            fixed_end.move_(&icon_end, nx, ny);
            positions_end.borrow_mut().insert(key.clone(), (nx, ny));
            save_positions(&positions_path, &positions_end.borrow());
        });
        icon.add_controller(drag);
    }
}

fn make_icon(
    window: &gtk::Window,
    entry: &Entry,
    positions: Rc<RefCell<HashMap<String, (f64, f64)>>>,
    positions_path: PathBuf,
    refresh: Rc<dyn Fn()>,
) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 4);
    container.add_css_class("desktop-icon");
    container.set_size_request(92, 100);
    container.set_valign(gtk::Align::Start);

    let image = gtk::Image::from_icon_name(entry.icon);
    image.set_pixel_size(48);
    image.add_css_class("desktop-icon-image");
    let label = gtk::Label::new(Some(&entry.name));
    label.add_css_class("desktop-label");
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_justify(gtk::Justification::Center);
    label.set_max_width_chars(12);
    label.set_lines(2);
    label.set_wrap(true);
    container.append(&image);
    container.append(&label);

    let click = gtk::GestureClick::new();
    click.set_button(1);
    let path = entry.path.clone();
    let window_click = window.clone();
    let container_select = container.clone();
    click.connect_pressed(move |gesture, presses, _, _| {
        container_select.add_css_class("selected");
        if presses == 2 {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            open_path(&path);
            let _ = &window_click;
        }
    });
    container.add_controller(click);

    let right_click = gtk::GestureClick::new();
    right_click.set_button(3);
    let entry_clone = entry.clone();
    let refresh_menu = refresh.clone();
    let window_menu = window.clone();
    let positions_menu = positions.clone();
    let path_menu = positions_path.clone();
    let container_menu = container.clone();
    right_click.connect_pressed(move |gesture, _, _, _| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        show_icon_menu(
            &window_menu,
            &container_menu,
            &entry_clone,
            refresh_menu.clone(),
            positions_menu.clone(),
            path_menu.clone(),
        );
    });
    container.add_controller(right_click);
    container
}

fn show_icon_menu(
    window: &gtk::Window,
    anchor: &gtk::Box,
    entry: &Entry,
    refresh: Rc<dyn Fn()>,
    positions: Rc<RefCell<HashMap<String, (f64, f64)>>>,
    positions_path: PathBuf,
) {
    let popover = context_popover(anchor);
    let path = entry.path.clone();
    let is_dir = entry.is_dir;
    let name = entry.name.clone();
    let key = entry.key.clone();
    menu_action(&popover, "Open", {
        let path = path.clone();
        move || open_path(&path)
    });
    if is_dir {
        menu_action(&popover, "Open in terminal", {
            let path = path.clone();
            move || open_terminal(&path)
        });
    }
    if !key.starts_with('@') {
        menu_action(&popover, "Rename", {
            let window = window.clone();
            let path = path.clone();
            let name = name.clone();
            let refresh = refresh.clone();
            move || {
                prompt_rename(&window, &path, &name, refresh.clone());
            }
        });
    }
    menu_action(&popover, "Properties", {
        let window = window.clone();
        let path = path.clone();
        let name = name.clone();
        move || show_properties(&window, &path, &name)
    });
    if !key.starts_with('@') {
        menu_action(&popover, "Move to trash", {
            let path = path.clone();
            let key = key.clone();
            let positions = positions.clone();
            let positions_path = positions_path.clone();
            let refresh = refresh.clone();
            move || {
                let _ = gio::File::for_path(&path).trash(None::<&gio::Cancellable>);
                positions.borrow_mut().remove(&key);
                save_positions(&positions_path, &positions.borrow());
                refresh();
            }
        });
    }
    popover.popup();
}

fn show_background_menu(
    window: &gtk::Window,
    anchor: &gtk::Fixed,
    x: f64,
    y: f64,
    refresh: Rc<dyn Fn()>,
) {
    let popover = gtk::Popover::new();
    popover.add_css_class("context-menu");
    popover.set_parent(anchor);
    popover.set_has_arrow(false);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    popover.set_child(Some(&content));

    let _ = window;
    menu_action(&popover, "New folder", {
        let refresh = refresh.clone();
        move || {
            let dir = desktop_directory();
            let mut target = dir.join("New Folder");
            let mut index = 2;
            while target.exists() {
                target = dir.join(format!("New Folder {index}"));
                index += 1;
            }
            let _ = std::fs::create_dir_all(&target);
            refresh();
        }
    });
    menu_action(&popover, "Open terminal", {
        let dir = desktop_directory();
        move || open_terminal(&dir)
    });
    menu_action(&popover, "Change wallpaper", || ipc::spawn_self("settings"));
    menu_action(&popover, "Display settings", || ipc::spawn_self("settings"));
    menu_action(&popover, "Refresh", move || refresh());
    popover.popup();
}

fn context_popover(anchor: &impl IsA<gtk::Widget>) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("context-menu");
    popover.set_parent(anchor);
    popover.set_has_arrow(false);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    popover.set_child(Some(&content));
    let pop = popover.clone();
    popover.connect_closed(move |_| pop.unparent());
    popover
}

fn menu_action(popover: &gtk::Popover, label: &str, action: impl Fn() + 'static) {
    let Some(content) = popover.child().and_downcast::<gtk::Box>() else {
        return;
    };
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("context-item");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    row.append(&text);
    button.set_child(Some(&row));
    let popover = popover.clone();
    button.connect_clicked(move |_| {
        popover.popdown();
        action();
    });
    content.append(&button);
}

fn prompt_rename(window: &gtk::Window, path: &Path, name: &str, refresh: Rc<dyn Fn()>) {
    let dialog = gtk::Window::builder()
        .title("Rename")
        .transient_for(window)
        .modal(true)
        .default_width(380)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    let entry = gtk::Entry::new();
    entry.set_text(name);
    content.append(&entry);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    let rename = gtk::Button::with_label("Rename");
    rename.add_css_class("suggested");
    buttons.append(&cancel);
    buttons.append(&rename);
    content.append(&buttons);
    dialog.set_child(Some(&content));
    let dialog_close = dialog.clone();
    cancel.connect_clicked(move |_| dialog_close.close());
    let path = path.to_path_buf();
    let dialog_rename = dialog.clone();
    rename.connect_clicked(move |_| {
        let new_name = entry.text().to_string();
        if !new_name.is_empty() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::rename(&path, parent.join(new_name));
            }
        }
        dialog_rename.close();
        refresh();
    });
    dialog.present();
}

fn show_properties(window: &gtk::Window, path: &Path, name: &str) {
    let dialog = gtk::Window::builder()
        .title("Properties")
        .transient_for(window)
        .modal(true)
        .default_width(430)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);
    let title = gtk::Label::new(Some(name));
    title.add_css_class("section-title");
    title.set_xalign(0.0);
    content.append(&title);
    let kind = if path.is_dir() { "Folder" } else { "File" };
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let details = gtk::Label::new(None);
    details.set_markup(&format!(
        "<b>Type</b>  {}\n<b>Path</b>  {}\n<b>Size</b>  {} bytes",
        kind,
        glib::markup_escape_text(&path.display().to_string()),
        size
    ));
    details.set_xalign(0.0);
    details.set_selectable(true);
    content.append(&details);
    let close = gtk::Button::with_label("Close");
    close.set_halign(gtk::Align::End);
    let dialog_close = dialog.clone();
    close.connect_clicked(move |_| dialog_close.close());
    content.append(&close);
    dialog.set_child(Some(&content));
    dialog.present();
}

fn open_path(path: &Path) {
    let uri = format!("file://{}", path.display());
    if gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>).is_err() {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

fn open_terminal(path: &Path) {
    for terminal in ["foot", "kitty", "alacritty", "wezterm", "konsole", "gnome-terminal", "xterm"] {
        if std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {terminal}"))
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            let _ = std::process::Command::new(terminal).current_dir(path).spawn();
            return;
        }
    }
}

fn file_icon(path: &Path, is_dir: bool) -> &'static str {
    if is_dir {
        return "folder";
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => "image-x-generic",
        "mp4" | "mkv" | "webm" | "avi" => "video-x-generic",
        "mp3" | "flac" | "ogg" | "wav" => "audio-x-generic",
        "pdf" => "x-office-document",
        "zip" | "tar" | "gz" | "xz" | "7z" => "package-x-generic",
        _ => "text-x-generic",
    }
}

fn desktop_directory() -> PathBuf {
    if let Some(dir) = glib::user_special_dir(glib::UserDirectory::Desktop) {
        return dir;
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Desktop")
}

fn load_positions(path: &Path) -> HashMap<String, (f64, f64)> {
    let Ok(data) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(values) = serde_json::from_str::<HashMap<String, [f64; 2]>>(&data) else {
        return HashMap::new();
    };
    values.into_iter().map(|(key, value)| (key, (value[0], value[1]))).collect()
}

fn save_positions(path: &Path, positions: &HashMap<String, (f64, f64)>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let values: HashMap<String, [f64; 2]> = positions
        .iter()
        .map(|(key, (x, y))| (key.clone(), [*x, *y]))
        .collect();
    if let Ok(data) = serde_json::to_string_pretty(&values) {
        let _ = std::fs::write(path, data);
    }
}
