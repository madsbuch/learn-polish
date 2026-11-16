//! Today's news in Polish, for news podcasts: headlines from GDELT's free
//! news index (no key), and each chosen story's own short summary, read from
//! the page it points to.
//!
//! GDELT gives a headline, a link, the outlet, when it was seen and a
//! picture — not the article. The summary publishers put in every page (for
//! link previews, even behind a paywall) adds a sentence or two. That is all
//! an episode is written from, besides Wikipedia for background: the articles
//! themselves belong to their publishers and are only linked to.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

/// The country, as episodes name it.
pub const PLACE: &str = "Poland";
/// GDELT's names for where the news comes from and the language it is in.
const COUNTRY: &str = "poland";
const LANGUAGE: &str = "polish";
/// If GDELT will not take a query of filters alone: words nearly every
/// Polish news story contains.
const FALLBACK_TERMS: &str = r#"("polska" OR "polski" OR "polskie")"#;

const API: &str = "https://api.gdeltproject.org/api/v2/doc/doc";
/// How many headlines the writer chooses from.
const HEADLINES: usize = 75;
/// The most of a page read for its summary: the summary is in the head.
const PAGE_LIMIT: usize = 400_000;

/// A news story's headline, as GDELT has it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Headline {
    pub title: String,
    pub url: String,
    /// The outlet's address, e.g. "pap.pl".
    pub outlet: String,
    /// When GDELT saw it: "2026-11-16".
    pub date: String,
    /// The story's picture, if it has one.
    pub image: String,
    /// The page's own short summary, once read.
    pub summary: String,
}

/// GDELT's search: the outlet's country and language, and the subject's
/// words when there is a subject. A subject in several words is a phrase.
fn query(subject: &str, fallback: bool) -> String {
    let mut parts = vec![format!("sourcecountry:{COUNTRY}"), format!("sourcelang:{LANGUAGE}")];
    let subject = subject.trim().replace('"', "");
    if !subject.is_empty() {
        parts.push(if subject.contains(' ') { format!("\"{subject}\"") } else { subject });
    } else if fallback {
        parts.push(FALLBACK_TERMS.to_string());
    }
    parts.join(" ")
}

/// The latest headlines from Polish outlets, about `subject` if one is given
/// (over the last week then; else the last day).
pub async fn headlines(subject: &str) -> Result<Vec<Headline>> {
    let timespan = if subject.trim().is_empty() { "1d" } else { "7d" };
    match fetch_headlines(&query(subject, false), timespan).await {
        Ok(found) if !found.is_empty() => Ok(found),
        // Filters alone may be refused, or find nothing: try with words.
        _ if subject.trim().is_empty() => fetch_headlines(&query(subject, true), timespan).await,
        other => other,
    }
}

async fn fetch_headlines(query: &str, timespan: &str) -> Result<Vec<Headline>> {
    let max = HEADLINES.to_string();
    let request = || {
        crate::http::news().get(API).query(&[
            ("query", query),
            ("mode", "artlist"),
            ("format", "json"),
            ("maxrecords", max.as_str()),
            ("timespan", timespan),
            ("sort", "hybridrel"),
        ])
    };
    let unreachable = |e: reqwest::Error| Error::msg(format!("Could not reach the news index (GDELT): {e}"));
    let mut body = request().send().await.map_err(unreachable)?.text().await.map_err(unreachable)?;
    // GDELT asks for a pause between requests, and says so in plain text.
    if body.to_lowercase().contains("limit requests") {
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        body = request().send().await.map_err(unreachable)?.text().await.map_err(unreachable)?;
    }
    parse_headlines(&body)
}

/// GDELT's answer: JSON with the articles, or (for a query it will not
/// take) a sentence saying why.
pub fn parse_headlines(body: &str) -> Result<Vec<Headline>> {
    let trimmed = body.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(trimmed).map_err(|_| {
        Error::msg(format!("The news index said: {}", trimmed.chars().take(200).collect::<String>()))
    })?;
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for article in value["articles"].as_array().into_iter().flatten() {
        let title = crate::text::clean(&decode_entities(article["title"].as_str().unwrap_or_default()));
        let url = article["url"].as_str().unwrap_or_default().to_string();
        // The same story from two outlets, or twice from one: once.
        let key: String = title.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
        if title.is_empty() || !url.starts_with("http") || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let seen_at = article["seendate"].as_str().unwrap_or_default();
        let date = if seen_at.len() >= 8 && seen_at.is_ascii() {
            format!("{}-{}-{}", &seen_at[0..4], &seen_at[4..6], &seen_at[6..8])
        } else {
            String::new()
        };
        out.push(Headline {
            title,
            url,
            outlet: article["domain"].as_str().unwrap_or_default().to_string(),
            date,
            image: article["socialimage"].as_str().filter(|s| s.starts_with("https://")).unwrap_or_default().to_string(),
            summary: String::new(),
        });
    }
    Ok(out)
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

    #[test]
    fn the_query_asks_for_polish_outlets_and_the_subject() {
        assert_eq!(query("", false), format!("sourcecountry:{COUNTRY} sourcelang:{LANGUAGE}"));
        assert!(query("", true).ends_with(FALLBACK_TERMS));
        assert!(query("rowery", false).ends_with(" rowery"));
        assert!(query("the \"royal\" family", false).ends_with(" \"the royal family\""), "{}", query("the \"royal\" family", false));
    }

    #[test]
    fn headlines_are_read_once_each_with_their_date() {
        let body = r#"{"articles": [
            {"url": "https://www.dr.dk/nyheder/a", "title": "Rząd &amp; pogoda", "seendate": "20261116T081500Z",
             "socialimage": "https://www.dr.dk/a.jpg", "domain": "dr.dk", "language": "Polish", "sourcecountry": "Poland"},
            {"url": "https://tv2.dk/a", "title": "Rząd & pogoda!", "seendate": "20261116T090000Z", "domain": "tv2.dk"},
            {"url": "not a link", "title": "Broken", "seendate": ""},
            {"url": "https://politiken.dk/b", "title": "Nowy most w Krakowie", "seendate": "20261115T120000Z",
             "socialimage": "http://insecure/x.jpg", "domain": "politiken.dk"}
        ]}"#;
        let found = parse_headlines(body).unwrap();
        let shape: Vec<(&str, &str, &str, &str)> =
            found.iter().map(|h| (h.title.as_str(), h.outlet.as_str(), h.date.as_str(), h.image.as_str())).collect();
        assert_eq!(
            shape,
            [
                ("Rząd & pogoda", "dr.dk", "2026-11-16", "https://www.dr.dk/a.jpg"),
                ("Nowy most w Krakowie", "politiken.dk", "2026-11-15", ""),
            ]
        );
        assert!(parse_headlines("{}").unwrap().is_empty());
        let refused = parse_headlines("Your search contained no keywords.").unwrap_err();
        assert!(refused.to_string().contains("no keywords"));
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
