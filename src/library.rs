//! The podcasts the user follows, kept in `feeds.json` so they survive
//! losing the OPML file they were imported from. Each feed records when it
//! was last fetched and which episodes have been downloaded.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feed {
    /// The RSS or Atom feed's address. Feeds are told apart by this.
    pub url: String,
    pub title: String,
    /// The podcast's web page, if the OPML file or the feed gave one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub website: String,
    pub added: DateTime<Utc>,
    /// When the feed was last fetched successfully.
    #[serde(default)]
    pub last_fetched: Option<DateTime<Utc>>,
    /// Ids (GUIDs, or failing that the audio address) of the episodes
    /// downloaded, so they can be left out next time.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub downloaded: BTreeSet<String>,
    /// Set by hand in `feeds.json` to leave the feed out of fetching every
    /// feed and of fetching on its own when shown. It can still be fetched
    /// by choosing it and fetching just that one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

impl Feed {
    pub fn new(url: String, title: String, website: String) -> Self {
        Feed { url, title, website, added: Utc::now(), last_fetched: None, downloaded: BTreeSet::new(), disabled: false }
    }

    /// The name shown in lists: its title, or its address if it has none yet.
    pub fn name(&self) -> &str {
        if self.title.trim().is_empty() { &self.url } else { &self.title }
    }

    /// "Last fetched 23 Oct 2026 14:05", in local time, or "Never fetched".
    pub fn fetched_text(&self) -> String {
        match self.last_fetched {
            Some(at) => format!("last fetched {}", at.with_timezone(&chrono::Local).format("%-d %b %Y %H:%M")),
            None => "never fetched".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub feeds: Vec<Feed>,
    /// Why the file couldn't be read when the app started, if it is there
    /// but couldn't be (locked by another program, say). It is then never
    /// saved over, so the feeds in it aren't lost.
    #[serde(skip)]
    pub load_error: Option<String>,
    /// Something the user should be told about loading the file that
    /// doesn't stop it being saved, such as it being damaged and put aside.
    #[serde(skip)]
    pub load_notice: Option<String>,
}

impl Library {
    pub fn load() -> Self {
        Self::load_from(&crate::paths::feeds_file())
    }

    fn load_from(path: &std::path::Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                // Keep the damaged file rather than overwrite it on the next save.
                log::warn!("could not read {}: {e}", path.display());
                match crate::paths::back_up_damaged(path) {
                    Ok(backup) => Library {
                        load_notice: Some(format!(
                            "The list of feeds in {} was damaged ({e}), so the app has started with an empty \
                             list. The damaged file has been kept as {}.",
                            path.display(),
                            backup.display()
                        )),
                        ..Library::default()
                    },
                    // Not moved, so saving would lose it.
                    Err(move_error) => Library {
                        load_error: Some(format!(
                            "The list of feeds in {} is damaged ({e}) and could not be put aside \
                             ({move_error}). It has been left as it is, and changes to the list won't be saved \
                             until the app is started again.",
                            path.display()
                        )),
                        ..Library::default()
                    },
                }
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Library::default(),
            Err(e) => {
                log::error!("could not read {}: {e}", path.display());
                Library {
                    load_error: Some(format!(
                        "Could not read the list of feeds in {}: {e}. It has been left as it is, and changes \
                         to the list won't be saved until the app is started again.",
                        path.display()
                    )),
                    ..Library::default()
                }
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&crate::paths::feeds_file())
    }

    fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        if self.load_error.is_some() {
            return Err(std::io::Error::other("the list of feeds couldn't be read, so it isn't saved over"));
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::paths::write_atomic(path, &json)
    }

    pub fn find(&self, url: &str) -> Option<usize> {
        self.feeds.iter().position(|f| same_url(&f.url, url))
    }

    /// Adds feeds not already in the list, and returns how many were new.
    /// A title or web page missing from a feed already there is filled in.
    pub fn add_all(&mut self, feeds: Vec<Feed>) -> usize {
        let mut added = 0;
        for feed in feeds {
            match self.find(&feed.url) {
                Some(i) => {
                    let known = &mut self.feeds[i];
                    if known.title.is_empty() {
                        known.title = feed.title;
                    }
                    if known.website.is_empty() {
                        known.website = feed.website;
                    }
                }
                None => {
                    self.feeds.push(feed);
                    added += 1;
                }
            }
        }
        added
    }
}

/// Feed addresses that differ only in http/https, the case of the host name
/// or a trailing slash are the same feed. The rest of the address is case
/// sensitive, as feed ids in it often are.
fn same_url(a: &str, b: &str) -> bool {
    fn key(u: &str) -> String {
        let u = u.trim();
        let u = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://")).unwrap_or(u);
        let u = u.trim_end_matches('/');
        let (host, path) = u.split_at(u.find('/').unwrap_or(u.len()));
        format!("{}{path}", host.to_lowercase())
    }
    key(a) == key(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates_are_not_added() {
        let mut lib = Library::default();
        let a = Feed::new("https://example.com/feed/".into(), String::new(), String::new());
        let b = Feed::new("http://EXAMPLE.com/feed".into(), "Example".into(), String::new());
        assert_eq!(lib.add_all(vec![a]), 1);
        assert_eq!(lib.add_all(vec![b]), 0);
        assert_eq!(lib.feeds[0].title, "Example");
        // Only the host name is case-insensitive.
        let c = Feed::new("https://example.com/FEED".into(), String::new(), String::new());
        assert_eq!(lib.add_all(vec![c]), 1);
    }

    #[test]
    fn unreadable_file_is_not_saved_over() {
        let dir = tempfile::tempdir().unwrap();
        // A missing file is a fresh start, and can be saved.
        let missing = Library::load_from(&dir.path().join("feeds.json"));
        assert!(missing.load_error.is_none());
        missing.save_to(&dir.path().join("feeds.json")).unwrap();
        // A folder where the file should be can't be read, like a locked file.
        let blocked = dir.path().join("blocked.json");
        std::fs::create_dir(&blocked).unwrap();
        let mut lib = Library::load_from(&blocked);
        assert!(lib.load_error.is_some());
        lib.feeds.push(Feed::new("https://example.com/feed".into(), String::new(), String::new()));
        assert!(lib.save_to(&blocked).is_err());
        assert!(blocked.is_dir());
    }

    #[test]
    fn damaged_file_is_kept_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feeds.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let lib = Library::load_from(&path);
        assert!(lib.load_error.is_none(), "a fresh list can be saved");
        assert!(lib.load_notice.is_some(), "the user is told");
        assert!(!path.exists());
        let kept: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].to_string_lossy().starts_with("feeds.json.damaged-"));
    }

    #[test]
    fn round_trips_through_json() {
        let mut lib = Library::default();
        let mut feed = Feed::new("https://example.com/feed".into(), "Example".into(), String::new());
        feed.last_fetched = Some(Utc::now());
        feed.downloaded.insert("guid-1".into());
        lib.feeds.push(feed);
        let json = serde_json::to_string(&lib).unwrap();
        let back: Library = serde_json::from_str(&json).unwrap();
        assert_eq!(back.feeds[0].downloaded.len(), 1);
        assert_eq!(back.feeds[0].last_fetched, lib.feeds[0].last_fetched);
        assert!(!back.feeds[0].disabled);
        assert!(!json.contains("disabled"), "only disabled feeds say so");
    }

    #[test]
    fn disabled_is_read_and_kept() {
        let json = r#"{"feeds":[{"url":"https://example.com/feed","title":"","added":"2026-10-23T14:05:00Z","disabled":true}]}"#;
        let lib: Library = serde_json::from_str(json).unwrap();
        assert!(lib.feeds[0].disabled);
        assert!(serde_json::to_string(&lib).unwrap().contains(r#""disabled":true"#));
    }
}
