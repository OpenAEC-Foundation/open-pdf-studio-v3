//! Persistent application settings (Preferences + recent files).
//!
//! Stored as JSON in the OS config directory. `#[serde(default)]` makes every
//! field optional when loading, so new settings can be added over time without
//! breaking config files written by older versions — keeping this maintainable
//! as the app grows.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub interface_language: String,
    pub theme: String,
    pub restore_session: bool,
    pub author_name: String,
    pub recent_files: Vec<String>,
    /// Last window size (logical px); 0 = unset (use the default size).
    pub window_w: f32,
    pub window_h: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            interface_language: "Auto-detect".to_string(),
            theme: "Default".to_string(),
            restore_session: false,
            author_name: std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_default(),
            recent_files: Vec::new(),
            window_w: 0.0,
            window_h: 0.0,
        }
    }
}

impl Settings {
    /// Location of the settings file (e.g. `%APPDATA%\Impertio\OpenPdfStudio\config\settings.json`).
    fn path() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "Impertio", "OpenPdfStudio")
            .map(|dirs| dirs.config_dir().join("settings.json"))
    }

    /// Load from disk, falling back to defaults on any error (missing file,
    /// parse error, etc.).
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persist to disk. Best-effort: failures are logged, never fatal.
    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    eprintln!("[settings] failed to write {path:?}: {e}");
                }
            }
            Err(e) => eprintln!("[settings] serialize failed: {e}"),
        }
    }

    /// Move `path` to the front of the recents list (dedup, cap at 50).
    pub fn push_recent(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_string());
        self.recent_files.truncate(50);
    }

    pub fn remove_recent(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
    }
}
