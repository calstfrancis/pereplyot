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
    /// Folder that receives a Markdown file of each document's notes; empty means off.
    pub markdown_folder: String,
    /// The last version whose Welcome / What's New window was shown.
    pub last_seen_version: String,
    /// Folder that receives a daily backup; empty means off.
    pub backup_folder: String,
    /// The citation style notes and references are worded in (a `cite::styles()` name).
    pub citation_style: String,
    /// Hide every button that would send a request to an online service.
    pub never_online: bool,
    /// Spaced review of what is tagged `#remember`; off until asked for.
    pub review: bool,
    /// "Look up…" on a selection: a name and a web address with `{q}` where the words go. Edit the list
    /// in `config.json` to add a library catalogue or take a site away.
    pub lookup_templates: Vec<(String, String)>,
}

fn default_lookups() -> Vec<(String, String)> {
    [
        (
            "Wikipedia",
            "https://en.wikipedia.org/wiki/Special:Search?search={q}",
        ),
        ("Logeion (Greek, Latin)", "https://logeion.uchicago.edu/{q}"),
        ("PhilPapers", "https://philpapers.org/s/{q}"),
        ("Google Scholar", "https://scholar.google.com/scholar?q={q}"),
        (
            "Bible Gateway",
            "https://www.biblegateway.com/passage/?search={q}",
        ),
    ]
    .iter()
    .map(|(n, u)| (n.to_string(), u.to_string()))
    .collect()
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
            markdown_folder: String::new(),
            last_seen_version: String::new(),
            backup_folder: String::new(),
            citation_style: crate::cite::DEFAULT_STYLE.to_string(),
            never_online: false,
            review: false,
            lookup_templates: default_lookups(),
        }
    }
}

impl Config {
    pub fn load() -> Config {
        use fond_read_gtk::fsutil::{lenient, load_checked, Loaded};
        match load_checked(&config_path(), lenient::<Config>) {
            Loaded::Ok(c) => c,
            Loaded::Missing => Config::default(),
            Loaded::Corrupt {
                recovered_from_backup,
                ..
            } => recovered_from_backup.unwrap_or_default(),
        }
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
