//! Checking GitHub for a newer release.

use anyhow::Context;
use serde::Deserialize;

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/mediaswing/audioseek/releases/latest";

/// The page the newest release's downloads are on. The address is fixed here
/// rather than taken from GitHub's reply, so only this page is ever opened.
pub const LATEST_RELEASE_PAGE: &str = "https://github.com/mediaswing/audioseek/releases/latest";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

/// The newest release's version, such as "1.2.0", if it is newer than this
/// one.
pub fn check() -> anyhow::Result<Option<String>> {
    let text = crate::net::fetch_text(LATEST_RELEASE_API)?;
    let release: Release = serde_json::from_str(&text).context("could not read GitHub's reply")?;
    let latest = release.tag_name.trim_start_matches('v');
    let newer = is_newer(latest, env!("CARGO_PKG_VERSION"))
        .with_context(|| format!("the newest release has an unexpected version: {}", release.tag_name))?;
    Ok(newer.then(|| latest.to_owned()))
}

/// Whether version `a` comes after `b`. Both are three numbers, such as 1.2.0.
fn is_newer(a: &str, b: &str) -> Option<bool> {
    Some(parse(a)? > parse(b)?)
}

fn parse(version: &str) -> Option<[u64; 3]> {
    let mut parts = version.split('.').map(|p| p.parse::<u64>().ok());
    let parsed = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(parsed)
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions() {
        assert_eq!(is_newer("1.0.1", "1.0.0"), Some(true));
        assert_eq!(is_newer("1.10.0", "1.9.9"), Some(true));
        assert_eq!(is_newer("2.0.0", "1.99.99"), Some(true));
        assert_eq!(is_newer("1.0.0", "1.0.0"), Some(false));
        assert_eq!(is_newer("0.9.0", "1.0.0"), Some(false));
        assert_eq!(is_newer("1.0", "1.0.0"), None);
        assert_eq!(is_newer("1.0.0-beta", "1.0.0"), None);
        assert_eq!(is_newer("1.0.0.1", "1.0.0"), None);
    }

    /// Asks GitHub for the newest release. Needs the internet:
    /// `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_check() {
        println!("{:?}", super::check().unwrap());
    }
}
