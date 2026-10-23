//! The log file, `audioseek.log` in the app's config folder. Release builds
//! on Windows have no console, so this is the only place warnings, errors and
//! crashes are kept.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

/// When the log grows past this, it is moved to `audioseek.old.log` the next
/// time the app starts, so the two together never take much room.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

struct Logger {
    file: Mutex<File>,
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // The app's own messages, and only problems from the libraries,
        // which are chatty.
        if metadata.target().starts_with(env!("CARGO_CRATE_NAME")) {
            metadata.level() <= log::Level::Info
        } else {
            metadata.level() <= log::Level::Warn
        }
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}: {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            record.level(),
            record.target(),
            redact_urls(&record.args().to_string())
        );
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

/// Web addresses in `text` with anything that might be a password or key
/// taken out, as people attach the log to bug reports and private feeds
/// carry keys in their addresses: the user name and password, the query
/// and fragment, and any part of the path at least 20 characters long with
/// a digit in it, such as `/feeds/3f9a…`. What's left still says which
/// feed or file it was.
fn redact_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("http://").into_iter().chain(rest.find("https://")).min() {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail.find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>')).unwrap_or(tail.len());
        // Punctuation after an address, as in "could not fetch <url>: …",
        // belongs to the sentence.
        let url_len = tail[..end].trim_end_matches(['.', ',', ':', ';', ')']).len();
        let (url, after) = tail.split_at(url_len);
        out.push_str(&redact_url(url));
        rest = after;
    }
    out.push_str(rest);
    out
}

fn redact_url(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let hidden = rest.find(['?', '#']).is_some();
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let path: Vec<&str> = path
        .split('/')
        .map(|part| if part.len() >= 20 && part.contains(|c: char| c.is_ascii_digit()) { "…" } else { part })
        .collect();
    let mut out = format!("{scheme}://{host}{}", path.join("/"));
    if hidden {
        out.push_str("?…");
    }
    out
}

/// Starts writing the log, and logs any crash before it happens. If the log
/// can't be opened the app runs without one.
pub fn init() {
    let path = crate::paths::log_file();
    let opened = (|| {
        std::fs::create_dir_all(path.parent()?).ok()?;
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
            let _ = std::fs::rename(&path, crate::paths::old_log_file());
        }
        OpenOptions::new().create(true).append(true).open(&path).ok()
    })();
    let Some(file) = opened else {
        eprintln!("Could not open the log file {}", path.display());
        return;
    };
    if log::set_boxed_logger(Box::new(Logger { file: Mutex::new(file) })).is_err() {
        return;
    }
    log::set_max_level(log::LevelFilter::Info);

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        log::error!("crashed in thread {}: {info}", thread.name().unwrap_or("unnamed"));
        default_hook(info);
    }));

    log::info!(
        "audioseek {} started on {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

#[cfg(test)]
mod tests {
    use super::redact_urls;

    #[test]
    fn keys_are_kept_out_of_the_log() {
        assert_eq!(
            redact_urls("could not fetch https://www.patreon.com/rss/show?auth=abc123: 404"),
            "could not fetch https://www.patreon.com/rss/show?…: 404"
        );
        assert_eq!(
            redact_urls("https://show.supercast.com/feeds/a8f3b2c19d7e4f6a0b1c2d3e4f5a6b7c x"),
            "https://show.supercast.com/feeds/… x"
        );
        assert_eq!(redact_urls("http://me:secret@example.com/feed.xml"), "http://example.com/feed.xml");
        // Ordinary addresses, several to a line, are left alone.
        assert_eq!(
            redact_urls("a https://feeds.simplecast.com/54nAGcIl and http://b.example/ep/1.mp3."),
            "a https://feeds.simplecast.com/54nAGcIl and http://b.example/ep/1.mp3."
        );
        assert_eq!(redact_urls("no address here"), "no address here");
    }
}
