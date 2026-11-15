//! Wikipedia as the source of study blocks — culture especially: a lesson
//! on Kraków is written from the Kraków article, the article is kept with
//! the block (readable in the app, offline, in English and in Polish), and
//! every three months the lesson is checked against the article again.
//!
//! Wikipedia's APIs need no key. Articles are CC BY-SA: the reader shows
//! where each comes from.

use diesel::prelude::*;
use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
use crate::files;
use crate::schema::{blocks, wiki_sources};

/// How often a block's lesson is compared with its sources again.
pub const CHECK_EVERY_DAYS: i64 = 90;
/// The most of an article an AI reads at once (fact checks, the tutor).
const READ_LIMIT: usize = 12_000;

/// An article as fetched.
#[derive(Debug, Clone)]
pub struct Article {
    pub lang: String,
    pub title: String,
    pub url: String,
    pub pageid: i64,
    pub revid: i64,
    /// The lead section.
    pub summary: String,
    /// The whole article, as markdown.
    pub content: String,
    pub thumbnail: Option<String>,
    /// The same article's title on Polish Wikipedia.
    pub polish_title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub title: String,
    pub snippet: String,
}

fn api(lang: &str) -> Result<String> {
    if !(2..=3).contains(&lang.len()) || !lang.chars().all(|c| c.is_ascii_lowercase()) {
        return Err(Error::msg(format!("'{lang}' is not a Wikipedia language code.")));
    }
    Ok(format!("https://{lang}.wikipedia.org/w/api.php"))
}

async fn get(lang: &str, query: &[(&str, &str)]) -> Result<Value> {
    let response = crate::http::wikipedia()
        .get(api(lang)?)
        .query(&[("format", "json"), ("formatversion", "2")])
        .query(query)
        .send()
        .await
        .map_err(|e| Error::msg(format!("Could not reach Wikipedia: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(Error::msg(format!("Wikipedia answered {status}.")));
    }
    let body: Value = response.json().await.map_err(|_| Error::msg("Wikipedia sent something unreadable."))?;
    if let Some(info) = body.pointer("/error/info").and_then(Value::as_str) {
        return Err(Error::msg(format!("Wikipedia: {info}")));
    }
    Ok(body)
}

/// Finds articles. Snippets come as plain text.
pub async fn search(lang: &str, query: &str) -> Result<Vec<SearchHit>> {
    let body = get(lang, &[("action", "query"), ("list", "search"), ("srsearch", query), ("srlimit", "6")]).await?;
    Ok(body
        .pointer("/query/search")
        .and_then(Value::as_array)
        .map(|hits| {
            hits.iter()
                .map(|hit| SearchHit {
                    title: hit["title"].as_str().unwrap_or_default().to_string(),
                    snippet: strip_tags(hit["snippet"].as_str().unwrap_or_default()),
                })
                .collect()
        })
        .unwrap_or_default())
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut inside = false;
    for c in html.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out.replace("&quot;", "\"").replace("&amp;", "&").replace("&#039;", "'")
}

/// Wikipedia's plain text marks headings as `== History ==`; markdown
/// headings make them readable (and our renderer draws them).
pub fn to_markdown(plain: &str) -> String {
    let mut out = Vec::new();
    for line in plain.lines() {
        let trimmed = line.trim();
        let level = trimmed.chars().take_while(|c| *c == '=').count();
        if level >= 2 && trimmed.ends_with(&"=".repeat(level)) && trimmed.len() > level * 2 {
            let heading = trimmed[level..trimmed.len() - level].trim();
            if !heading.is_empty() {
                out.push(String::new());
                out.push(format!("{} {heading}", "#".repeat(level.min(4))));
                continue;
            }
        }
        out.push(line.to_string());
    }
    // Sections Wikipedia leaves empty (galleries, references) go.
    let text = out.join("\n");
    let mut cleaned: Vec<&str> = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with('#') {
            let next_text = lines[i + 1..].iter().find(|l| !l.trim().is_empty());
            if next_text.is_none_or(|l| l.starts_with('#') && heading_level(l) <= heading_level(line)) {
                continue;
            }
        }
        cleaned.push(line);
    }
    cleaned.join("\n").trim().to_string()
}

fn heading_level(line: &str) -> usize {
    line.chars().take_while(|c| *c == '#').count()
}

/// The lead: everything before the first heading.
pub fn lead(markdown: &str) -> String {
    markdown.split("\n#").next().unwrap_or_default().trim().to_string()
}

/// Fetches an article: its text, revision, address, picture, and the
/// title of its Polish version.
pub async fn fetch(lang: &str, title: &str) -> Result<Article> {
    let body = get(
        lang,
        &[
            ("action", "query"),
            ("titles", title),
            ("redirects", "1"),
            ("prop", "extracts|revisions|pageimages|langlinks|info"),
            ("explaintext", "1"),
            ("exsectionformat", "wiki"),
            ("rvprop", "ids"),
            ("piprop", "thumbnail"),
            ("pithumbsize", "640"),
            ("lllang", "pl"),
            ("inprop", "url"),
        ],
    )
    .await?;
    let page = body
        .pointer("/query/pages/0")
        .ok_or_else(|| Error::msg("Wikipedia sent no article."))?;
    if page.get("missing").and_then(Value::as_bool) == Some(true) {
        return Err(Error::msg(format!("There is no article called “{title}” on {lang}.wikipedia.org.")));
    }
    let content = to_markdown(page["extract"].as_str().unwrap_or_default());
    Ok(Article {
        lang: lang.to_string(),
        title: page["title"].as_str().unwrap_or(title).to_string(),
        url: page["fullurl"].as_str().map(str::to_string).unwrap_or_else(|| {
            format!("https://{lang}.wikipedia.org/wiki/{}", title.replace(' ', "_"))
        }),
        pageid: page["pageid"].as_i64().unwrap_or_default(),
        revid: page.pointer("/revisions/0/revid").and_then(Value::as_i64).unwrap_or_default(),
        summary: lead(&content),
        content,
        thumbnail: page.pointer("/thumbnail/source").and_then(Value::as_str).map(str::to_string),
        polish_title: page.pointer("/langlinks/0/title").and_then(Value::as_str).map(str::to_string),
    })
}

/// The article's current revision, without its text: is it still the same?
pub async fn current_revision(lang: &str, title: &str) -> Result<i64> {
    let body = get(lang, &[("action", "query"), ("titles", title), ("redirects", "1"), ("prop", "revisions"), ("rvprop", "ids")])
        .await?;
    body.pointer("/query/pages/0/revisions/0/revid")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::msg(format!("Wikipedia has no “{title}” any more.")))
}

// ---------------------------------------------------------------------------
// Sources of a block
// ---------------------------------------------------------------------------

/// A source as the block page shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: i64,
    pub block_id: i64,
    pub lang: String,
    pub title: String,
    pub url: String,
    pub summary: String,
    pub image_file_id: Option<i64>,
    /// Polish Wikipedia has it too.
    pub polish_title: Option<String>,
    pub checked_at: i64,
}

type SourceRow = (i64, i64, String, String, String, String, Option<i64>, Option<String>, i64);

fn source_from(row: SourceRow) -> Source {
    let (id, block_id, lang, title, url, summary, image_file_id, polish_title, checked_at) = row;
    Source { id, block_id, lang, title, url, summary, image_file_id, polish_title, checked_at }
}

pub fn sources_of(conn: &mut Conn, block_id: i64) -> Result<Vec<Source>> {
    Ok(wiki_sources::table
        .filter(wiki_sources::block_id.eq(block_id))
        .order((wiki_sources::ord, wiki_sources::id))
        .select((
            wiki_sources::id,
            wiki_sources::block_id,
            wiki_sources::lang,
            wiki_sources::title,
            wiki_sources::url,
            wiki_sources::summary,
            wiki_sources::image_file_id,
            wiki_sources::pl_title,
            wiki_sources::checked_at,
        ))
        .load::<SourceRow>(conn)?
        .into_iter()
        .map(source_from)
        .collect())
}

/// Stores an article as a source of a study block (replacing an earlier
/// copy of the same article), with its Polish version's text and its picture.
pub fn store(
    conn: &mut Conn,
    block_id: i64,
    article: &Article,
    polish: Option<&Article>,
    picture: Option<(String, Vec<u8>)>,
) -> Result<Source> {
    let kind: String = blocks::table
        .find(block_id)
        .select(blocks::kind)
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg("That block no longer exists."))?;
    if kind != "study" {
        return Err(Error::msg("Only study blocks take sources."));
    }
    let image = match picture {
        Some((mime, bytes)) => Some(files::store(
            conn,
            files::WIKI_IMAGE,
            &format!("wiki-{}-{}.{}", article.lang, article.pageid, files::extension_for(&mime)),
            &mime,
            &bytes,
        )?),
        None => None,
    };
    let now = db::now_secs();
    let ord: i64 = wiki_sources::table
        .filter(wiki_sources::block_id.eq(block_id))
        .count()
        .get_result(conn)?;
    let existing: Option<(i64, Option<i64>)> = wiki_sources::table
        .filter(wiki_sources::block_id.eq(block_id))
        .filter(wiki_sources::lang.eq(&article.lang))
        .filter(wiki_sources::pageid.eq(article.pageid))
        .select((wiki_sources::id, wiki_sources::image_file_id))
        .first(conn)
        .optional()?;
    // Fetched again without a picture (a fact check): the old one stays.
    let image = image.or(existing.and_then(|(_, old)| old));
    let values = (
        wiki_sources::title.eq(&article.title),
        wiki_sources::url.eq(&article.url),
        wiki_sources::revid.eq(article.revid),
        wiki_sources::summary.eq(&article.summary),
        wiki_sources::content.eq(&article.content),
        wiki_sources::image_file_id.eq(image),
        wiki_sources::pl_title.eq(polish.map(|p| p.title.clone())),
        wiki_sources::pl_url.eq(polish.map(|p| p.url.clone())),
        wiki_sources::pl_content.eq(polish.map(|p| p.content.clone())),
        wiki_sources::fetched_at.eq(now),
        wiki_sources::checked_at.eq(now),
    );
    let id = match existing {
        Some((id, old_image)) => {
            diesel::update(wiki_sources::table.find(id)).set(values).execute(conn)?;
            if let Some(old) = old_image.filter(|old| Some(*old) != image) {
                files::delete(conn, &[old])?;
            }
            id
        }
        None => diesel::insert_into(wiki_sources::table)
            .values((
                wiki_sources::block_id.eq(block_id),
                wiki_sources::ord.eq(ord),
                wiki_sources::lang.eq(&article.lang),
                wiki_sources::pageid.eq(article.pageid),
                values,
            ))
            .returning(wiki_sources::id)
            .get_result(conn)?,
    };
    Ok(sources_of(conn, block_id)?.into_iter().find(|s| s.id == id).expect("just stored"))
}

/// Fetches an article and attaches it to a study block: the tutor's
/// attach_source, for new and existing blocks alike.
pub async fn attach(db: &Db, block_id: i64, lang: &str, title: &str) -> Result<Source> {
    let article = fetch(lang, title).await?;
    // Reading practice: the Polish version, when there is one.
    let polish = match (&article.polish_title, lang) {
        (Some(pl), l) if l != "pl" => fetch("pl", pl).await.ok(),
        _ => None,
    };
    let picture = match &article.thumbnail {
        Some(url) => download_picture(url).await.ok(),
        None => None,
    };
    store(&mut db.lock(), block_id, &article, polish.as_ref(), picture)
}

async fn download_picture(url: &str) -> Result<(String, Vec<u8>)> {
    let response = crate::http::wikipedia().get(url).send().await?;
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .split(';')
        .next()
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = response.bytes().await?.to_vec();
    if bytes.is_empty() || !mime.starts_with("image/") {
        return Err(Error::msg("no picture"));
    }
    Ok((mime, bytes))
}

pub fn remove(conn: &mut Conn, source_id: i64) -> Result<()> {
    let image: Option<Option<i64>> =
        wiki_sources::table.find(source_id).select(wiki_sources::image_file_id).first(conn).optional()?;
    diesel::delete(wiki_sources::table.find(source_id)).execute(conn)?;
    files::delete(conn, &image.flatten().into_iter().collect::<Vec<_>>())?;
    Ok(())
}

/// What the examiner and the fact check read: each source's lead, and as
/// much of the rest as fits.
pub fn reading_for(conn: &mut Conn, block_id: i64, limit: usize) -> Result<String> {
    let rows: Vec<(String, String)> = wiki_sources::table
        .filter(wiki_sources::block_id.eq(block_id))
        .order(wiki_sources::ord)
        .select((wiki_sources::title, wiki_sources::content))
        .load(conn)?;
    if rows.is_empty() {
        return Ok(String::new());
    }
    let share = limit / rows.len();
    Ok(rows
        .into_iter()
        .map(|(title, content)| format!("### {title} (Wikipedia)\n{}", clip(&content, share)))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let cut: String = text.chars().take(limit).collect();
    format!("{}…", cut.rsplit_once('\n').map_or(cut.as_str(), |(head, _)| head))
}

// ---------------------------------------------------------------------------
// Fact checks
// ---------------------------------------------------------------------------

/// The outcome of checking a lesson against its sources.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactCheck {
    pub block_id: i64,
    pub title: String,
    /// The lesson was out of date and has been corrected.
    pub updated: bool,
    /// What changed, when it did.
    pub note: Option<String>,
}

/// Blocks whose sources have not been compared with Wikipedia for three months.
pub fn blocks_due(conn: &mut Conn, now: i64) -> Result<Vec<i64>> {
    let mut ids: Vec<i64> = wiki_sources::table
        .filter(wiki_sources::checked_at.lt(now - CHECK_EVERY_DAYS * 86_400))
        .select(wiki_sources::block_id)
        .load(conn)?;
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// At startup: checks every block that is due, in the background, and says
/// when a lesson was corrected (`facts://updated`).
pub fn check_due_in_background(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let due = blocks_due(&mut app.state::<Db>().lock(), db::now_secs()).unwrap_or_default();
    if due.is_empty() {
        return;
    }
    let busy = crate::work::begin(app, "Checking lessons against Wikipedia…");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        let db = app.state::<Db>();
        for block_id in due {
            match fact_check(&db, block_id, false).await {
                Ok(check) if check.updated => {
                    let _ = app.emit("facts://updated", check);
                }
                Ok(_) => {}
                Err(error) => eprintln!("fact check of block {block_id}: {error}"),
            }
        }
    });
}

/// Compares a block's sources with Wikipedia now. Articles that changed are
/// fetched again; if any did (or `force`), an AI reads the lesson against
/// them and corrects what is out of date — nothing else — noting what
/// changed. Unchanged articles cost nothing.
pub async fn fact_check(db: &Db, block_id: i64, force: bool) -> Result<FactCheck> {
    let (title, sources): (String, Vec<(i64, String, String, i64)>) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let title = blocks::table.find(block_id).select(blocks::title).first(conn)?;
        let sources = wiki_sources::table
            .filter(wiki_sources::block_id.eq(block_id))
            .select((wiki_sources::id, wiki_sources::lang, wiki_sources::title, wiki_sources::revid))
            .load(conn)?;
        (title, sources)
    };
    let mut changed = false;
    for (id, lang, article_title, revid) in &sources {
        let now = db::now_secs();
        if current_revision(lang, article_title).await? == *revid {
            diesel::update(wiki_sources::table.find(id))
                .set(wiki_sources::checked_at.eq(now))
                .execute(&mut *db.lock())?;
            continue;
        }
        changed = true;
        let article = fetch(lang, article_title).await?;
        let polish = match (&article.polish_title, lang.as_str()) {
            (Some(pl), l) if l != "pl" => fetch("pl", pl).await.ok(),
            _ => None,
        };
        store(&mut db.lock(), block_id, &article, polish.as_ref(), None)?;
    }
    let mut check = FactCheck { block_id, title, updated: false, note: None };
    if changed || force {
        if let Some(note) = correct_lesson(db, block_id).await? {
            check.updated = true;
            check.note = Some(note);
        }
    }
    diesel::update(blocks::table.find(block_id))
        .set(blocks::fact_checked_at.eq(db::now_secs()))
        .execute(&mut *db.lock())?;
    Ok(check)
}

/// Asks the model whether the lesson is out of date against its sources and,
/// if so, stores the corrected lesson. Returns what changed.
async fn correct_lesson(db: &Db, block_id: i64) -> Result<Option<String>> {
    let (key, model, lesson, objectives, reading) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let (lesson, objectives): (String, String) =
            blocks::table.find(block_id).select((blocks::lesson, blocks::objectives)).first(conn)?;
        (
            crate::ai::api_key(conn)?,
            crate::ai::require_model(conn)?,
            lesson,
            objectives,
            reading_for(conn, block_id, READ_LIMIT)?,
        )
    };
    if reading.is_empty() {
        return Ok(None);
    }
    let messages = vec![
        crate::ai::Wire::system(
            "You keep a short lesson in a Polish-learning app accurate against its sources. You \
             answer with JSON only.",
        ),
        crate::ai::Wire::user(format!(
            "The lesson:\n{lesson}\n\nWhat the learner should be able to do: {objectives}\n\n\
             The sources, as they read today:\n{reading}\n\n\
             Is anything in the lesson now wrong or out of date according to the sources — a figure, \
             a name, an officeholder, a date? Wording, style, the Polish teaching, and things the \
             sources do not mention are fine: do not touch those.\n\
             Answer {{\"outdated\": false}}, or {{\"outdated\": true, \"changes\": \"one or two \
             sentences: what was out of date and what it is now\", \"lesson\": \"the whole lesson, \
             corrected and otherwise unchanged, in the same markdown\"}}."
        )),
    ];
    let reply = crate::ai::complete_with(&key, &model, &messages, 0.1, 4000).await?;
    let mut guard = db.lock();
    let conn = &mut *guard;
    let _ = crate::costs::record_tokens(conn, crate::costs::FACT_CHECK, &model, reply.cost, "", reply.tokens);
    let verdict = parse_verdict(&reply.text)?;
    let Some((changes, corrected)) = verdict else { return Ok(None) };
    let today = crate::commands::civil_date(db::today(conn));
    let note = format!("{}-{:02}-{:02}: {changes}", today.0, today.1, today.2);
    diesel::update(blocks::table.find(block_id))
        .set((blocks::lesson.eq(corrected.trim()), blocks::fact_note.eq(&note)))
        .execute(conn)?;
    Ok(Some(note))
}

/// The model's verdict: None when the lesson stands, else what changed and
/// the corrected lesson.
fn parse_verdict(text: &str) -> Result<Option<(String, String)>> {
    let start = text.find('{').ok_or_else(|| Error::msg("The fact check gave no answer."))?;
    let end = text.rfind('}').ok_or_else(|| Error::msg("The fact check gave no answer."))?;
    let value: Value = serde_json::from_str(&text[start..=end])
        .map_err(|_| Error::msg("The fact check's answer was unreadable."))?;
    if value["outdated"].as_bool() != Some(true) {
        return Ok(None);
    }
    let changes = value["changes"].as_str().unwrap_or("Corrected against Wikipedia.").trim().to_string();
    match value["lesson"].as_str().map(str::trim).filter(|l| !l.is_empty()) {
        Some(lesson) => Ok(Some((changes, lesson.to_string()))),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn block_sources(db: State<'_, Db>, block_id: i64) -> Result<Vec<Source>> {
    sources_of(&mut db.lock(), block_id)
}

/// One source's article for the reader, in its own language or in Polish.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleView {
    pub lang: String,
    pub title: String,
    pub url: String,
    pub content: String,
}

#[tauri::command]
pub async fn source_article(db: State<'_, Db>, source_id: i64, polish: bool) -> Result<ArticleView> {
    let row: (String, String, String, String, Option<String>, Option<String>, Option<String>) = wiki_sources::table
        .find(source_id)
        .select((
            wiki_sources::lang,
            wiki_sources::title,
            wiki_sources::url,
            wiki_sources::content,
            wiki_sources::pl_title,
            wiki_sources::pl_url,
            wiki_sources::pl_content,
        ))
        .first(&mut *db.lock())
        .optional()?
        .ok_or_else(|| Error::msg("That source no longer exists."))?;
    let (lang, title, url, content, pl_title, pl_url, pl_content) = row;
    Ok(match (polish, pl_title, pl_url, pl_content) {
        (true, Some(title), Some(url), Some(content)) => ArticleView { lang: "pl".into(), title, url, content },
        _ => ArticleView { lang, title, url, content },
    })
}

/// Checks one block against its sources now, whatever the date.
#[tauri::command]
pub async fn fact_check_block(db: State<'_, Db>, block_id: i64) -> Result<FactCheck> {
    fact_check(&db, block_id, true).await
}

#[tauri::command]
pub async fn source_remove(db: State<'_, Db>, source_id: i64) -> Result<()> {
    remove(&mut db.lock(), source_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikipedia_text_becomes_markdown() {
        let plain = "Kraków is a city.\nIt is old.\n\n== History ==\nFounded early.\n\n=== Middle Ages ===\nA capital.\n\n== See also ==\n\n== References ==\n";
        let md = to_markdown(plain);
        assert!(md.contains("## History\nFounded early."), "{md}");
        assert!(md.contains("### Middle Ages"));
        assert!(!md.contains("See also") && !md.contains("References"), "empty sections go: {md}");
        assert_eq!(lead(&md), "Kraków is a city.\nIt is old.");
    }

    #[test]
    fn only_wikipedia_language_codes_make_an_address() {
        assert_eq!(api("pl").unwrap(), "https://pl.wikipedia.org/w/api.php");
        assert!(api("evil.example.com/").is_err());
        assert!(api("EN").is_err());
    }

    #[test]
    fn a_fact_check_verdict_is_read_from_the_reply() {
        assert_eq!(parse_verdict("```json\n{\"outdated\": false}\n```").unwrap(), None);
        let (changes, lesson) = parse_verdict(
            "Here: {\"outdated\": true, \"changes\": \"The population is now 804,000.\", \"lesson\": \"Kraków has 804,000 people.\"}",
        )
        .unwrap()
        .unwrap();
        assert!(changes.contains("804,000") && lesson.starts_with("Kraków"));
        assert!(parse_verdict("no json here").is_err());
        // A verdict without a corrected lesson changes nothing.
        assert_eq!(parse_verdict("{\"outdated\": true, \"changes\": \"x\"}").unwrap(), None);
    }

    #[test]
    fn search_snippets_lose_their_markup() {
        assert_eq!(strip_tags("the <span class=\"searchmatch\">Kraków</span> &quot;old&quot; town"), "the Kraków \"old\" town");
    }
}
