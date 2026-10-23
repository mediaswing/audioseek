//! Small helpers for text read from feeds and OPML files.

use quick_xml::events::BytesStart;

/// A downloaded or opened file's bytes as text. Feeds and OPML files are
/// nearly always UTF-8; anything else loses only odd characters. A leading
/// byte order mark is dropped.
pub fn decode(bytes: Vec<u8>) -> String {
    let mut text = String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    let bom = text.len() - text.trim_start_matches('\u{feff}').len();
    text.drain(..bom);
    text
}

/// `text` with each run of whitespace, line breaks included, made one space,
/// and none at either end.
pub fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The value of an XML element's attribute, with entities resolved.
pub fn attr(e: &BytesStart, name: &str) -> Option<String> {
    let a = e.try_get_attribute(name).ok()??;
    Some(a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding() {
        assert_eq!(decode("\u{feff}<rss/>".into()), "<rss/>");
        assert_eq!(decode(b"caf\xe9".to_vec()), "caf\u{fffd}");
        assert_eq!(decode(Vec::new()), "");
    }

    #[test]
    fn collapsing() {
        assert_eq!(collapse_whitespace("  One\n\t two  "), "One two");
    }
}
