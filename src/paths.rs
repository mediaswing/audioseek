//! Locations of the app's own files.

use std::path::{Path, PathBuf};

#[cfg(any(target_os = "windows", target_os = "macos"))]
const APP_DIR: &str = "AudioSeek";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const APP_DIR: &str = "audioseek";

/// Settings and the list of feeds. On Windows this is `%APPDATA%\AudioSeek`.
pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join(APP_DIR)
}

pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

/// Every feed added, and when each was last fetched, so the list survives
/// losing the OPML file it came from.
pub fn feeds_file() -> PathBuf {
    config_dir().join("feeds.json")
}

/// Warnings, errors and crashes. See `logging.rs`.
pub fn log_file() -> PathBuf {
    config_dir().join("audioseek.log")
}

/// The log before the last one, kept when it grew too large.
pub fn old_log_file() -> PathBuf {
    config_dir().join("audioseek.old.log")
}

/// Where episodes go unless another folder is chosen.
pub fn default_download_dir() -> PathBuf {
    dirs::audio_dir()
        .or_else(dirs::download_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("Podcasts")
}

/// Moves a file that couldn't be read out of the way, so it isn't saved
/// over, and returns where it went. The name has the time in it, such as
/// `feeds.json.damaged-2026-10-23-14-05-00`, so an earlier one is kept too.
/// If there is one already from the same second, " (2)" and so on is added.
pub fn back_up_damaged(path: &Path) -> std::io::Result<PathBuf> {
    let mut stem = path.file_name().unwrap_or_default().to_owned();
    stem.push(chrono::Local::now().format(".damaged-%Y-%m-%d-%H-%M-%S").to_string());
    let mut backup = path.with_file_name(&stem);
    let mut n = 2;
    // Renaming would replace an earlier backup on Linux and macOS.
    while backup.exists() {
        let mut name = stem.clone();
        name.push(format!(" ({n})"));
        backup = path.with_file_name(name);
        n += 1;
    }
    std::fs::rename(path, &backup)?;
    Ok(backup)
}

/// Writes a file atomically: to a temporary file beside it, then renamed
/// over it, so a crash part way through never leaves it half written.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| std::io::Error::other("file has no parent folder"))?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn backups_never_replace_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feeds.json");
        let mut backups = Vec::new();
        for i in 0..3 {
            std::fs::write(&path, format!("damaged {i}")).unwrap();
            backups.push(super::back_up_damaged(&path).unwrap());
        }
        assert!(!path.exists());
        for (i, backup) in backups.iter().enumerate() {
            assert_eq!(std::fs::read_to_string(backup).unwrap(), format!("damaged {i}"));
        }
    }
}
