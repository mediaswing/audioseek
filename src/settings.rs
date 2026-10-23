//! User settings, stored as JSON in the app's config folder.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// How a downloaded episode's file is named.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Naming {
    /// When it was published, in local time, such as `2026-10-23-14-05-00.mp3`.
    #[default]
    Timestamp,
    /// The timestamp, then the episode's title.
    TimestampTitle,
    /// The episode's title alone.
    Title,
}

impl Naming {
    pub const ALL: [Naming; 3] = [Naming::Timestamp, Naming::TimestampTitle, Naming::Title];

    pub fn label(self) -> &'static str {
        match self {
            Naming::Timestamp => "Publish date and time (2026-10-23-14-05-00.mp3)",
            Naming::TimestampTitle => "Date and time, then title (2026-10-23-14-05-00 Title.mp3)",
            Naming::Title => "Episode title (Title.mp3)",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub download_dir: PathBuf,
    pub naming: Naming,
    /// Put each podcast's episodes in a folder named after the podcast.
    pub folder_per_podcast: bool,
    /// Whether keyboard shortcuts work at all, if the user has chosen.
    /// Otherwise they are on. See `shortcuts_enabled`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcuts_on: Option<bool>,
    /// Shortcuts changed from the defaults, keyed by action, such as
    /// "download_selected": "F9". "off" turns one off. See `shortcuts.rs`.
    pub shortcuts: BTreeMap<String, String>,
    /// Ask GitHub whether there is a newer version when the app starts.
    pub check_for_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: crate::paths::default_download_dir(),
            naming: Naming::Timestamp,
            folder_per_podcast: true,
            shortcuts_on: None,
            shortcuts: BTreeMap::new(),
            check_for_updates: true,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let path = crate::paths::settings_file();
        let Ok(bytes) = std::fs::read(&path) else { return Self::default() };
        serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            log::warn!("could not read {}, so using the default settings: {e}", path.display());
            Self::default()
        })
    }

    pub fn shortcuts_enabled(&self) -> bool {
        self.shortcuts_on.unwrap_or(true)
    }

    pub fn save(&self) {
        let Ok(json) = serde_json::to_vec_pretty(self) else { return };
        if let Err(e) = crate::paths::write_atomic(&crate::paths::settings_file(), &json) {
            log::warn!("could not save settings: {e}");
        }
    }
}
