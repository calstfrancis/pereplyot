use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn config_dir() -> PathBuf {
    glib::user_config_dir().join("pereplyot")
}

fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// "system" | "light" | "dark".
    pub theme: String,
    /// Custom labels for the reader's four highlight colours, in palette order; blank means
    /// the colour's default label.
    pub highlight_labels: Vec<String>,
    /// Library card width in px (`LIBRARY_SIZE_MIN..=LIBRARY_SIZE_MAX`).
    pub library_card_size: u32,
    /// "added" | "title".
    pub library_sort: String,
    /// The shelf last shown in the Library; empty means all documents.
    pub library_shelf: String,
    /// Show a few highlights from past reading on the Library page.
    pub resurface: bool,
    /// Texture each reading colour as well as colouring it.
    pub patterns: bool,
    /// The last version whose Welcome / What's New window was shown.
    pub last_seen_version: String,
}

pub const LIBRARY_SIZE_MIN: u32 = 80;
pub const LIBRARY_SIZE_MAX: u32 = 280;

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: "system".to_string(),
            highlight_labels: Vec::new(),
            library_card_size: 120,
            library_sort: "added".to_string(),
            library_shelf: String::new(),
            resurface: false,
            patterns: false,
            last_seen_version: String::new(),
        }
    }
}

impl Config {
    pub fn load() -> Config {
        fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fond_read_gtk::fsutil::write_atomic(&config_path(), json.as_bytes());
        }
    }

    pub fn color_scheme(&self) -> libadwaita::ColorScheme {
        match self.theme.as_str() {
            "light" => libadwaita::ColorScheme::ForceLight,
            "dark" => libadwaita::ColorScheme::ForceDark,
            _ => libadwaita::ColorScheme::Default,
        }
    }
}
