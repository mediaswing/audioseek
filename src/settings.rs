//! User settings, stored as JSON in the app's config folder.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    /// Why the file couldn't be read when the app started, if it is there
    /// but couldn't be. It is then never saved over, as in `Library`.
    #[serde(skip)]
    pub load_error: Option<String>,
    /// Something to tell the user about loading the file that doesn't stop
    /// it being saved, such as it being damaged and put aside.
    #[serde(skip)]
    pub load_notice: Option<String>,
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
            load_error: None,
            load_notice: None,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        Self::load_from(&crate::paths::settings_file())
    }

    fn load_from(path: &Path) -> Self {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                log::error!("could not read {}: {e}", path.display());
                return Self {
                    load_error: Some(format!(
                        "Could not read the settings in {}: {e}. The default settings are being used, and \
                         changes to them won't be saved until the app is started again.",
                        path.display()
                    )),
                    ..Self::default()
                };
            }
        };
        serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            log::warn!("could not read {}, so using the default settings: {e}", path.display());
            match crate::paths::back_up_damaged(path) {
                Ok(backup) => Self {
                    load_notice: Some(format!(
                        "The settings in {} were damaged ({e}), so the default settings are being used. The \
                         damaged file has been kept as {}.",
                        path.display(),
                        backup.display()
                    )),
                    ..Self::default()
                },
                Err(move_error) => Self {
                    load_error: Some(format!(
                        "The settings in {} are damaged ({e}) and could not be put aside ({move_error}). The \
                         default settings are being used, and changes to them won't be saved until the app is \
                         started again.",
                        path.display()
                    )),
                    ..Self::default()
                },
            }
        })
    }

    pub fn shortcuts_enabled(&self) -> bool {
        self.shortcuts_on.unwrap_or(true)
    }

    /// Saves the settings, and says whether it did: not if the file
    /// couldn't be read at startup (see `load_error`), so it isn't saved
    /// over.
    pub fn save(&self) -> std::io::Result<bool> {
        self.save_to(&crate::paths::settings_file())
    }

    fn save_to(&self, path: &Path) -> std::io::Result<bool> {
        if self.load_error.is_some() {
            return Ok(false);
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::paths::write_atomic(path, &json)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damaged_file_is_kept_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let settings = Settings::load_from(&path);
        assert!(settings.load_notice.is_some());
        assert!(!path.exists());
        assert!(settings.save_to(&path).unwrap());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2, "the damaged copy is kept");
    }

    #[test]
    fn unreadable_file_is_not_saved_over() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be can't be read, like a locked file.
        let blocked = dir.path().join("settings.json");
        std::fs::create_dir(&blocked).unwrap();
        let settings = Settings::load_from(&blocked);
        assert!(settings.load_error.is_some());
        assert!(!settings.save_to(&blocked).unwrap(), "not saved");
        assert!(blocked.is_dir());
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"naming":"title"}"#).unwrap();
        assert_eq!(settings.naming, Naming::Title);
        assert!(settings.folder_per_podcast);
        assert!(settings.check_for_updates);
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("load_error") && !json.contains("load_notice"), "what loading found is never saved");
    }
}
