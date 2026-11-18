//! Wikipedia as the source of study lessons — culture especially: a lesson
//! on Kraków is written from the Kraków article, the article is kept with
//! the lesson (readable in the app, offline, in English and in Polish), and
//! every three months the lesson is checked against the article again.
//!
//! Wikipedia's APIs need no key. Articles are CC BY-SA: the reader shows
//! where each comes from.

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
use crate::files;
use crate::schema::{lessons, songs, wiki_sources};
#[allow(unused_imports)]
use crate::ids::*;

/// How often a lesson's lesson is compared with its sources again.
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
    /// The article as Wikipedia lays it out (HTML), for the reader; fetched
    /// only by `fetch_with_layout`.
    pub html: Option<String>,
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

// What we read of Wikipedia's API answers (format=json, formatversion=2).

#[derive(Deserialize)]
struct ApiError {
    #[serde(default)]
    info: String,
}

/// Any answer: an error, if there was one, and the rest as `T`.
#[derive(Deserialize)]
struct Answer<T> {
    #[serde(default)]
    error: Option<ApiError>,
    #[serde(flatten)]
    body: T,
}

#[derive(Deserialize, Default)]
struct SearchBody {
    #[serde(default)]
    query: Option<SearchQuery>,
}

#[derive(Deserialize, Default)]
struct SearchQuery {
    #[serde(default)]
    search: Vec<RawHit>,
}

#[derive(Deserialize)]
struct RawHit {
    #[serde(default)]
    title: String,
    #[serde(default)]
    snippet: String,
}

#[derive(Deserialize, Default)]
struct PagesBody {
    #[serde(default)]
    query: Option<PagesQuery>,
}

#[derive(Deserialize, Default)]
struct PagesQuery {
    #[serde(default)]
    pages: Vec<RawPage>,
}

#[derive(Deserialize, Default)]
struct RawPage {
    #[serde(default)]
    missing: bool,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    extract: Option<String>,
    #[serde(default)]
    fullurl: Option<String>,
    #[serde(default)]
    pageid: i64,
    #[serde(default)]
    revisions: Vec<RawRevision>,
    #[serde(default)]
    thumbnail: Option<RawThumbnail>,
    #[serde(default)]
    langlinks: Vec<RawLanglink>,
}

#[derive(Deserialize)]
struct RawRevision {
    #[serde(default)]
    revid: i64,
}

#[derive(Deserialize)]
struct RawThumbnail {
    source: String,
}

#[derive(Deserialize)]
struct RawLanglink {
    title: String,
}

#[derive(Deserialize, Default)]
struct ParseBody {
    #[serde(default)]
    parse: Option<ParsedPage>,
}

#[derive(Deserialize)]
struct ParsedPage {
    #[serde(default)]
    text: String,
}

async fn get<T: serde::de::DeserializeOwned>(lang: &str, query: &[(&str, &str)]) -> Result<T> {
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
    let answer: Answer<T> = response.json().await.map_err(|_| Error::msg("Wikipedia sent something unreadable."))?;
    if let Some(error) = answer.error {
        return Err(Error::msg(format!("Wikipedia: {}", error.info)));
    }
    Ok(answer.body)
}

/// Finds articles. Snippets come as plain text.
pub async fn search(lang: &str, query: &str) -> Result<Vec<SearchHit>> {
    let body: SearchBody =
        get(lang, &[("action", "query"), ("list", "search"), ("srsearch", query), ("srlimit", "6")]).await?;
    Ok(body
        .query
        .unwrap_or_default()
        .search
        .into_iter()
        .map(|hit| SearchHit { title: hit.title, snippet: strip_tags(&hit.snippet) })
        .collect())
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
    let body: PagesBody = get(
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
        .query
        .and_then(|q| q.pages.into_iter().next())
        .ok_or_else(|| Error::msg("Wikipedia sent no article."))?;
    if page.missing {
        return Err(Error::msg(format!("There is no article called “{title}” on {lang}.wikipedia.org.")));
    }
    let content = to_markdown(page.extract.as_deref().unwrap_or_default());
    Ok(Article {
        lang: lang.to_string(),
        title: page.title.unwrap_or_else(|| title.to_string()),
        url: page
            .fullurl
            .unwrap_or_else(|| format!("https://{lang}.wikipedia.org/wiki/{}", title.replace(' ', "_"))),
        pageid: page.pageid,
        revid: page.revisions.first().map(|r| r.revid).unwrap_or_default(),
        summary: lead(&content),
        content,
        thumbnail: page.thumbnail.map(|t| t.source),
        polish_title: page.langlinks.into_iter().next().map(|l| l.title),
        html: None,
    })
}

/// The article as Wikipedia lays it out: its HTML, with sections, lists,
/// tables, the infobox and figures. The reader turns it into the app's own
/// elements (it is never inserted as HTML).
pub async fn fetch_html(lang: &str, title: &str) -> Result<String> {
    let body: ParseBody = get(
        lang,
        &[
            ("action", "parse"),
            ("page", title),
            ("redirects", "1"),
            ("prop", "text"),
            ("disableeditsection", "1"),
            ("disabletoc", "1"),
        ],
    )
    .await?;
    body.parse
        .map(|p| p.text)
        .filter(|html| !html.trim().is_empty())
        .ok_or_else(|| Error::msg("Wikipedia sent the article without its text."))
}

/// An article with its layout. The layout is a nicety: without it the reader
/// shows the plain text.
pub async fn fetch_with_layout(lang: &str, title: &str) -> Result<Article> {
    let mut article = fetch(lang, title).await?;
    article.html = fetch_html(lang, &article.title).await.ok();
    Ok(article)
}

/// The article's current revision, without its text: is it still the same?
pub async fn current_revision(lang: &str, title: &str) -> Result<i64> {
    let body: PagesBody =
        get(lang, &[("action", "query"), ("titles", title), ("redirects", "1"), ("prop", "revisions"), ("rvprop", "ids")])
            .await?;
    body.query
        .and_then(|q| q.pages.into_iter().next())
        .and_then(|page| page.revisions.first().map(|r| r.revid))
        .ok_or_else(|| Error::msg(format!("Wikipedia has no “{title}” any more.")))
}

// ---------------------------------------------------------------------------
// Sources of a lesson
// ---------------------------------------------------------------------------

/// Whose source it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Owner {
    /// A study lesson, which is written from it and checked against it.
    Lesson(LessonId),
    /// A podcast episode, which is written from it.
    Song(SongId),
}

/// A source as a lesson's or an episode's page shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: SourceId,
    pub lesson_id: Option<LessonId>,
    pub song_id: Option<SongId>,
    pub lang: String,
    pub title: String,
    pub url: String,
    pub summary: String,
    pub image_file_id: Option<FileId>,
    /// Polish Wikipedia has it too.
    pub polish_title: Option<String>,
    pub checked_at: i64,
}

type SourceRow = (SourceId, Option<LessonId>, Option<SongId>, String, String, String, String, Option<FileId>, Option<String>, i64);

fn source_from(row: SourceRow) -> Source {
    let (id, lesson_id, song_id, lang, title, url, summary, image_file_id, polish_title, checked_at) = row;
    Source { id, lesson_id, song_id, lang, title, url, summary, image_file_id, polish_title, checked_at }
}

fn owned_by(conn: &mut Conn, owner: Owner) -> Result<Vec<Source>> {
    let query = wiki_sources::table
        .order((wiki_sources::ord, wiki_sources::id))
        .select((
            wiki_sources::id,
            wiki_sources::lesson_id,
            wiki_sources::song_id,
            wiki_sources::lang,
            wiki_sources::title,
            wiki_sources::url,
            wiki_sources::summary,
            wiki_sources::image_file_id,
            wiki_sources::pl_title,
            wiki_sources::checked_at,
        ))
        .into_boxed();
    let query = match owner {
        Owner::Lesson(id) => query.filter(wiki_sources::lesson_id.eq(id)),
        Owner::Song(id) => query.filter(wiki_sources::song_id.eq(id)),
    };
    Ok(query.load::<SourceRow>(conn)?.into_iter().map(source_from).collect())
}

pub fn sources_of(conn: &mut Conn, lesson_id: LessonId) -> Result<Vec<Source>> {
    owned_by(conn, Owner::Lesson(lesson_id))
}

pub fn sources_of_song(conn: &mut Conn, song_id: SongId) -> Result<Vec<Source>> {
    owned_by(conn, Owner::Song(song_id))
}

/// Stores an article as a source of a study lesson (replacing an earlier
/// copy of the same article), with its Polish version's text and its picture.
pub fn store(
    conn: &mut Conn,
    lesson_id: LessonId,
    article: &Article,
    polish: Option<&Article>,
    picture: Option<(String, Vec<u8>)>,
) -> Result<Source> {
    store_for(conn, Owner::Lesson(lesson_id), article, polish, picture)
}

/// Stores an article as a source of a lesson or an episode.
pub fn store_for(
    conn: &mut Conn,
    owner: Owner,
    article: &Article,
    polish: Option<&Article>,
    picture: Option<(String, Vec<u8>)>,
) -> Result<Source> {
    let (lesson_id, song_id) = match owner {
        Owner::Lesson(id) => {
            let kind: String = lessons::table
                .find(id)
                .select(lessons::lesson_type)
                .first(conn)
                .optional()?
                .ok_or_else(|| Error::msg("That lesson no longer exists."))?;
            if kind != "study" {
                return Err(Error::msg("Only study lessons take sources."));
            }
            (Some(id), None)
        }
        Owner::Song(id) => {
            songs::table
                .find(id)
                .select(songs::id)
                .first::<SongId>(conn)
                .optional()?
                .ok_or_else(|| Error::msg("That podcast no longer exists."))?;
            (None, Some(id))
        }
    };
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
    let siblings = owned_by(conn, owner)?;
    let ord = crate::convert::count(siblings.len());
    let existing: Option<(SourceId, Option<FileId>)> = wiki_sources::table
        .filter(wiki_sources::lesson_id.is(lesson_id))
        .filter(wiki_sources::song_id.is(song_id))
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
                wiki_sources::id.eq(SourceId::new()),
                wiki_sources::lesson_id.eq(lesson_id),
                wiki_sources::song_id.eq(song_id),
                wiki_sources::ord.eq(ord),
                wiki_sources::lang.eq(&article.lang),
                wiki_sources::pageid.eq(article.pageid),
                values,
            ))
            .returning(wiki_sources::id)
            .get_result(conn)?,
    };
    // The layout is kept unless a new one came with the article.
    if let Some(html) = &article.html {
        diesel::update(wiki_sources::table.find(id)).set(wiki_sources::html.eq(html)).execute(conn)?;
    }
    if let Some(html) = polish.and_then(|p| p.html.as_ref()) {
        diesel::update(wiki_sources::table.find(id)).set(wiki_sources::pl_html.eq(html)).execute(conn)?;
    }
    Ok(owned_by(conn, owner)?.into_iter().find(|s| s.id == id).expect("just stored"))
}

/// Fetches an article and attaches it to a study lesson: the tutor's
/// attach_source, for new and existing lessons alike.
pub async fn attach(db: &Db, lesson_id: LessonId, lang: &str, title: &str) -> Result<Source> {
    attach_to(db, Owner::Lesson(lesson_id), lang, title).await
}

/// Fetches an article with its layout, its Polish version and its picture,
/// and keeps it as a lesson's or an episode's source.
pub async fn attach_to(db: &Db, owner: Owner, lang: &str, title: &str) -> Result<Source> {
    let article = fetch_with_layout(lang, title).await?;
    // Reading practice: the Polish version, when there is one.
    let polish = match (&article.polish_title, lang) {
        (Some(pl), l) if l != "pl" => fetch_with_layout("pl", pl).await.ok(),
        _ => None,
    };
    let picture = match &article.thumbnail {
        Some(url) => download_picture(url).await.ok(),
        None => None,
    };
    store_for(&mut db.lock(), owner, &article, polish.as_ref(), picture)
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

pub fn remove(conn: &mut Conn, source_id: SourceId) -> Result<()> {
    let image: Option<Option<FileId>> =
        wiki_sources::table.find(source_id).select(wiki_sources::image_file_id).first(conn).optional()?;
    diesel::delete(wiki_sources::table.find(source_id)).execute(conn)?;
    files::delete(conn, &image.flatten().into_iter().collect::<Vec<_>>())?;
    Ok(())
}

/// What the examiner and the fact check read: each source's lead, and as
/// much of the rest as fits.
pub fn reading_for(conn: &mut Conn, lesson_id: LessonId, limit: usize) -> Result<String> {
    let rows: Vec<(String, String)> = wiki_sources::table
        .filter(wiki_sources::lesson_id.eq(lesson_id))
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
    pub lesson_id: LessonId,
    pub title: String,
    /// The lesson was out of date and has been corrected.
    pub updated: bool,
    /// What changed, when it did.
    pub note: Option<String>,
}

/// Lessons whose sources have not been compared with Wikipedia for three months.
///
/// Only sources whose current version this device wrote
/// (`sync::written_here`): it fetched them, or checked them last. With the
/// learner's data on two devices, one checks each lesson — both would pay
/// for the same AI read and could correct the lesson twice.
pub fn lessons_due(conn: &mut Conn, now: i64) -> Result<Vec<LessonId>> {
    // Only lessons are checked: an episode is recorded once, as it was written.
    let due: Vec<(SourceId, Option<LessonId>)> = wiki_sources::table
        .filter(wiki_sources::checked_at.lt(now - CHECK_EVERY_DAYS * 86_400))
        .select((wiki_sources::id, wiki_sources::lesson_id))
        .load(conn)?;
    let mut ids: Vec<LessonId> = due
        .into_iter()
        .filter(|(source, _)| crate::sync::written_here(conn, "wiki_sources", &source.to_string()))
        .filter_map(|(_, lesson)| lesson)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// At startup: checks every lesson that is due, in the background, and says
/// when a lesson was corrected (`facts://updated`).
pub fn check_due_in_background(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let due = lessons_due(&mut app.state::<Db>().lock(), db::now_secs()).unwrap_or_default();
    if due.is_empty() {
        return;
    }
    let busy = crate::work::begin(app, "Checking lessons against Wikipedia…");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        let db = app.state::<Db>();
        for lesson_id in due {
            match fact_check(&db, lesson_id, false).await {
                Ok(check) if check.updated => {
                    let _ = app.emit("facts://updated", check);
                }
                Ok(_) => {}
                Err(error) => crate::diagnostics::note("background", format!("Checking lesson {lesson_id} against Wikipedia failed: {error}")),
            }
        }
    });
}

/// Compares a lesson's sources with Wikipedia now. Articles that changed are
/// fetched again; if any did (or `force`), an AI reads the lesson against
/// them and corrects what is out of date — nothing else — noting what
/// changed. Unchanged articles cost nothing.
pub async fn fact_check(db: &Db, lesson_id: LessonId, force: bool) -> Result<FactCheck> {
    let (title, sources): (String, Vec<(SourceId, String, String, i64)>) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let title = lessons::table.find(lesson_id).select(lessons::title).first(conn)?;
        let sources = wiki_sources::table
            .filter(wiki_sources::lesson_id.eq(lesson_id))
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
        let article = fetch_with_layout(lang, article_title).await?;
        let polish = match (&article.polish_title, lang.as_str()) {
            (Some(pl), l) if l != "pl" => fetch_with_layout("pl", pl).await.ok(),
            _ => None,
        };
        store(&mut db.lock(), lesson_id, &article, polish.as_ref(), None)?;
    }
    let mut check = FactCheck { lesson_id, title, updated: false, note: None };
    if changed || force {
        if let Some(note) = correct_lesson(db, lesson_id).await? {
            check.updated = true;
            check.note = Some(note);
        }
    }
    diesel::update(lessons::table.find(lesson_id))
        .set(lessons::fact_checked_at.eq(db::now_secs()))
        .execute(&mut *db.lock())?;
    Ok(check)
}

/// Asks the model whether the lesson is out of date against its sources and,
/// if so, stores the corrected lesson. Returns what changed.
async fn correct_lesson(db: &Db, lesson_id: LessonId) -> Result<Option<String>> {
    let (key, model, content, objectives, reading) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let (content, objectives): (String, String) =
            lessons::table.find(lesson_id).select((lessons::content, lessons::objectives)).first(conn)?;
        (
            crate::ai::api_key(conn)?,
            crate::ai::require_model(conn)?,
            content,
            objectives,
            reading_for(conn, lesson_id, READ_LIMIT)?,
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
            "The lesson:\n{content}\n\nWhat the learner should be able to do: {objectives}\n\n\
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
    let verdict = parse_verdict(&reply.text).inspect_err(|_| {
        crate::diagnostics::unreadable_reply("A Wikipedia fact check", &reply.text);
    })?;
    let Some((changes, corrected)) = verdict else { return Ok(None) };
    let today = crate::commands::civil_date(db::today(conn));
    let note = format!("{}-{:02}-{:02}: {changes}", today.0, today.1, today.2);
    diesel::update(lessons::table.find(lesson_id))
        .set((lessons::content.eq(corrected.trim()), lessons::fact_note.eq(&note)))
        .execute(conn)?;
    Ok(Some(note))
}

/// The model's verdict: None when the lesson stands, else what changed and
/// the corrected lesson.
fn parse_verdict(text: &str) -> Result<Option<(String, String)>> {
    let start = text.find('{').ok_or_else(|| Error::msg("The fact check gave no answer."))?;
    let end = text.rfind('}').ok_or_else(|| Error::msg("The fact check gave no answer."))?;
    #[derive(Deserialize)]
    struct Verdict {
        #[serde(default, deserialize_with = "crate::wire::lenient")]
        outdated: Option<bool>,
        #[serde(default, deserialize_with = "crate::wire::lenient")]
        changes: Option<String>,
        #[serde(default, deserialize_with = "crate::wire::lenient")]
        lesson: Option<String>,
    }
    let verdict: Verdict = serde_json::from_str(&text[start..=end])
        .map_err(|_| Error::msg("The fact check's answer was unreadable."))?;
    if verdict.outdated != Some(true) {
        return Ok(None);
    }
    let changes = verdict.changes.as_deref().unwrap_or("Corrected against Wikipedia.").trim().to_string();
    match verdict.lesson.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        Some(corrected) => Ok(Some((changes, corrected.to_string()))),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn lesson_sources(db: State<'_, Db>, lesson_id: LessonId) -> Result<Vec<Source>> {
    sources_of(&mut db.lock(), lesson_id)
}

/// One source's article for the reader, in its own language or in Polish.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleView {
    pub lang: String,
    pub title: String,
    pub url: String,
    /// The plain text, as markdown: shown when there is no layout.
    pub content: String,
    /// The article as Wikipedia lays it out (HTML), when it is known.
    pub html: Option<String>,
}

#[tauri::command]
pub async fn source_article(db: State<'_, Db>, source_id: SourceId, polish: bool) -> Result<ArticleView> {
    type Row = (String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);
    let row: Row = wiki_sources::table
        .find(source_id)
        .select((
            wiki_sources::lang,
            wiki_sources::title,
            wiki_sources::url,
            wiki_sources::content,
            wiki_sources::html,
            wiki_sources::pl_title,
            wiki_sources::pl_url,
            wiki_sources::pl_content,
            wiki_sources::pl_html,
        ))
        .first(&mut *db.lock())
        .optional()?
        .ok_or_else(|| Error::msg("That source no longer exists."))?;
    let (lang, title, url, content, html, pl_title, pl_url, pl_content, pl_html) = row;
    let (mut view, in_polish) = match (polish, pl_title, pl_url, pl_content) {
        (true, Some(title), Some(url), Some(content)) => {
            (ArticleView { lang: "pl".into(), title, url, content, html: pl_html }, true)
        }
        _ => (ArticleView { lang, title, url, content, html }, false),
    };
    // Sources kept before the reader showed layouts get theirs the first
    // time they are read online; offline, the plain text does.
    if view.html.is_none() {
        if let Ok(html) = fetch_html(&view.lang, &view.title).await {
            let row = diesel::update(wiki_sources::table.find(source_id));
            let _ = if in_polish {
                row.set(wiki_sources::pl_html.eq(&html)).execute(&mut *db.lock())
            } else {
                row.set(wiki_sources::html.eq(&html)).execute(&mut *db.lock())
            };
            view.html = Some(html);
        }
    }
    Ok(view)
}

/// An episode's sources, for its page.
#[tauri::command]
pub async fn song_sources(db: State<'_, Db>, song_id: SongId) -> Result<Vec<Source>> {
    sources_of_song(&mut db.lock(), song_id)
}

/// One of an episode's articles, kept now if it is not yet (the episode is
/// still being made, or the article could not be fetched then), so it opens
/// in the app's reader rather than outside it.
#[tauri::command]
pub async fn song_source_open(db: State<'_, Db>, song_id: SongId, lang: String, title: String) -> Result<Source> {
    let kept = sources_of_song(&mut db.lock(), song_id)?;
    if let Some(source) = kept.into_iter().find(|s| s.lang == lang && s.title == title) {
        return Ok(source);
    }
    attach_to(&db, Owner::Song(song_id), &lang, &title).await
}

/// Finds English Wikipedia articles, for choosing what a podcast is written from.
#[tauri::command]
pub async fn wiki_find(query: String) -> Result<Vec<SearchHit>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    search("en", query.trim()).await
}

/// Checks one lesson against its sources now, whatever the date.
#[tauri::command]
pub async fn fact_check_lesson(db: State<'_, Db>, lesson_id: LessonId) -> Result<FactCheck> {
    fact_check(&db, lesson_id, true).await
}

#[tauri::command]
pub async fn source_remove(db: State<'_, Db>, source_id: SourceId) -> Result<()> {
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
        let (changes, corrected) = parse_verdict(
            "Here: {\"outdated\": true, \"changes\": \"The population is now 804,000.\", \"lesson\": \"Kraków has 804,000 people.\"}",
        )
        .unwrap()
        .unwrap();
        assert!(changes.contains("804,000") && corrected.starts_with("Kraków"));
        assert!(parse_verdict("no json here").is_err());
        // A verdict without a corrected lesson changes nothing.
        assert_eq!(parse_verdict("{\"outdated\": true, \"changes\": \"x\"}").unwrap(), None);
    }

    #[test]
    fn search_snippets_lose_their_markup() {
        assert_eq!(strip_tags("the <span class=\"searchmatch\">Kraków</span> &quot;old&quot; town"), "the Kraków \"old\" town");
    }
}
