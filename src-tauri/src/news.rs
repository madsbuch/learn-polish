//! Today's news in Polish, for news podcasts: the latest stories from the
//! RSS feeds of Polish news outlets, and each chosen story's own short
//! summary.
//!
//! A feed gives a headline, a link, when it was published, usually a picture,
//! and sometimes a summary. Where it has none, the summary the page gives of
//! itself (for link previews, even behind a paywall) adds a sentence or two.
//! That is all an episode is written from, besides Wikipedia for background:
//! the articles themselves belong to their outlets and are only linked to.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The country, as episodes name it.
pub const PLACE: &str = "Poland";

/// The feeds read, as (outlet, address). Several sections of one outlet
/// give a broader mix; the same story in two of them is told once. A feed
/// that does not answer is left out.
const FEEDS: &[(&str, &str)] = &[
    ("tvn24.pl", "https://tvn24.pl/najnowsze.xml"),
    ("polsatnews.pl", "https://www.polsatnews.pl/rss/wszystkie.xml"),
    ("rmf24.pl", "https://www.rmf24.pl/feed"),
];

/// How many headlines the writer chooses from, newest first.
const HEADLINES: usize = 120;
/// The most of a page read for its summary: the summary is in the head.
const PAGE_LIMIT: usize = 400_000;
/// A feed's "summary" shorter than this (or the headline again) is none.
const SUMMARY_MIN: usize = 40;

/// A news story, as its outlet's feed has it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Headline {
    pub title: String,
    pub url: String,
    /// The outlet, e.g. "tvn24.pl".
    pub outlet: String,
    /// When it was published: "2026-11-16".
    pub date: String,
    /// The story's picture, if it has one.
    pub image: String,
    /// A sentence or two on the story: the feed's, or the page's own.
    pub summary: String,
    /// When it was published, for putting the newest first (seconds since
    /// 1970, roughly: the zone is ignored).
    #[serde(skip)]
    pub sort_key: i64,
}

/// The latest stories from every feed, newest first, each once. The subject
/// (if any) is for the writer to choose by: feeds cannot be searched.
pub async fn headlines() -> Result<Vec<Headline>> {
    let pages = futures_util::future::join_all(FEEDS.iter().map(|(outlet, url)| async move {
        let response = crate::http::news().get(*url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body = response.text().await.ok()?;
        Some(parse_feed(&body, outlet))
    }))
    .await;
    if pages.iter().all(Option::is_none) {
        return Err(Error::msg("None of the news feeds answered. Check the connection and try again."));
    }
    Ok(merge(pages.into_iter().flatten().collect()))
}

/// Every feed's stories as one list: newest first, the same headline once.
fn merge(feeds: Vec<Vec<Headline>>) -> Vec<Headline> {
    let mut all: Vec<Headline> = feeds.into_iter().flatten().collect();
    all.sort_by(|a, b| b.sort_key.cmp(&a.sort_key));
    let mut seen: Vec<String> = Vec::new();
    all.retain(|h| {
        let key: String = h.title.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
        let fresh = !seen.contains(&key);
        if fresh {
            seen.push(key);
        }
        fresh && !h.url.is_empty()
    });
    // One story can be in two sections under one address.
    let mut urls: Vec<String> = Vec::new();
    all.retain(|h| {
        let fresh = !urls.contains(&h.url);
        urls.push(h.url.clone());
        fresh
    });
    all.truncate(HEADLINES);
    all
}

/// The stories in an RSS feed.
pub fn parse_feed(xml: &str, outlet: &str) -> Vec<Headline> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = xml[from..].find("<item") {
        let start = from + at;
        let Some(end) = xml[start..].find("</item>").map(|e| start + e) else { break };
        let item = &xml[start..end];
        from = end + 7;

        let title = crate::text::clean(&strip_tags(&decode_entities(&element(item, "title").unwrap_or_default())));
        let url = decode_entities(&element(item, "link").unwrap_or_default()).trim().to_string();
        if title.is_empty() || !url.starts_with("http") {
            continue;
        }
        let description = element(item, "description").map(|d| decode_entities(&d)).unwrap_or_default();
        let summary = crate::text::clean(&strip_tags(&description));
        let summary = if summary.chars().count() < SUMMARY_MIN || summary.starts_with(&title) {
            String::new()
        } else {
            summary.chars().take(600).collect()
        };
        let image = attribute_of(item, "media:content", "url")
            .or_else(|| attribute_of(item, "enclosure", "url"))
            .or_else(|| attribute_of(&description, "img", "src"))
            .map(|u| decode_entities(&u))
            .filter(|u| u.starts_with("https://"))
            .unwrap_or_default();
        let (date, sort_key) = element(item, "pubDate").and_then(|d| published(&d)).unwrap_or_default();
        out.push(Headline { title, url, outlet: outlet.to_string(), date, image, summary, sort_key });
    }
    out
}

/// An element's text in an item, without its CDATA wrapping.
fn element(item: &str, name: &str) -> Option<String> {
    let open = format!("<{name}");
    let mut at = 0;
    // `<title` must not match `<titleSomething`.
    let start = loop {
        let found = at + item[at..].find(&open)?;
        let next = item[found + open.len()..].chars().next()?;
        if next == '>' || next == ' ' || next == '\n' || next == '\t' {
            break found;
        }
        at = found + open.len();
    };
    let body = start + item[start..].find('>')? + 1;
    if item[..body].ends_with("/>") {
        return None;
    }
    let end = body + item[body..].find(&format!("</{name}>"))?;
    let text = item[body..end].trim();
    let text = text.strip_prefix("<![CDATA[").and_then(|t| t.strip_suffix("]]>")).unwrap_or(text);
    Some(text.trim().to_string())
}

/// An attribute of the first tag of that name: `<enclosure url="…">`.
fn attribute_of(text: &str, tag: &str, name: &str) -> Option<String> {
    let start = text.find(&format!("<{tag} "))?;
    let end = start + text[start..].find('>')?;
    let lower = text[start..end].to_ascii_lowercase();
    attribute(&text[start..end], &lower, name)
}

/// Text without its markup.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut inside = false;
    for c in html.chars() {
        match c {
            '<' => inside = true,
            '>' if inside => {
                inside = false;
                out.push(' ');
            }
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

/// "Mon, 16 Nov 2026 15:20:05 GMT" as "2026-11-16", and a key that orders
/// it (the zone is ignored: an hour either way does not matter here).
fn published(text: &str) -> Option<(String, i64)> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    // The weekday is optional.
    let parts = if parts.first()?.ends_with(',') { &parts[1..] } else { &parts[..] };
    let day: i64 = parts.first()?.parse().ok()?;
    let month = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]
        .iter()
        .position(|m| parts.get(1).is_some_and(|p| p.to_ascii_lowercase().starts_with(m)))
        .map(crate::convert::count)?
        + 1;
    let year: i64 = parts.get(2)?.parse().ok()?;
    let mut clock = parts.get(3).unwrap_or(&"0:0:0").split(':').map(|n| n.parse::<i64>().unwrap_or(0));
    let (h, m, s) = (clock.next().unwrap_or(0), clock.next().unwrap_or(0), clock.next().unwrap_or(0));
    let key = ((((year * 12 + month) * 31 + day) * 24 + h) * 60 + m) * 60 + s;
    Some((format!("{year}-{month:02}-{day:02}"), key))
}

/// The short summary a page gives of itself (for link previews), if it has
/// one and can be read in time.
pub async fn summary(url: &str) -> Option<String> {
    let response = crate::http::news()
        .get(url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    let page = String::from_utf8_lossy(&bytes[..bytes.len().min(PAGE_LIMIT)]).to_string();
    page_summary(&page)
}

/// The page's description: Open Graph's, else Twitter's, else the plain one.
pub fn page_summary(html: &str) -> Option<String> {
    // ASCII only, so positions in it are positions in the page.
    let lower = html.to_ascii_lowercase();
    let head = lower.split("</head>").next().unwrap_or(&lower);
    let mut found: [Option<String>; 3] = [None, None, None];
    let mut from = 0;
    while let Some(at) = head[from..].find("<meta") {
        let start = from + at;
        let end = head[start..].find('>').map(|e| start + e).unwrap_or(head.len());
        let tag = &head[start..end];
        let slot = if tag.contains("\"og:description\"") || tag.contains("'og:description'") {
            Some(0)
        } else if tag.contains("\"twitter:description\"") || tag.contains("'twitter:description'") {
            Some(1)
        } else if tag.contains("name=\"description\"") || tag.contains("name='description'") {
            Some(2)
        } else {
            None
        };
        if let Some(slot) = slot {
            // The value, from the original (not lowercased) text.
            if let Some(content) = attribute(&html[start..end], &tag, "content") {
                let text = crate::text::clean(&decode_entities(&content));
                if !text.is_empty() && found[slot].is_none() {
                    found[slot] = Some(text.chars().take(600).collect());
                }
            }
        }
        from = end;
    }
    found.into_iter().flatten().next()
}

/// An attribute's value in a tag; `lower` is the tag lowercased, to find it.
fn attribute(tag: &str, lower: &str, name: &str) -> Option<String> {
    let at = lower.find(&format!("{name}="))? + name.len() + 1;
    let quote = tag[at..].chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &tag[at + 1..];
    Some(rest[..rest.find(quote)?].to_string())
}

/// HTML entities as they turn up in headlines and summaries.
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let Some(end) = tail.char_indices().take(12).find(|(_, c)| *c == ';').map(|(i, _)| i) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let name = &tail[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "nbsp" => Some(' '),
            "aelig" => Some('æ'),
            "AElig" => Some('Æ'),
            "oslash" => Some('ø'),
            "Oslash" => Some('Ø'),
            "aring" => Some('å'),
            "Aring" => Some('Å'),
            "oacute" => Some('ó'),
            "Oacute" => Some('Ó'),
            "eacute" => Some('é'),
            "ndash" => Some('–'),
            "mdash" => Some('—'),
            "hellip" => Some('…'),
            "laquo" => Some('«'),
            "raquo" => Some('»'),
            "rsquo" | "lsquo" => Some('’'),
            "rdquo" | "ldquo" | "bdquo" => Some('”'),
            _ => {
                let number = name.strip_prefix('#').and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                });
                number.and_then(char::from_u32)
            }
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the real feeds: `cargo test live_feeds -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_feeds() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let found = runtime.block_on(headlines()).unwrap();
        for h in found.iter().take(8) {
            println!("{} | {} | {} | img={} | {}", h.date, h.outlet, h.title, !h.image.is_empty(), h.summary.chars().take(60).collect::<String>());
        }
        println!("{} stories", found.len());
        assert!(found.len() >= 20);
        let page = runtime.block_on(summary(&found[0].url));
        println!("page summary of the first: {page:?}");
    }

    #[test]
    fn feeds_give_their_stories_with_dates_pictures_and_summaries() {
        let feed = r#"<?xml version="1.0"?><rss><channel><title>TVN24</title>
            <item>
              <title><![CDATA[Rząd &amp; pogoda]]></title>
              <link>https://tvn24.pl/a</link>
              <pubDate>Mon, 16 Nov 2026 15:20:05 GMT</pubDate>
              <media:content url="https://tvn24.pl/a.jpg?im=x&amp;y=1" medium="image"/>
            </item>
            <item>
              <title>Nowy most w Krakowie</title>
              <description><![CDATA[<p><img src="https://x.pl/b.jpg" alt="" />Most ma być gotowy w 2030 roku, powiedział dziś minister.</p>]]></description>
              <link>https://polsatnews.pl/b</link>
              <pubDate>16 Nov 2026 16:01:00 +0100</pubDate>
            </item>
            <item><title>Bez linku</title></item>
            <item>
              <title>Krótko</title><link>https://rmf24.pl/c</link>
              <description>Krótko</description>
              <enclosure url="http://insecure/c.jpg" type="image/jpeg"/>
            </item>
          </channel></rss>"#;
        let found = parse_feed(feed, "tvn24.pl");
        let shape: Vec<(&str, &str, &str, &str)> =
            found.iter().map(|h| (h.title.as_str(), h.date.as_str(), h.image.as_str(), h.summary.as_str())).collect();
        assert_eq!(
            shape,
            [
                ("Rząd & pogoda", "2026-11-16", "https://tvn24.pl/a.jpg?im=x&y=1", ""),
                ("Nowy most w Krakowie", "2026-11-16", "https://x.pl/b.jpg", "Most ma być gotowy w 2030 roku, powiedział dziś minister."),
                ("Krótko", "", "", ""),
            ]
        );
        assert!(found[1].sort_key > found[0].sort_key);
    }

    #[test]
    fn stories_from_every_feed_are_merged_newest_first_once_each() {
        let story = |title: &str, url: &str, key: i64| Headline {
            title: title.into(),
            url: url.into(),
            sort_key: key,
            ..Default::default()
        };
        let merged = merge(vec![
            vec![story("Old news", "https://a/1", 1), story("Big story", "https://a/2", 5)],
            vec![story("Big story!", "https://b/2", 6), story("Other", "https://a/1", 3), story("Fresh", "https://b/3", 9)],
        ]);
        let titles: Vec<&str> = merged.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, ["Fresh", "Big story!", "Other"], "same headline once, same link once");
        assert_eq!(published("Mon, 16 Nov 2026 15:20:05 GMT").unwrap().0, "2026-11-16");
        assert_eq!(published("nonsense"), None);
    }

    #[test]
    fn a_page_describes_itself_in_its_head() {
        let page = r#"<html><head><title>x</title>
            <meta name="description" content="The plain one.">
            <meta content='Most &amp; tunel: otwarty dzi&#347; &ndash; wreszcie' property='og:description'/>
            </head><body><meta property="og:description" content="Not this one"></body></html>"#;
        assert_eq!(page_summary(page).as_deref(), Some("Most & tunel: otwarty dziś – wreszcie"));
        assert_eq!(page_summary(r#"<head><meta name="description" content="Tylko ten."></head>"#).as_deref(), Some("Tylko ten."));
        assert_eq!(page_summary("<head></head>"), None);
        assert_eq!(decode_entities("&AElig;bler &amp; &oslash;l &x; &#x2014;"), "Æbler & øl &x; —");
    }
}
