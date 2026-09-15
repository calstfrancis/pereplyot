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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: "system".to_string(),
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
        let dir = config_dir();
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(config_path(), json);
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
