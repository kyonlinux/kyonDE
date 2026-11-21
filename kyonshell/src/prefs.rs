use std::path::PathBuf;
use std::time::SystemTime;

use kyon_ipc::{KyonConfig, theme};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellPrefs {
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
    pub pinned: Vec<String>,
}

impl Default for ShellPrefs {
    fn default() -> Self {
        let shell = KyonConfig::default().shell;
        Self::from_shell(&shell)
    }
}

impl ShellPrefs {
    pub fn from_shell(shell: &kyon_ipc::Shell) -> Self {
        Self {
            theme: shell.theme.clone(),
            dark: shell.dark,
            accent: shell.accent.clone(),
            font: shell.font.clone(),
            panel_position: shell.panel_position.clone(),
            panel_height: shell.panel_height,
            panel_floating: shell.panel_floating,
            panel_opacity: shell.panel_opacity,
            clock_seconds: shell.clock_seconds,
            workspaces: shell.workspaces,
            pinned: shell.pinned.clone(),
        }
    }

    pub fn to_shell(&self) -> kyon_ipc::Shell {
        kyon_ipc::Shell {
            theme: self.theme.clone(),
            dark: self.dark,
            accent: self.accent.clone(),
            font: self.font.clone(),
            panel_position: self.panel_position.clone(),
            panel_height: self.panel_height,
            panel_floating: self.panel_floating,
            panel_opacity: self.panel_opacity,
            clock_seconds: self.clock_seconds,
            workspaces: self.workspaces,
            launcher_style: "grid".into(),
            pinned: self.pinned.clone(),
        }
    }

    pub fn preset(&self) -> &'static kyon_ipc::Theme {
        theme(&self.theme)
    }

    pub fn palette(&self) -> kyon_ipc::Palette {
        self.preset().palette(self.dark)
    }

    pub fn accent_color(&self) -> String {
        let preset = self.preset();
        if self.accent.is_empty() {
            preset.palette(self.dark).accent.to_string()
        } else {
            self.accent.clone()
        }
    }
}

pub fn path() -> PathBuf {
    kyon_ipc::shell_path()
}

pub fn load_from_disk() -> ShellPrefs {
    if let Ok(data) = std::fs::read_to_string(path()) {
        if let Ok(prefs) = serde_json::from_str::<ShellPrefs>(&data) {
            return prefs;
        }
    }
    if let Some(snapshot) = crate::ipc::state() {
        return ShellPrefs::from_shell(&snapshot.config.shell);
    }
    ShellPrefs::default()
}

pub fn save(prefs: &ShellPrefs) {
    let path = path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(prefs) {
        let _ = std::fs::write(path, data);
    }
}

pub fn mtime() -> Option<SystemTime> {
    std::fs::metadata(path()).and_then(|m| m.modified()).ok()
}

pub fn apply_to_compositor(prefs: &ShellPrefs) {
    crate::ipc::apply(serde_json::json!({
        "shell": prefs.to_shell(),
    }));
}
