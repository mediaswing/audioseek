//! Names for downloaded files and podcast folders.

use crate::feed::Episode;
use crate::settings::Naming;
use chrono::TimeZone;

/// How a publish time is written in a file name: sorts by date, and has no
/// colons, which Windows doesn't allow in file names.
const TIMESTAMP: &str = "%Y-%m-%d-%H-%M-%S";

/// Extensions kept from the audio address as they are.
const MEDIA_EXTENSIONS: [&str; 14] =
    ["mp3", "m4a", "m4b", "mp4", "aac", "ogg", "oga", "opus", "wav", "flac", "m4v", "mov", "webm", "mkv"];

/// The file name for `episode`, with its publish time in the computer's
/// own time zone.
pub fn file_name(episode: &Episode, naming: Naming) -> String {
    file_name_in(episode, naming, &chrono::Local)
}

/// As `file_name`, with the publish time in time zone `tz`.
pub fn file_name_in<Tz: TimeZone>(episode: &Episode, naming: Naming, tz: &Tz) -> String {
    let stamp = episode.published.map(|d| d.with_timezone(tz).naive_local().format(TIMESTAMP).to_string());
    let title = sanitize(&episode.title);
    let stem = match (naming, stamp) {
        (Naming::Timestamp, Some(stamp)) => stamp,
        (Naming::TimestampTitle, Some(stamp)) if !title.is_empty() => format!("{stamp} {title}"),
        (Naming::TimestampTitle, Some(stamp)) => stamp,
        // With no publish date, the title has to do.
        _ if !title.is_empty() => title,
        _ => "episode".to_owned(),
    };
    format!("{stem}.{}", extension(&episode.url, &episode.mime))
}

/// A podcast's folder name.
pub fn folder_name(title: &str) -> String {
    let name = sanitize(title);
    if name.is_empty() { "Podcast".to_owned() } else { name }
}

/// The file type, from the address if it names one, else from the MIME type.
fn extension(url: &str, mime: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.rsplit('/').next().unwrap_or(path);
    if let Some((_, ext)) = last.rsplit_once('.') {
        let ext = ext.to_ascii_lowercase();
        if let Some(known) = MEDIA_EXTENSIONS.iter().find(|e| **e == ext) {
            return known;
        }
    }
    match mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" | "audio/aac" => "m4a",
        "audio/ogg" => "ogg",
        "audio/opus" => "opus",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/flac" | "audio/x-flac" => "flac",
        "video/mp4" => "mp4",
        "video/x-m4v" => "m4v",
        "video/quicktime" => "mov",
        "video/webm" | "audio/webm" => "webm",
        _ => "mp3",
    }
}

/// Makes `text` safe as a file or folder name on Windows, macOS and Linux:
/// no characters they forbid, no trailing dots or spaces, no reserved device
/// names such as CON, and not too long.
pub fn sanitize(text: &str) -> String {
    const MAX_CHARS: usize = 120;
    let cleaned: String = text
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let mut name: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.chars().count() > MAX_CHARS {
        name = name.chars().take(MAX_CHARS).collect();
    }
    let name = name.trim_end_matches(['.', ' ']).trim_start_matches('.').to_owned();
    let device = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (device.len() == 4
            && (device.starts_with("COM") || device.starts_with("LPT"))
            && device.as_bytes()[3].is_ascii_digit());
    if reserved { format!("_{name}") } else { name }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, FixedOffset};

    fn episode(date: Option<&str>, title: &str, url: &str) -> Episode {
        Episode {
            id: url.to_owned(),
            title: title.to_owned(),
            published: date.map(|d| DateTime::parse_from_rfc3339(d).unwrap()),
            url: url.to_owned(),
            mime: String::new(),
            duration: String::new(),
        }
    }

    #[test]
    fn timestamp_is_in_the_local_zone() {
        let ep = episode(Some("2026-10-23T23:30:00+00:00"), "Late show", "https://x/a.mp3?id=1");
        let new_york = FixedOffset::west_opt(4 * 3600).unwrap();
        let tokyo = FixedOffset::east_opt(9 * 3600).unwrap();
        assert_eq!(file_name_in(&ep, Naming::Timestamp, &new_york), "2026-10-23-19-30-00.mp3");
        assert_eq!(file_name_in(&ep, Naming::Timestamp, &tokyo), "2026-10-24-08-30-00.mp3");
        assert_eq!(file_name_in(&ep, Naming::TimestampTitle, &tokyo), "2026-10-24-08-30-00 Late show.mp3");
        assert_eq!(file_name_in(&ep, Naming::Title, &tokyo), "Late show.mp3");
    }

    #[test]
    fn undated_episodes_use_the_title() {
        let ep = episode(None, "What? Why: <now>", "https://x/download");
        assert_eq!(file_name_in(&ep, Naming::Timestamp, &chrono::Utc), "What Why now.mp3");
    }

    #[test]
    fn extensions() {
        assert_eq!(extension("https://x/a.M4A", ""), "m4a");
        assert_eq!(extension("https://x/play/123", "audio/x-m4a"), "m4a");
        assert_eq!(extension("https://x/file.php?f=a.ogg", "audio/mpeg"), "mp3");
        assert_eq!(extension("https://x/a", ""), "mp3");
    }

    #[test]
    fn sanitizing() {
        assert_eq!(sanitize("  a/b\\c  "), "a b c");
        assert_eq!(sanitize("con"), "_con");
        assert_eq!(sanitize("COM1.txt"), "_COM1.txt");
        assert_eq!(sanitize("Ends with dots..."), "Ends with dots");
        assert_eq!(sanitize(&"x".repeat(300)).len(), 120);
    }
}
