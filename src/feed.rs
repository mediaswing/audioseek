//! Reading a podcast's episodes out of its RSS or Atom feed.

use crate::text::{attr, collapse_whitespace};
use anyhow::{Context, bail};
use chrono::{DateTime, FixedOffset, NaiveDateTime};
use quick_xml::events::Event;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Episode {
    /// The GUID, or the audio address when the feed gives none.
    pub id: String,
    pub title: String,
    pub published: Option<DateTime<FixedOffset>>,
    /// The audio (or video) file's address.
    pub url: String,
    /// Its MIME type, such as `audio/mpeg`, if given.
    pub mime: String,
    /// How long it is, as the feed writes it, such as "1:02:03".
    pub duration: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Podcast {
    pub title: String,
    pub website: String,
    /// Newest first.
    pub episodes: Vec<Episode>,
}

/// The text field being read.
#[derive(Clone, Copy, PartialEq)]
enum Field {
    Title,
    Link,
    Guid,
    Date,
    /// Atom's <updated>, used when there is no <published>.
    Updated,
    Duration,
}

/// Reads an RSS 2.0 or Atom feed. Items without an audio or video file
/// (blog posts, say) are left out.
pub fn parse(xml: &str) -> anyhow::Result<Podcast> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut podcast = Podcast::default();
    let mut is_feed = false;
    // Inside an <item> or <entry>, the episode so far.
    let mut item: Option<Episode> = None;
    let mut item_date = String::new();
    let mut item_updated = String::new();
    let mut field: Option<Field> = None;
    let mut text = String::new();
    // Depth of elements inside an item we don't read, such as <media:group>
    // with its own <title>, or <image><title> in the channel.
    let mut depth = 0usize;
    let mut skip_depth: Option<usize> = None;

    loop {
        let event = reader.read_event().context("the feed is damaged")?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(event, Event::Empty(_));
                let name = e.name();
                let qname = name.as_ref();
                let local = e.local_name();
                let local = local.as_ref();
                match local {
                    "rss" | "feed" | "RDF" => is_feed = true,
                    "item" | "entry" if skip_depth.is_none() => {
                        item = Some(Episode {
                            id: String::new(),
                            title: String::new(),
                            published: None,
                            url: String::new(),
                            mime: String::new(),
                            duration: String::new(),
                        });
                        item_date.clear();
                        item_updated.clear();
                    }
                    _ => {}
                }
                // Some feeds give the file only as <media:content>, often
                // inside a <media:group>, which is otherwise skipped for its
                // own <media:title>.
                if let Some(ep) = item.as_mut()
                    && local == "content"
                    && qname.starts_with("media:")
                    && ep.url.is_empty()
                {
                    ep.url = attr(e, "url").unwrap_or_default();
                    ep.mime = attr(e, "type").unwrap_or_default();
                }
                if skip_depth.is_none() {
                    match (item.as_mut(), local) {
                        (Some(ep), "enclosure") => {
                            if ep.url.is_empty() {
                                ep.url = attr(e, "url").unwrap_or_default();
                                ep.mime = attr(e, "type").unwrap_or_default();
                            }
                        }
                        // Atom: <link rel="enclosure" href="..." type="..."/>
                        (Some(ep), "link") if attr(e, "rel").as_deref() == Some("enclosure") => {
                            if ep.url.is_empty() {
                                ep.url = attr(e, "href").unwrap_or_default();
                                ep.mime = attr(e, "type").unwrap_or_default();
                            }
                        }
                        (Some(_), "group" | "image" | "author" | "source") if !empty => skip_depth = Some(depth),
                        (None, "image" | "author" | "owner" | "category") if !empty => skip_depth = Some(depth),
                        (Some(_), _) if !empty => {
                            field = match local {
                                "title" if !qname.contains(':') => Some(Field::Title),
                                "guid" | "id" => Some(Field::Guid),
                                "pubDate" | "published" | "date" => Some(Field::Date),
                                "updated" => Some(Field::Updated),
                                "duration" => Some(Field::Duration),
                                _ => None,
                            };
                            text.clear();
                        }
                        (None, _) if !empty => {
                            field = match local {
                                "title" if !qname.contains(':') => Some(Field::Title),
                                "link" if !qname.contains(':') => Some(Field::Link),
                                _ => None,
                            };
                            text.clear();
                            // Atom gives the web page as <link href=...>.
                            if local == "link"
                                && podcast.website.is_empty()
                                && let Some(href) = attr(e, "href")
                            {
                                podcast.website = href;
                            }
                        }
                        (None, "link")
                            if podcast.website.is_empty() && attr(e, "rel").is_none_or(|r| r == "alternate") =>
                        {
                            podcast.website = attr(e, "href").unwrap_or_default();
                        }
                        _ => {}
                    }
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::Text(t) if field.is_some() => text.push_str(&t.xml10_content()),
            Event::CData(t) if field.is_some() => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if field.is_some() => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else if let Some(s) = quick_xml::escape::resolve_html5_entity(r.as_ref()) {
                    text.push_str(s);
                }
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                if skip_depth == Some(depth) {
                    skip_depth = None;
                    field = None;
                    continue;
                }
                if skip_depth.is_some() {
                    continue;
                }
                let value = text.trim().to_owned();
                match (item.as_mut(), field.take()) {
                    (Some(ep), Some(Field::Title)) if ep.title.is_empty() => ep.title = collapse_whitespace(&value),
                    (Some(ep), Some(Field::Guid)) if ep.id.is_empty() => ep.id = value,
                    (Some(_), Some(Field::Date)) if item_date.is_empty() => item_date = value,
                    (Some(_), Some(Field::Updated)) if item_updated.is_empty() => item_updated = value,
                    (Some(ep), Some(Field::Duration)) => ep.duration = value,
                    (None, Some(Field::Title)) if podcast.title.is_empty() => {
                        podcast.title = collapse_whitespace(&value)
                    }
                    (None, Some(Field::Link)) if podcast.website.is_empty() => podcast.website = value,
                    _ => {}
                }
                text.clear();
                let local = e.local_name();
                if matches!(local.as_ref(), "item" | "entry")
                    && let Some(mut ep) = item.take()
                    && !ep.url.trim().is_empty()
                {
                    ep.url = ep.url.trim().to_owned();
                    ep.published = parse_date(&item_date).or_else(|| parse_date(&item_updated));
                    if ep.id.is_empty() {
                        ep.id = ep.url.clone();
                    }
                    if ep.title.is_empty() {
                        ep.title = "Untitled episode".to_owned();
                    }
                    podcast.episodes.push(ep);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !is_feed {
        bail!("this address is not a podcast feed");
    }
    // Some feeds give two items the same GUID. A repeat of an episode is
    // dropped; a different episode is told apart by its audio address.
    let mut seen: HashMap<String, String> = HashMap::new();
    podcast.episodes.retain_mut(|ep| {
        match seen.get(&ep.id) {
            None => {}
            Some(url) if *url == ep.url => return false,
            Some(_) => ep.id = format!("{} {}", ep.id, ep.url),
        }
        seen.insert(ep.id.clone(), ep.url.clone()).is_none()
    });
    // Newest first; episodes without a date go last, in feed order.
    podcast.episodes.sort_by_key(|e| std::cmp::Reverse(e.published));
    Ok(podcast)
}

/// Reads a publish date as feeds write it: RFC 2822 in RSS
/// ("Tue, 03 Oct 2023 10:00:00 +0000"), RFC 3339 in Atom, and some common
/// mistakes, such as the wrong day of the week or no time zone (taken as UTC).
pub fn parse_date(s: &str) -> Option<DateTime<FixedOffset>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(d) = DateTime::parse_from_rfc2822(s) {
        return Some(d);
    }
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(d);
    }
    // Drop the day of the week, which some feeds get wrong.
    let without_day = s.split_once(',').map(|(_, rest)| rest.trim()).unwrap_or(s);
    if let Ok(d) = DateTime::parse_from_rfc2822(without_day) {
        return Some(d);
    }
    for format in ["%d %b %Y %H:%M:%S %z", "%d %b %Y %H:%M %z", "%d %B %Y %H:%M:%S %z"] {
        if let Ok(d) = DateTime::parse_from_str(without_day, format) {
            return Some(d);
        }
    }
    let utc = FixedOffset::east_opt(0)?;
    for format in ["%d %b %Y %H:%M:%S", "%d %b %Y %H:%M", "%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
        // The text may end in a zone name chrono doesn't know, such as "PST8".
        let trimmed =
            without_day.rsplit_once(' ').filter(|_| format.matches(' ').count() < without_day.matches(' ').count());
        for candidate in [Some(without_day), trimmed.map(|(head, _)| head)].into_iter().flatten() {
            if let Ok(naive) = NaiveDateTime::parse_from_str(candidate, format) {
                return Some(naive.and_utc().with_timezone(&utc));
            }
        }
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(without_day, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc().with_timezone(&utc));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd" xmlns:media="http://search.yahoo.com/mrss/">
<channel>
  <title>Test &amp; Show</title>
  <link>https://show.example</link>
  <image><title>Logo title</title><url>https://show.example/logo.png</url></image>
  <itunes:title>Not the title</itunes:title>
  <itunes:image href="https://show.example/art.jpg"/>
  <itunes:category text="News"/>
  <item>
    <title><![CDATA[Episode <One>]]></title>
    <itunes:title>Itunes one</itunes:title>
    <itunes:image href="https://show.example/1.jpg"/>
    <guid isPermaLink="false">ep-1</guid>
    <pubDate>Tue, 03 Oct 2023 10:00:00 +0000</pubDate>
    <enclosure url="https://cdn.example/1.mp3" type="audio/mpeg" length="123"/>
    <itunes:duration>1:02:03</itunes:duration>
  </item>
  <item>
    <title>Blog post with no audio</title>
    <pubDate>Wed, 04 Oct 2023 10:00:00 +0000</pubDate>
  </item>
  <item>
    <title>Episode Two</title>
    <pubDate>Sun, 05 Nov 2023 18:30:00 GMT</pubDate>
    <media:content url="https://cdn.example/2.m4a" type="audio/mp4"/>
  </item>
  <item>
    <title>Episode Three</title>
    <pubDate>Sun, 01 Oct 2023 18:30:00 GMT</pubDate>
    <media:group>
      <media:title>Not the title</media:title>
      <media:content url="https://cdn.example/3.mp3" type="audio/mpeg"/>
    </media:group>
  </item>
</channel></rss>"#;

    #[test]
    fn reads_rss() {
        let p = parse(RSS).unwrap();
        assert_eq!(p.title, "Test & Show");
        assert_eq!(p.website, "https://show.example");
        assert_eq!(p.episodes.len(), 3);
        // Newest first.
        assert_eq!(p.episodes[0].title, "Episode Two");
        assert_eq!(p.episodes[0].id, "https://cdn.example/2.m4a");
        assert_eq!(p.episodes[1].title, "Episode <One>");
        assert_eq!(p.episodes[1].id, "ep-1");
        assert_eq!(p.episodes[1].duration, "1:02:03");
        assert_eq!(p.episodes[1].mime, "audio/mpeg");
        assert_eq!(p.episodes[2].title, "Episode Three");
        assert_eq!(p.episodes[2].url, "https://cdn.example/3.mp3");
    }

    #[test]
    fn reads_atom() {
        let atom = r#"<feed xmlns="http://www.w3.org/2005/Atom">
          <title>Atom Show</title><link href="https://atom.example/"/>
          <entry><title>First</title><id>tag:1</id><published>2024-01-02T03:04:05-05:00</published>
            <link rel="alternate" href="https://atom.example/1"/>
            <link rel="enclosure" href="https://atom.example/1.mp3" type="audio/mpeg"/></entry>
        </feed>"#;
        let p = parse(atom).unwrap();
        assert_eq!(p.title, "Atom Show");
        assert_eq!(p.website, "https://atom.example/");
        assert_eq!(p.episodes[0].url, "https://atom.example/1.mp3");
        assert_eq!(p.episodes[0].published.unwrap().to_rfc3339(), "2024-01-02T03:04:05-05:00");
    }

    #[test]
    fn repeated_guids() {
        let rss = r#"<rss><channel><title>Show</title>
          <item><title>One</title><guid>same</guid><enclosure url="https://x/1.mp3"/></item>
          <item><title>One again</title><guid>same</guid><enclosure url="https://x/1.mp3"/></item>
          <item><title>Two</title><guid>same</guid><enclosure url="https://x/2.mp3"/></item>
        </channel></rss>"#;
        let p = parse(rss).unwrap();
        let ids: Vec<&str> = p.episodes.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["same", "same https://x/2.mp3"]);
        assert_eq!(p.episodes[0].title, "One");
    }

    #[test]
    fn rejects_html() {
        assert!(parse("<html><body>Not found</body></html>").is_err());
    }

    #[test]
    fn dates() {
        let d = |s| parse_date(s).map(|d| d.to_rfc3339());
        assert_eq!(d("Tue, 03 Oct 2023 10:00:00 +0000").unwrap(), "2023-10-03T10:00:00+00:00");
        // Wrong day of the week.
        assert_eq!(d("Mon, 03 Oct 2023 10:00:00 +0000").unwrap(), "2023-10-03T10:00:00+00:00");
        assert_eq!(d("Tue, 03 Oct 2023 10:00:00 EDT").unwrap(), "2023-10-03T10:00:00-04:00");
        assert_eq!(d("03 Oct 2023 10:00 +0100").unwrap(), "2023-10-03T10:00:00+01:00");
        assert_eq!(d("2023-10-03 10:00:00").unwrap(), "2023-10-03T10:00:00+00:00");
        assert_eq!(d("2023-10-03").unwrap(), "2023-10-03T00:00:00+00:00");
        assert_eq!(d("soon"), None);
    }
}
