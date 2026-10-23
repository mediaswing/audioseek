//! Reading the feeds out of an OPML file, as podcast apps export them, and
//! writing the list back out as one.

use crate::library::Feed;
use crate::text::attr;
use anyhow::{Context, bail};
use quick_xml::escape::escape;
use quick_xml::events::Event;

/// Every outline with an `xmlUrl`, however deeply nested in folders.
pub fn parse(xml: &str) -> anyhow::Result<Vec<Feed>> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut feeds = Vec::new();
    let mut seen_opml = false;
    loop {
        match reader.read_event().context("the OPML file is damaged")? {
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                "opml" => seen_opml = true,
                "outline" => {
                    if let Some(url) = attr(&e, "xmlUrl").filter(|u| !u.trim().is_empty()) {
                        let title = attr(&e, "title").or_else(|| attr(&e, "text")).unwrap_or_default();
                        let website = attr(&e, "htmlUrl").unwrap_or_default();
                        feeds.push(Feed::new(url.trim().to_owned(), title.trim().to_owned(), website));
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    if !seen_opml {
        bail!("this is not an OPML file");
    }
    Ok(feeds)
}

/// An OPML file listing `feeds`, which podcast apps can import.
pub fn export(feeds: &[Feed]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"2.0\">\n");
    out.push_str("  <head>\n    <title>Podcast Batch Downloader feeds</title>\n");
    out.push_str(&format!("    <dateCreated>{}</dateCreated>\n", chrono::Utc::now().to_rfc2822()));
    out.push_str("  </head>\n  <body>\n");
    for feed in feeds {
        let title = escape(feed.name());
        out.push_str(&format!(
            "    <outline type=\"rss\" text=\"{title}\" title=\"{title}\" xmlUrl=\"{}\"",
            escape(feed.url.as_str())
        ));
        if !feed.website.is_empty() {
            out.push_str(&format!(" htmlUrl=\"{}\"", escape(feed.website.as_str())));
        }
        out.push_str("/>\n");
    }
    out.push_str("  </body>\n</opml>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<opml version="1.0"><head><title>Subs</title></head><body>
  <outline text="feeds">
    <outline type="rss" text="One &amp; Two" xmlUrl="https://a.example/feed.xml" htmlUrl="https://a.example"/>
    <outline type="rss" text="Folderless" xmlUrl=" https://b.example/rss "></outline>
  </outline>
  <outline text="No feed here"/>
</body></opml>"#;

    #[test]
    fn reads_nested_outlines() {
        let feeds = parse(SAMPLE).unwrap();
        assert_eq!(feeds.len(), 2);
        assert_eq!(feeds[0].title, "One & Two");
        assert_eq!(feeds[0].website, "https://a.example");
        assert_eq!(feeds[1].url, "https://b.example/rss");
    }

    #[test]
    fn export_reads_back() {
        let feeds = parse(SAMPLE).unwrap();
        let again = parse(&export(&feeds)).unwrap();
        assert_eq!(again.len(), 2);
        assert_eq!(again[0].title, "One & Two");
        assert_eq!(again[0].url, feeds[0].url);
    }

    #[test]
    fn rejects_other_xml() {
        assert!(parse("<rss><channel/></rss>").is_err());
    }
}
