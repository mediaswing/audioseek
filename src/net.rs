//! Fetching feeds and downloading episodes.

use anyhow::{Context, bail};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Feeds larger than this are refused, rather than filling memory.
const MAX_FEED_BYTES: u64 = 64 * 1024 * 1024;

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            // Podcast hosts redirect through tracking services, often
            // several times.
            .max_redirects(10)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            // A server that stops sending part way through fails the
            // download, rather than leaving it (and Cancel) waiting forever.
            .timeout_per_read(Some(Duration::from_secs(60)))
            .user_agent(concat!("audioseek/", env!("CARGO_PKG_VERSION")))
            .build()
            .into()
    })
}

/// The feed at `url`, as text.
pub fn fetch_text(url: &str) -> anyhow::Result<String> {
    let mut response = agent().get(url).call().with_context(|| format!("could not fetch {url}"))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_FEED_BYTES)
        .read_to_vec()
        .with_context(|| format!("could not read {url}"))?;
    // Feeds are nearly always UTF-8; anything else loses only odd characters.
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}

/// Downloads `url` to `dest`, calling `progress` with the bytes received
/// and the total, if known. The file is written beside `dest` with `.part`
/// added and renamed when complete, so a cancelled or failed download never
/// leaves a file that looks finished.
pub fn download(
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, Option<u64>),
) -> anyhow::Result<()> {
    let response = agent().get(url).call().with_context(|| format!("could not download {url}"))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = response.into_body().into_reader();
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let result = (|| {
        let mut file = std::fs::File::create(&part).with_context(|| format!("could not create {}", part.display()))?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut received = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            let n = reader.read(&mut buf).context("the download was interrupted")?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).with_context(|| format!("could not write {}", part.display()))?;
            received += n as u64;
            progress(received, total);
        }
        if let Some(total) = total
            && received < total
        {
            bail!("the download ended early");
        }
        file.sync_all()?;
        drop(file);
        std::fs::rename(&part, dest).with_context(|| format!("could not save {}", dest.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

#[cfg(test)]
mod tests {
    /// Fetches and reads some real feeds. Needs the internet, so it only runs
    /// with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_feeds() {
        for url in [
            "https://feeds.npr.org/510289/podcast.xml",
            "https://feeds.simplecast.com/54nAGcIl",
            "https://feeds.megaphone.fm/vergecast",
            "https://www.bbc.co.uk/programmes/p02nq0gn/episodes/downloads.rss",
        ] {
            let text = super::fetch_text(url).unwrap_or_else(|e| panic!("{url}: {e:#}"));
            let podcast = crate::feed::parse(&text).unwrap_or_else(|e| panic!("{url}: {e:#}"));
            let first = &podcast.episodes[0];
            println!(
                "{url}\n  {} — {} episodes\n  newest: {} | {:?} | {}\n  file: {}",
                podcast.title,
                podcast.episodes.len(),
                first.title,
                first.published,
                first.url,
                crate::naming::file_name(first, crate::settings::Naming::Timestamp),
            );
            assert!(podcast.episodes.iter().all(|e| e.published.is_some()), "{url}: undated episodes");
        }
    }

    /// Downloads a real episode through a chain of tracking redirects. Needs
    /// the internet: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_download() {
        let text = super::fetch_text("https://feeds.simplecast.com/54nAGcIl").unwrap();
        let episode = crate::feed::parse(&text).unwrap().episodes.remove(0);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join(crate::naming::file_name(&episode, crate::settings::Naming::Timestamp));
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut last = (0, None);
        super::download(&episode.url, &dest, &cancel, |got, total| last = (got, total)).unwrap();
        let size = std::fs::metadata(&dest).unwrap().len();
        println!("{} bytes, reported {:?}", size, last);
        assert!(size > 1_000_000);
        assert_eq!(Some(size), last.1);
    }
}
