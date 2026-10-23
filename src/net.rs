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
    Ok(crate::text::decode(bytes))
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
    let total =
        response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
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

/// A web server for tests, so downloading can be tested without the
/// internet.
#[cfg(test)]
pub mod test_server {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serves `responses` in turn, one to each connection, and returns the
    /// server's address, such as `http://127.0.0.1:50000`. It stops after
    /// the last.
    pub fn serve(responses: Vec<Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for response in responses {
                let Ok((mut stream, _)) = listener.accept() else { return };
                // The request's headers; there is never a body.
                let mut request = Vec::new();
                let mut buf = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                // The client may have hung up, as a cancelled download does.
                let _ = stream.write_all(&response);
            }
        });
        format!("http://{addr}")
    }

    /// Serves the response given for each path asked for, such as
    /// "/feed.xml", and 404 Not Found for any other, to as many connections
    /// as come, one at a time. Returns the server's address.
    pub fn serve_paths(routes: Vec<(&str, Vec<u8>)>) -> String {
        let routes: Vec<(String, Vec<u8>)> = routes.into_iter().map(|(p, r)| (p.to_owned(), r)).collect();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut request = Vec::new();
                let mut buf = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                // "GET /path HTTP/1.1"
                let request = String::from_utf8_lossy(&request);
                let path = request.split_whitespace().nth(1).unwrap_or("");
                let not_found = response("404 Not Found", &[], b"");
                let reply = routes.iter().find(|(p, _)| p == path).map_or(&not_found, |(_, r)| r);
                let _ = stream.write_all(reply);
            }
        });
        format!("http://{addr}")
    }

    /// A response with `status`, such as "200 OK", the headers given, and
    /// `body`, whose length it gives unless the headers do.
    pub fn response(status: &str, headers: &[&str], body: &[u8]) -> Vec<u8> {
        let mut out = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
        if !headers.iter().any(|h| h.to_ascii_lowercase().starts_with("content-length:")) {
            out.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        for header in headers {
            out.push_str(&format!("{header}\r\n"));
        }
        out.push_str("\r\n");
        let mut out = out.into_bytes();
        out.extend_from_slice(body);
        out
    }

    pub fn ok(body: &[u8]) -> Vec<u8> {
        response("200 OK", &[], body)
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::{ok, response, serve};
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The names of the files in `dir`.
    fn files(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn downloads_through_redirects() {
        let body = vec![7u8; 600 * 1024];
        let server = serve(vec![response("302 Found", &["Location: /episode.mp3"], b""), ok(&body)]);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("episode.mp3");
        let mut last = (0, None);
        let cancel = AtomicBool::new(false);
        super::download(&format!("{server}/track?to=episode"), &dest, &cancel, |got, total| last = (got, total))
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(last, (body.len() as u64, Some(body.len() as u64)));
        assert_eq!(files(dir.path()), ["episode.mp3"], "no .part file is left");
    }

    #[test]
    fn a_download_that_ends_early_leaves_nothing() {
        // Says 1000 bytes, sends 500 and hangs up.
        let server = serve(vec![response("200 OK", &["Content-Length: 1000"], &[1u8; 500])]);
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let result = super::download(&format!("{server}/a.mp3"), &dir.path().join("a.mp3"), &cancel, |_, _| {});
        assert!(result.is_err());
        assert!(files(dir.path()).is_empty(), "{:?}", files(dir.path()));
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let server = serve(vec![response("404 Not Found", &[], b"Not here")]);
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let result = super::download(&format!("{server}/gone.mp3"), &dir.path().join("gone.mp3"), &cancel, |_, _| {});
        assert!(result.is_err());
        assert!(files(dir.path()).is_empty(), "the error page isn't saved as the episode");
    }

    #[test]
    fn cancelling_part_way_leaves_nothing() {
        let server = serve(vec![ok(&vec![0u8; 8 * 1024 * 1024])]);
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let result = super::download(&format!("{server}/big.mp3"), &dir.path().join("big.mp3"), &cancel, |_, _| {
            cancel.store(true, Ordering::Relaxed)
        });
        assert_eq!(result.unwrap_err().to_string(), "cancelled");
        assert!(files(dir.path()).is_empty());
    }

    #[test]
    fn feeds_are_read_as_text() {
        let server = serve(vec![ok("\u{feff}<rss>caf\u{e9}</rss>".as_bytes())]);
        assert_eq!(super::fetch_text(&format!("{server}/feed")).unwrap(), "<rss>caf\u{e9}</rss>");
    }

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
