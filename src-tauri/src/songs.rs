//! Learning songs: an AI songwriter drafts bilingual lyrics, Suno (through
//! kie.ai) sings them, and the finished tracks are downloaded so they can be
//! replayed forever — kie.ai deletes generated files after 14 days.
//!
//! The song shape is the one that works for learning: verses pair a short
//! English line with the same line in Polish, sung right after it, and the
//! chorus is a single Polish sentence repeated until it sticks.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::ai;
use crate::db::{self, Db};
use crate::error::{Error, Result};

const KIE_BASE_URL: &str = "https://api.kie.ai/api/v1";
/// kie.ai's Suno generation endpoint, as named in its unified task API.
const KIE_MUSIC_MODEL: &str = "ai-music-api/generate";

/// Suno versions kie.ai currently accepts for new songs.
pub const SUNO_MODELS: &[&str] = &["V6", "V6_MINI", "V6_WILD"];
pub const DEFAULT_SUNO_MODEL: &str = "V6";

/// Suno usually finishes in two to four minutes.
const POLL_INTERVAL: Duration = Duration::from_secs(8);
const POLL_LIMIT: Duration = Duration::from_secs(15 * 60);

const TITLE_LIMIT: usize = 80;
const STYLE_LIMIT: usize = 1000;
const LYRICS_LIMIT: usize = 5000;

/// Steers Suno away from delivery that is hard to understand.
const NEGATIVE_TAGS: &str = "rap, spoken word, screaming, mumbled vocals, heavy distortion";


fn unreachable(error: reqwest::Error) -> Error {
    Error::msg(format!("Could not reach kie.ai: {error}"))
}

// ---------------------------------------------------------------------------
// Types shared with the frontend
// ---------------------------------------------------------------------------

/// One phrase of a song: the English line and the Polish line sung after it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SongLine {
    pub en: String,
    pub pl: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SongSection {
    /// verse | chorus | bridge | intro | outro
    pub kind: String,
    pub lines: Vec<SongLine>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SongDraft {
    pub title: String,
    /// The hook: the one Polish sentence the song is built to plant.
    pub chorus_pl: String,
    pub chorus_en: String,
    /// Suno style prompt: genre, tempo, voice.
    pub style: String,
    /// "f", "m" or "" for no preference.
    pub vocal_gender: String,
    pub sections: Vec<SongSection>,
    /// The course whose words it uses: its listening block is filed there.
    pub topic_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongTrack {
    pub id: i64,
    pub song_id: i64,
    pub ord: i64,
    /// Absolute path on this device; the webview plays it via the asset protocol.
    pub audio_path: String,
    pub image_path: Option<String>,
    pub duration: f64,
    pub play_count: i64,
    /// Lyrics timed to this take, when Suno's word timings are available.
    pub lines: Option<Vec<TimedLine>>,
}

/// One sung line with when it is sung, for lyrics that follow the music.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimedLine {
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// "pl" or "en" when the line matches the song's lyrics, else "".
    pub lang: String,
    /// For a Polish line, its English meaning.
    pub gloss: Option<String>,
    /// Set on the first line of a section: "Verse 1", "Chorus".
    pub section: Option<String>,
}

/// A sung word and its timing, as Suno aligns it. Stored compactly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimedWord {
    #[serde(rename = "w")]
    pub text: String,
    #[serde(rename = "s")]
    pub start: f64,
    #[serde(rename = "e")]
    pub end: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: i64,
    pub title: String,
    pub chorus_pl: String,
    pub chorus_en: String,
    pub style: String,
    pub vocal_gender: String,
    pub sections: Vec<SongSection>,
    /// generating | ready | failed
    pub status: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub tracks: Vec<SongTrack>,
}

/// What the songwriter is asked for.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DraftRequest {
    /// Free text: what the song should be about.
    pub brief: String,
    /// Draw vocabulary from this conversation topic.
    pub topic_id: Option<i64>,
    /// A fixed chorus sentence; empty lets the songwriter choose.
    pub chorus_pl: String,
    pub style: String,
    pub vocal_gender: String,
    /// "short" | "medium" | "long"; empty uses the setting.
    pub length: String,
    /// When refining: the current draft and what to change about it.
    pub previous: Option<SongDraft>,
    pub feedback: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongsChanged {
    pub song_id: i64,
}

// ---------------------------------------------------------------------------
// Lyrics
// ---------------------------------------------------------------------------

fn section_label(kind: &str) -> &'static str {
    match kind {
        "chorus" => "Chorus",
        "bridge" => "Bridge",
        "intro" => "Intro",
        "outro" => "Outro",
        _ => "Verse",
    }
}

/// Lyrics in the form Suno sings. Verses give each English line and then its
/// Polish twin; the chorus is sung in Polish only, so the hook is pure Polish.
pub fn render_lyrics(draft: &SongDraft) -> String {
    let mut out = String::new();
    let mut verse = 0;
    for section in &draft.sections {
        let kind = section.kind.as_str();
        let label = section_label(kind);
        let header = if label == "Verse" {
            verse += 1;
            format!("[Verse {verse}]")
        } else {
            format!("[{label}]")
        };
        let mut body = Vec::new();
        for line in &section.lines {
            let en = line.en.trim();
            let pl = line.pl.trim();
            if kind == "chorus" {
                if !pl.is_empty() {
                    body.push(pl.to_string());
                }
            } else {
                if !en.is_empty() {
                    body.push(en.to_string());
                }
                if !pl.is_empty() {
                    body.push(pl.to_string());
                }
            }
        }
        if body.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&header);
        out.push('\n');
        for line in body {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

fn truncate(text: &str, limit: usize) -> String {
    text.trim().chars().take(limit).collect()
}

/// Cleans a draft from the model or the editor so Suno never rejects it.
pub fn normalize_draft(mut draft: SongDraft) -> Result<SongDraft> {
    draft.title = truncate(&draft.title, TITLE_LIMIT);
    draft.chorus_pl = draft.chorus_pl.trim().to_string();
    draft.chorus_en = draft.chorus_en.trim().to_string();
    draft.style = truncate(&draft.style, STYLE_LIMIT);
    draft.vocal_gender = match draft.vocal_gender.trim() {
        "f" | "female" => "f".into(),
        "m" | "male" => "m".into(),
        _ => String::new(),
    };
    for section in &mut draft.sections {
        section.kind = match section.kind.trim().to_lowercase().as_str() {
            k if k.starts_with("chorus") || k == "hook" || k == "refrain" => "chorus",
            k if k.starts_with("bridge") => "bridge",
            k if k.starts_with("intro") => "intro",
            k if k.starts_with("outro") => "outro",
            _ => "verse",
        }
        .to_string();
        for line in &mut section.lines {
            line.en = line.en.trim().to_string();
            line.pl = line.pl.trim().to_string();
        }
        section.lines.retain(|l| !l.en.is_empty() || !l.pl.is_empty());
    }
    draft.sections.retain(|s| !s.lines.is_empty());

    if draft.chorus_pl.is_empty() {
        // Fall back to the first Polish line of the first chorus.
        draft.chorus_pl = draft
            .sections
            .iter()
            .find(|s| s.kind == "chorus")
            .and_then(|s| s.lines.iter().find(|l| !l.pl.is_empty()))
            .map(|l| l.pl.clone())
            .unwrap_or_default();
    }
    if draft.title.is_empty() {
        draft.title = truncate(&draft.chorus_pl, TITLE_LIMIT);
    }
    if draft.sections.is_empty() {
        return Err(Error::msg("The song has no lyrics yet."));
    }
    if draft.title.is_empty() {
        return Err(Error::msg("Give the song a title."));
    }
    if draft.style.is_empty() {
        draft.style = "catchy acoustic pop, mid tempo, clear vocals, clear diction".into();
    }
    let lyrics = render_lyrics(&draft);
    if lyrics.chars().count() > LYRICS_LIMIT {
        return Err(Error::msg(format!(
            "The lyrics are {} characters; Suno accepts at most {LYRICS_LIMIT}.",
            lyrics.chars().count()
        )));
    }
    Ok(draft)
}

/// Pulls the JSON object out of a model reply that may wrap it in prose or
/// a code fence.
pub fn parse_draft(reply: &str) -> Result<SongDraft> {
    let start = reply.find('{');
    let end = reply.rfind('}');
    let (Some(start), Some(end)) = (start, end) else {
        return Err(Error::msg("The songwriter did not return any lyrics. Try again."));
    };
    if end <= start {
        return Err(Error::msg("The songwriter did not return any lyrics. Try again."));
    }
    let draft: SongDraft = serde_json::from_str(&reply[start..=end]).map_err(|e| {
        Error::msg(format!("The songwriter's answer was not readable ({e}). Try again."))
    })?;
    normalize_draft(draft)
}

// ---------------------------------------------------------------------------
// Songwriter prompt
// ---------------------------------------------------------------------------

const SONGWRITER_RULES: &str = r#"You write songs that teach Polish to an adult beginner who speaks English. The learner will listen to each song many times, so it must be catchy, simple and correct.

How the songs work:
- Verses are made of pairs: a short English line, then the same meaning in Polish. Both are sung, English first. 2–7 words per line.
- The chorus is ONE Polish sentence (the hook) that the learner should get stuck in their head. It is sung only in Polish, usually 2–4 times. Give its English meaning.
- Follow the structure and length given in the brief exactly; Suno sings as long as the lyrics are.
- Polish must be natural, grammatical and use correct diacritics (ą ć ę ł ń ó ś ź ż). Prefer the present tense and everyday words.
- Reuse the target words; repeating a word across verses helps it stick.
- style is a Suno style prompt under 200 characters: genre, tempo, mood, voice. Always include "clear vocals" and "clear diction". Never name real artists.

Reply with ONLY a JSON object, no prose, in exactly this shape:
{
  "title": "short title",
  "chorusPl": "the Polish hook sentence",
  "chorusEn": "its English meaning",
  "style": "suno style prompt",
  "vocalGender": "f" | "m" | "",
  "sections": [
    {"kind": "verse", "lines": [{"en": "I would like a coffee", "pl": "Poproszę kawę"}]},
    {"kind": "chorus", "lines": [{"en": "A coffee with milk, please", "pl": "Poproszę kawę z mlekiem"}]}
  ]
}"#;

struct SongContext {
    topic: Option<String>,
    words: Vec<String>,
    sentences: Vec<String>,
    weak_words: Vec<String>,
    existing_choruses: Vec<String>,
}

fn gather_context(conn: &Connection, topic_id: Option<i64>) -> Result<SongContext> {
    let strings = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> Result<Vec<String>> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map(params, |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    };
    let topic = match topic_id {
        Some(id) => conn
            .query_row("SELECT title_en FROM topics WHERE id = ?1", [id], |r| r.get(0))
            .optional()?,
        None => None,
    };
    let (words, sentences) = match topic_id {
        Some(id) => (
            strings(
                "SELECT pl || ' = ' || en FROM course_words WHERE topic_id = ?1 ORDER BY ord LIMIT 40",
                &[&id],
            )?,
            strings(
                "SELECT pl || ' = ' || en FROM course_sentences WHERE topic_id = ?1 ORDER BY ord LIMIT 20",
                &[&id],
            )?,
        ),
        // Without a topic, sing what the learner has actually met.
        None => (
            strings(
                "SELECT w.pl || ' = ' || w.en FROM reviews r JOIN words w ON w.id = r.item_id
                 WHERE r.kind = 'word' ORDER BY r.last_day DESC LIMIT 40",
                &[],
            )?,
            strings(
                "SELECT s.pl || ' = ' || s.en FROM reviews r JOIN sentences s ON s.id = r.item_id
                 WHERE r.kind = 'sentence' ORDER BY r.last_day DESC LIMIT 20",
                &[],
            )?,
        ),
    };
    Ok(SongContext {
        topic,
        words,
        sentences,
        weak_words: strings(
            "SELECT w.pl FROM reviews r JOIN words w ON w.id = r.item_id
             WHERE r.kind = 'word' ORDER BY r.lapses DESC, r.ease ASC LIMIT 10",
            &[],
        )?,
        existing_choruses: strings("SELECT chorus_pl FROM songs ORDER BY id DESC LIMIT 30", &[])?,
    })
}

fn songwriter_messages(request: &DraftRequest, context: &SongContext, length: &str) -> Vec<ai::Wire> {
    let mut brief = String::new();
    if let Some(previous) = &request.previous {
        brief.push_str("Here is the current draft:\n");
        brief.push_str(&serde_json::to_string_pretty(previous).unwrap_or_default());
        brief.push_str("\n\nRevise it as follows, keeping everything else:\n");
        brief.push_str(if request.feedback.trim().is_empty() {
            "Make it catchier."
        } else {
            request.feedback.trim()
        });
        brief.push_str("\n\nReturn the whole revised song as JSON.");
        return vec![ai::Wire::system(SONGWRITER_RULES), ai::Wire::user(brief)];
    }

    if !request.brief.trim().is_empty() {
        brief.push_str(&format!("The song is about: {}\n", request.brief.trim()));
    }
    if let Some(topic) = &context.topic {
        brief.push_str(&format!("Theme: {topic}\n"));
    }
    brief.push_str(structure_for(length));
    brief.push('\n');
    if request.chorus_pl.trim().is_empty() {
        brief.push_str(
            "Choose the chorus sentence yourself — ideally one of the sentences below.\n",
        );
    } else {
        brief.push_str(&format!(
            "The chorus sentence MUST be exactly: {}\n",
            request.chorus_pl.trim()
        ));
    }
    if !request.style.trim().is_empty() {
        brief.push_str(&format!("Musical style: {}\n", request.style.trim()));
    }
    match request.vocal_gender.as_str() {
        "f" => brief.push_str("Sung by a female voice.\n"),
        "m" => brief.push_str("Sung by a male voice.\n"),
        _ => {}
    }
    if !context.words.is_empty() {
        brief.push_str(&format!("\nTarget words:\n{}\n", context.words.join("\n")));
    }
    if !context.sentences.is_empty() {
        brief.push_str(&format!("\nUseful sentences:\n{}\n", context.sentences.join("\n")));
    }
    if !context.weak_words.is_empty() {
        brief.push_str(&format!(
            "\nThe learner keeps forgetting these, so work some in: {}\n",
            context.weak_words.join(", ")
        ));
    }
    if !context.existing_choruses.is_empty() {
        brief.push_str(&format!(
            "\nThese choruses already exist — use a different one: {}\n",
            context.existing_choruses.join(" | ")
        ));
    }
    if brief.trim().is_empty() {
        brief.push_str("Write a song about everyday life for a complete beginner.\n");
    }
    vec![ai::Wire::system(SONGWRITER_RULES), ai::Wire::user(brief)]
}

/// The model that writes lyrics: its own setting, else the tutor's model.
pub fn songwriter_model(conn: &Connection) -> Result<String> {
    let model = ai::stored_model(conn, "songwriter_model");
    if !model.is_empty() {
        return Ok(model);
    }
    ai::require_model(conn)
}

/// How long a song should be. Suno has no duration setting: a song lasts as
/// long as its lyrics, so each length is a structure for the songwriter.
pub const SONG_LENGTHS: &[&str] = &["short", "medium", "long"];
pub const DEFAULT_SONG_LENGTH: &str = "long";

pub fn song_length(conn: &Connection) -> String {
    let chosen = ai::stored_model(conn, "song_length");
    if SONG_LENGTHS.contains(&chosen.as_str()) {
        chosen
    } else {
        DEFAULT_SONG_LENGTH.to_string()
    }
}

/// The song's shape at each length, as the songwriter is told it.
pub fn structure_for(length: &str) -> &'static str {
    match length {
        "short" => {
            "Length: short, about 2 minutes. Structure: verse, chorus, verse, chorus. \
             4 pairs per verse; the chorus sung 2 times each time."
        }
        "medium" => {
            "Length: about 3 minutes. Structure: verse, chorus, verse, chorus, bridge, chorus. \
             4 pairs per verse; the chorus sung 3 times each time."
        }
        _ => {
            "Length: a full song of 4–5 minutes, so write plenty of lyrics. Structure: intro, \
             verse, chorus, verse, chorus, bridge, verse, chorus, outro. 6 pairs per verse, a \
             4-pair bridge that uses new words, the chorus sung 4 times each time, a short \
             intro and an outro that repeats the chorus sentence."
        }
    }
}

/// Added to the style of long songs, so Suno arranges at full length.
const LONG_STYLE: &str = "full-length song, instrumental break, long outro";

pub fn suno_model(conn: &Connection) -> String {
    let chosen = ai::stored_model(conn, "suno_model");
    if SUNO_MODELS.contains(&chosen.as_str()) {
        chosen
    } else {
        DEFAULT_SUNO_MODEL.to_string()
    }
}

pub fn kie_key(conn: &Connection) -> Result<String> {
    db::get_setting(conn, "kie_api_key")?
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| Error::msg("No kie.ai API key yet. Add one in Settings to make songs."))
}

// ---------------------------------------------------------------------------
// kie.ai
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct KieEnvelope {
    code: i64,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: serde_json::Value,
}

/// kie.ai answers HTTP 200 even for failures; the real status is in `code`.
async fn kie_json(request: reqwest::RequestBuilder) -> Result<serde_json::Value> {
    let response = request.send().await.map_err(unreachable)?;
    let status = response.status();
    let body = response.text().await.map_err(unreachable)?;
    let envelope: KieEnvelope = serde_json::from_str(&body).map_err(|_| {
        Error::msg(format!(
            "kie.ai returned an unexpected response ({status}): {}",
            body.chars().take(300).collect::<String>()
        ))
    })?;
    if envelope.code != 200 {
        let hint = match envelope.code {
            401 => " Check the kie.ai key in Settings.",
            402 => " Top up your kie.ai credits.",
            429 => " Wait a moment and try again.",
            _ => "",
        };
        return Err(Error::msg(format!("kie.ai ({}): {}{hint}", envelope.code, envelope.msg)));
    }
    Ok(envelope.data)
}

pub async fn credits(key: &str) -> Result<f64> {
    let data = kie_json(crate::http::downloads().get(format!("{KIE_BASE_URL}/chat/credit")).bearer_auth(key)).await?;
    data.as_f64()
        .ok_or_else(|| Error::msg("kie.ai did not report a credit balance."))
}

async fn create_task(key: &str, model: &str, draft: &SongDraft) -> Result<String> {
    let mut input = serde_json::json!({
        "custom_mode": true,
        "instrumental": false,
        "model": model,
        "title": draft.title,
        "lyrics": render_lyrics(draft),
        "style": draft.style,
        "negative_tags": NEGATIVE_TAGS,
    });
    if !draft.vocal_gender.is_empty() {
        input["vocal_gender"] = draft.vocal_gender.clone().into();
    }
    let data = kie_json(
        crate::http::downloads()
            .post(format!("{KIE_BASE_URL}/jobs/createTask"))
            .bearer_auth(key)
            .json(&serde_json::json!({ "model": KIE_MUSIC_MODEL, "input": input })),
    )
    .await?;
    data.get("taskId")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| Error::msg("kie.ai accepted the song but returned no task id."))
}

/// A finished take, as kie.ai reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteTrack {
    pub id: String,
    pub audio_url: String,
    pub image_url: Option<String>,
    pub duration: f64,
}

#[derive(Debug, PartialEq)]
pub enum TaskState {
    Pending,
    Done(Vec<RemoteTrack>),
    Failed(String),
}

fn tracks_from(value: &serde_json::Value) -> Vec<RemoteTrack> {
    let list = value
        .get("data")
        .or_else(|| value.get("sunoData"))
        .and_then(|v| v.as_array())
        .or_else(|| value.as_array());
    let Some(list) = list else { return Vec::new() };
    list.iter()
        .filter_map(|item| {
            let audio_url = item
                .get("audio_url")
                .or_else(|| item.get("audioUrl"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())?
                .to_string();
            Some(RemoteTrack {
                id: item
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| audio_url.clone()),
                audio_url,
                image_url: item
                    .get("image_url")
                    .or_else(|| item.get("imageUrl"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                duration: item.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0),
            })
        })
        .collect()
}

/// Reads a `recordInfo` payload. The tracks normally sit in `response.data`;
/// `resultJson` (a JSON string) is the fallback.
pub fn parse_task(data: &serde_json::Value) -> TaskState {
    let state = data.get("state").and_then(|v| v.as_str()).unwrap_or("");
    match state {
        "success" => {
            let mut tracks = data.get("response").map(tracks_from).unwrap_or_default();
            if tracks.is_empty() {
                if let Some(parsed) = data
                    .get("resultJson")
                    .and_then(|v| v.as_str())
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                {
                    tracks = tracks_from(&parsed);
                }
            }
            if tracks.is_empty() {
                TaskState::Failed("Suno finished but returned no audio.".into())
            } else {
                TaskState::Done(tracks)
            }
        }
        "fail" => TaskState::Failed(
            data.get("failMsg")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("Suno could not make this song.")
                .to_string(),
        ),
        _ => TaskState::Pending,
    }
}

/// The kie.ai credits a finished task used, when the answer says.
pub fn credits_consumed(data: &serde_json::Value) -> Option<f64> {
    let value = data.get("creditsConsumed")?;
    value.as_f64().or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
}

async fn task_state(key: &str, task_id: &str) -> Result<(TaskState, Option<f64>)> {
    let data = kie_json(
        crate::http::downloads()
            .get(format!("{KIE_BASE_URL}/jobs/recordInfo"))
            .query(&[("taskId", task_id)])
            .bearer_auth(key),
    )
    .await?;
    Ok((parse_task(&data), credits_consumed(&data)))
}

/// Downloads a file to `path` and returns its bytes, which are also kept in
/// the database.
async fn download(url: &str, path: &Path) -> Result<Vec<u8>> {
    let response = crate::http::downloads().get(url).send().await.map_err(unreachable)?;
    if !response.status().is_success() {
        return Err(Error::msg(format!("Could not download the song ({}).", response.status())));
    }
    let bytes = response.bytes().await.map_err(unreachable)?;
    if bytes.is_empty() {
        return Err(Error::msg("The downloaded song was empty."));
    }
    // Write then rename, so a crash never leaves a half file that looks done.
    let partial = path.with_extension("part");
    std::fs::write(&partial, &bytes)?;
    std::fs::rename(&partial, path)?;
    Ok(bytes.to_vec())
}

fn extension(url: &str, fallback: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    path.rsplit_once('.')
        .map(|(_, ext)| ext.to_lowercase())
        .filter(|ext| ext.len() <= 4 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or_else(|| fallback.to_string())
}

// ---------------------------------------------------------------------------
// Timed lyrics
// ---------------------------------------------------------------------------

const TIMESTAMP_MODEL: &str = "ai-music-api/timeStamped-lyrics";

/// Reads Suno's aligned words, in either the camelCase (direct answer) or
/// snake_case (callback) spelling.
pub fn parse_aligned_words(data: &serde_json::Value) -> Option<Vec<TimedWord>> {
    let list = data
        .pointer("/data/alignedWords")
        .or_else(|| data.pointer("/data/aligned_words"))
        .or_else(|| data.get("alignedWords"))
        .or_else(|| data.get("aligned_words"))?
        .as_array()?;
    let number = |item: &serde_json::Value, a: &str, b: &str| {
        item.get(a).or_else(|| item.get(b)).and_then(|v| v.as_f64()).unwrap_or(-1.0)
    };
    Some(
        list.iter()
            .filter_map(|item| {
                Some(TimedWord {
                    text: item.get("word")?.as_str()?.to_string(),
                    start: number(item, "startS", "start_s"),
                    end: number(item, "endS", "end_s"),
                })
            })
            .collect(),
    )
}

fn normalized(text: &str) -> String {
    text.chars()
        .filter_map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next()
            } else if c.is_whitespace() {
                Some(' ')
            } else {
                None
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Groups timed words into the lines they were sung as, and labels each with
/// its language and meaning from the song's lyrics.
pub fn timed_lines(words: &[TimedWord], sections: &[SongSection]) -> Vec<TimedLine> {
    let mut polish: std::collections::HashMap<String, String> = Default::default();
    let mut english: std::collections::HashSet<String> = Default::default();
    for section in sections {
        for line in &section.lines {
            if !line.pl.is_empty() {
                polish.insert(normalized(&line.pl), line.en.clone());
            }
            if !line.en.is_empty() {
                english.insert(normalized(&line.en));
            }
        }
    }

    let mut lines: Vec<TimedLine> = Vec::new();
    let mut text = String::new();
    let mut start = -1.0_f64;
    let mut end = -1.0_f64;
    let mut section: Option<String> = None;
    let mut pending_section: Option<String> = None;

    let finish = |text: &mut String,
                      start: &mut f64,
                      end: &mut f64,
                      section: &mut Option<String>,
                      lines: &mut Vec<TimedLine>| {
        let line = text.trim().to_string();
        if !line.is_empty() && *start >= 0.0 {
            let key = normalized(&line);
            let (lang, gloss) = match polish.get(&key) {
                Some(meaning) => ("pl", Some(meaning.clone()).filter(|m| !m.is_empty())),
                None if english.contains(&key) => ("en", None),
                None => ("", None),
            };
            lines.push(TimedLine {
                start: *start,
                end: end.max(*start),
                text: line,
                lang: lang.to_string(),
                gloss,
                section: section.take(),
            });
        }
        text.clear();
        *start = -1.0;
        *end = -1.0;
        *section = None;
    };

    for word in words {
        for (i, segment) in word.text.split('\n').enumerate() {
            if i > 0 {
                finish(&mut text, &mut start, &mut end, &mut section, &mut lines);
            }
            let piece = segment.trim();
            if piece.starts_with('[') && piece.ends_with(']') {
                pending_section = Some(piece.trim_matches(['[', ']']).trim().to_string());
                continue;
            }
            if piece.is_empty() {
                continue;
            }
            if text.is_empty() {
                section = pending_section.take();
            } else {
                text.push(' ');
            }
            text.push_str(piece);
            if word.start >= 0.0 && start < 0.0 {
                start = word.start;
            }
            if word.end >= 0.0 {
                end = word.end;
            }
        }
    }
    finish(&mut text, &mut start, &mut end, &mut section, &mut lines);
    lines
}

async fn fetch_timestamps(key: &str, task_id: &str, audio_id: &str) -> Result<Vec<TimedWord>> {
    let data = kie_json(
        crate::http::downloads()
            .post(format!("{KIE_BASE_URL}/jobs/createTask"))
            .bearer_auth(key)
            .json(&serde_json::json!({
                "model": TIMESTAMP_MODEL,
                "input": { "task_id": task_id, "audio_id": audio_id },
            })),
    )
    .await?;
    // The answer nests a second status inside `data`.
    if let Some(code) = data.get("code").and_then(|c| c.as_i64()) {
        if code != 200 {
            let msg = data.get("msg").and_then(|m| m.as_str()).unwrap_or("no lyrics timing");
            return Err(Error::msg(format!("kie.ai ({code}): {msg}")));
        }
    }
    parse_aligned_words(&data).ok_or_else(|| Error::msg("kie.ai returned no lyrics timing."))
}

/// Fetches and stores one take's word timings.
async fn sync_track_lyrics(app: &AppHandle, track_id: i64) -> Result<()> {
    let (key, task_id, audio_id) = {
        let db = app.state::<Db>();
        let conn = db.lock();
        let row: Option<(Option<String>, String)> = conn
            .query_row(
                "SELECT s.task_id, t.remote_id FROM song_tracks t JOIN songs s ON s.id = t.song_id
                 WHERE t.id = ?1",
                [track_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            Some((Some(task_id), audio_id)) => (kie_key(&conn)?, task_id, audio_id),
            _ => return Err(Error::msg("This song has no Suno task to ask for timings.")),
        }
    };
    let words = fetch_timestamps(&key, &task_id, &audio_id).await?;
    let db = app.state::<Db>();
    db.lock().execute(
        "UPDATE song_tracks SET aligned_words = ?2 WHERE id = ?1",
        rusqlite::params![track_id, serde_json::to_string(&words)?],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Keeps the database and the song files on disk in step, in the background:
/// a take downloaded before songs were stored in the database has its bytes
/// copied in, and a file that has gone missing (an app-data clean-up, or a
/// database restored from an export) is written back out from its bytes.
pub fn sync_media(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(error) = sync_media_now(&app) {
            eprintln!("song media sync failed: {error}");
        }
    });
}

fn sync_media_now(app: &AppHandle) -> Result<()> {
    let dir = songs_dir(app)?;
    let db = app.state::<Db>();
    let rows: Vec<(i64, String, Option<String>, bool, bool)> = {
        let conn = db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, audio_path, image_path, audio_bytes IS NOT NULL, image_bytes IS NOT NULL
             FROM song_tracks",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };

    for (id, audio_path, image_path, has_audio, has_image) in rows {
        for (column, path, stored) in [
            ("audio", Some(audio_path), has_audio),
            ("image", image_path, has_image),
        ] {
            let Some(path) = path else { continue };
            let mut on_disk = PathBuf::from(&path);
            if stored && !on_disk.exists() {
                // The data folder may have moved (a restore on a new phone),
                // so the file goes back under the current one.
                if let Some(name) = on_disk.file_name() {
                    on_disk = dir.join(name);
                }
                let bytes: Vec<u8> = db.lock().query_row(
                    &format!("SELECT {column}_bytes FROM song_tracks WHERE id = ?1"),
                    [id],
                    |r| r.get(0),
                )?;
                std::fs::write(&on_disk, bytes)?;
                db.lock().execute(
                    &format!("UPDATE song_tracks SET {column}_path = ?2 WHERE id = ?1"),
                    rusqlite::params![id, on_disk.to_string_lossy()],
                )?;
            } else if !stored && on_disk.exists() {
                let bytes = std::fs::read(&on_disk)?;
                db.lock().execute(
                    &format!("UPDATE song_tracks SET {column}_bytes = ?2 WHERE id = ?1"),
                    rusqlite::params![id, bytes],
                )?;
            }
        }
    }
    Ok(())
}

pub fn songs_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| Error::msg(format!("no app data directory: {e}")))?
        .join("songs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn load_tracks(conn: &Connection, song_id: i64, sections: &[SongSection]) -> Result<Vec<SongTrack>> {
    let mut stmt = conn.prepare(
        "SELECT id, song_id, ord, audio_path, image_path, duration, play_count, aligned_words
         FROM song_tracks WHERE song_id = ?1 ORDER BY ord",
    )?;
    let tracks = stmt
        .query_map([song_id], |r| {
            Ok(SongTrack {
                id: r.get(0)?,
                song_id: r.get(1)?,
                ord: r.get(2)?,
                audio_path: r.get(3)?,
                image_path: r.get(4)?,
                duration: r.get(5)?,
                play_count: r.get(6)?,
                lines: r
                    .get::<_, Option<String>>(7)?
                    .and_then(|json| serde_json::from_str::<Vec<TimedWord>>(&json).ok())
                    .filter(|words| !words.is_empty())
                    .map(|words| timed_lines(&words, sections)),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

fn row_to_song(r: &rusqlite::Row) -> rusqlite::Result<Song> {
    let sections: String = r.get(6)?;
    Ok(Song {
        id: r.get(0)?,
        title: r.get(1)?,
        chorus_pl: r.get(2)?,
        chorus_en: r.get(3)?,
        style: r.get(4)?,
        vocal_gender: r.get(5)?,
        sections: serde_json::from_str(&sections).unwrap_or_default(),
        status: r.get(7)?,
        error: r.get(8)?,
        created_at: r.get(9)?,
        tracks: Vec::new(),
    })
}

const SONG_COLUMNS: &str =
    "id, title, chorus_pl, chorus_en, style, vocal_gender, sections, status, error, created_at";

pub fn load_song(conn: &Connection, id: i64) -> Result<Song> {
    let mut song = conn
        .query_row(
            &format!("SELECT {SONG_COLUMNS} FROM songs WHERE id = ?1"),
            [id],
            row_to_song,
        )
        .optional()?
        .ok_or_else(|| Error::msg("That song no longer exists."))?;
    song.tracks = load_tracks(conn, id, &song.sections)?;
    Ok(song)
}

pub fn list_songs(conn: &Connection) -> Result<Vec<Song>> {
    let mut stmt = conn.prepare(&format!("SELECT {SONG_COLUMNS} FROM songs ORDER BY id DESC"))?;
    let mut songs = stmt
        .query_map([], row_to_song)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for song in &mut songs {
        song.tracks = load_tracks(conn, song.id, &song.sections)?;
    }
    Ok(songs)
}

pub fn insert_song(conn: &Connection, draft: &SongDraft, suno_model: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO songs (title, chorus_pl, chorus_en, style, vocal_gender, sections, lyrics,
                            suno_model, status, created_at, topic_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'generating', ?9,
                 (SELECT id FROM topics WHERE id = ?10))",
        rusqlite::params![
            draft.title,
            draft.chorus_pl,
            draft.chorus_en,
            draft.style,
            draft.vocal_gender,
            serde_json::to_string(&draft.sections)?,
            render_lyrics(draft),
            suno_model,
            db::now_secs(),
            draft.topic_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn set_status(conn: &Connection, id: i64, status: &str, error: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE songs SET status = ?2, error = ?3 WHERE id = ?1",
        rusqlite::params![id, status, error],
    )?;
    Ok(())
}

fn draft_of(song: &Song) -> SongDraft {
    SongDraft {
        title: song.title.clone(),
        chorus_pl: song.chorus_pl.clone(),
        chorus_en: song.chorus_en.clone(),
        style: song.style.clone(),
        vocal_gender: song.vocal_gender.clone(),
        sections: song.sections.clone(),
        // Only for generation, which does not file anything.
        topic_id: None,
    }
}

fn notify(app: &AppHandle, song_id: i64) {
    let _ = app.emit("songs://changed", SongsChanged { song_id });
}

// ---------------------------------------------------------------------------
// Generation
// ---------------------------------------------------------------------------

/// Waits for Suno, then downloads every take. Runs detached from the command
/// that started it, so leaving the screen (or the app) does not lose the song;
/// unfinished songs are picked up again at the next launch.
pub fn spawn_generation(app: AppHandle, song_id: i64) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = generate(&app, song_id).await {
            let db = app.state::<Db>();
            let conn = db.lock();
            let _ = set_status(&conn, song_id, "failed", Some(&error.to_string()));
            drop(conn);
            notify(&app, song_id);
        }
    });
}

async fn generate(app: &AppHandle, song_id: i64) -> Result<()> {
    let (key, task_id, model, draft) = {
        let db = app.state::<Db>();
        let conn = db.lock();
        let song = load_song(&conn, song_id)?;
        let task_id: Option<String> =
            conn.query_row("SELECT task_id FROM songs WHERE id = ?1", [song_id], |r| r.get(0))?;
        let model: String =
            conn.query_row("SELECT suno_model FROM songs WHERE id = ?1", [song_id], |r| r.get(0))?;
        (kie_key(&conn)?, task_id, model, draft_of(&song))
    };

    let task_id = match task_id {
        Some(id) if !id.is_empty() => id,
        _ => {
            let id = create_task(&key, &model, &draft).await?;
            let db = app.state::<Db>();
            db.lock().execute(
                "UPDATE songs SET task_id = ?2 WHERE id = ?1",
                rusqlite::params![song_id, id],
            )?;
            id
        }
    };

    let started = std::time::Instant::now();
    let tracks = loop {
        match task_state(&key, &task_id).await {
            Ok((TaskState::Done(tracks), credits)) => {
                // Logged once per task, however often a retry polls it again.
                let db = app.state::<Db>();
                let _ = crate::costs::record_once(
                    &db.lock(),
                    crate::costs::SONGS,
                    "kie.ai",
                    &model,
                    credits.map(|c| c * crate::costs::KIE_USD_PER_CREDIT),
                    &credits.map(|c| format!("{c} credits")).unwrap_or_default(),
                    Some(&format!("kie:{task_id}")),
                );
                break tracks;
            }
            Ok((TaskState::Failed(reason), _)) => {
                // The task is spent; a retry must start a new one.
                let db = app.state::<Db>();
                db.lock().execute("UPDATE songs SET task_id = NULL WHERE id = ?1", [song_id])?;
                return Err(Error::msg(reason));
            }
            Ok((TaskState::Pending, _)) => {}
            // A dropped connection mid-wait is not worth abandoning the song.
            Err(error) if started.elapsed() > POLL_LIMIT => return Err(error),
            Err(_) => {}
        }
        if started.elapsed() > POLL_LIMIT {
            return Err(Error::msg(
                "Suno is taking unusually long. Tap retry to check on it again.",
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    };

    let dir = songs_dir(app)?;
    for (ord, track) in tracks.iter().enumerate() {
        let audio = dir.join(format!(
            "song-{song_id}-{ord}.{}",
            extension(&track.audio_url, "mp3")
        ));
        let audio_bytes = download(&track.audio_url, &audio).await?;
        let (image, image_bytes) = match &track.image_url {
            Some(url) => {
                let path = dir.join(format!("song-{song_id}-{ord}.{}", extension(url, "jpg")));
                // Cover art is a nicety; a failed download must not lose the song.
                match download(url, &path).await {
                    Ok(bytes) => (Some(path), Some(bytes)),
                    Err(_) => (None, None),
                }
            }
            None => (None, None),
        };
        let db = app.state::<Db>();
        db.lock().execute(
            "INSERT INTO song_tracks
                 (song_id, ord, remote_id, audio_path, image_path, duration, audio_bytes, image_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(song_id, remote_id) DO UPDATE SET
                 audio_path = excluded.audio_path, image_path = excluded.image_path,
                 duration = excluded.duration, audio_bytes = excluded.audio_bytes,
                 image_bytes = excluded.image_bytes",
            rusqlite::params![
                song_id,
                ord as i64,
                track.id,
                audio.to_string_lossy(),
                image.map(|p| p.to_string_lossy().to_string()),
                track.duration,
                audio_bytes,
                image_bytes,
            ],
        )?;
    }

    let db = app.state::<Db>();
    set_status(&db.lock(), song_id, "ready", None)?;
    notify(app, song_id);

    // Word timings, so the lyrics can follow the music. A song without them
    // still plays, and playing it later tries again.
    let track_ids: Vec<i64> = {
        let db = app.state::<Db>();
        let conn = db.lock();
        let mut stmt = conn.prepare("SELECT id FROM song_tracks WHERE song_id = ?1")?;
        let ids = stmt.query_map([song_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids
    };
    let mut synced = false;
    for id in track_ids {
        synced |= sync_track_lyrics(app, id).await.is_ok();
    }
    if synced {
        notify(app, song_id);
    }
    Ok(())
}

/// Resumes songs that were still being made when the app last closed.
pub fn resume_pending(app: &AppHandle) {
    let pending: Vec<i64> = {
        let db = app.state::<Db>();
        let conn = db.lock();
        conn.prepare("SELECT id FROM songs WHERE status = 'generating'")
            .and_then(|mut stmt| {
                stmt.query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<Vec<i64>>>()
            })
            .unwrap_or_default()
    };
    for id in pending {
        spawn_generation(app.clone(), id);
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn songs_list(db: State<'_, Db>) -> Result<Vec<Song>> {
    list_songs(&db.lock())
}

/// Asks the songwriter for a draft, or a revision of one.
#[tauri::command]
pub async fn song_draft(db: State<'_, Db>, request: DraftRequest) -> Result<SongDraft> {
    write_draft(&db, request).await
}

/// The songwriter at work: used by the composer screen and by the tutor.
pub async fn write_draft(db: &Db, request: DraftRequest) -> Result<SongDraft> {
    let (key, model, messages) = {
        let conn = db.lock();
        let context = gather_context(&conn, request.topic_id)?;
        let length = if SONG_LENGTHS.contains(&request.length.as_str()) {
            request.length.clone()
        } else {
            song_length(&conn)
        };
        (
            ai::api_key(&conn)?,
            songwriter_model(&conn)?,
            (songwriter_messages(&request, &context, &length), length),
        )
    };
    let (messages, length) = messages;
    // A long song needs room for its lyrics in the answer.
    let reply = ai::complete_with(&key, &model, &messages, 0.8, 5000).await?;
    let _ = crate::costs::record(
        &db.lock(),
        crate::costs::SONGWRITER,
        "openrouter",
        &model,
        reply.cost,
        "",
    );
    let mut draft = parse_draft(&reply.text)?;
    if length == "long" && request.previous.is_none() && !draft.style.contains(LONG_STYLE) {
        draft.style = format!("{}, {LONG_STYLE}", draft.style.trim_end_matches([',', ' ']));
    }
    draft.topic_id = request.topic_id.or(request.previous.as_ref().and_then(|p| p.topic_id));
    // The learner's explicit choices beat the model's.
    if request.previous.is_none() {
        if !request.chorus_pl.trim().is_empty() {
            draft.chorus_pl = request.chorus_pl.trim().to_string();
        }
        if !request.vocal_gender.is_empty() {
            draft.vocal_gender = request.vocal_gender.clone();
        }
    }
    Ok(draft)
}

/// Saves the song and starts Suno on it. Returns at once; progress arrives as
/// `songs://changed` events.
#[tauri::command]
pub async fn song_create(app: AppHandle, db: State<'_, Db>, draft: SongDraft) -> Result<Song> {
    let id = start_song(&app, &db, draft)?;
    load_song(&db.lock(), id)
}

/// Saves a finished draft and hands it to Suno in the background.
pub fn start_song(app: &AppHandle, db: &Db, draft: SongDraft) -> Result<i64> {
    let draft = normalize_draft(draft)?;
    let id = {
        let conn = db.lock();
        kie_key(&conn)?;
        let model = suno_model(&conn);
        insert_song(&conn, &draft, &model)?
    };
    spawn_generation(app.clone(), id);
    notify(app, id);
    Ok(id)
}

#[tauri::command]
pub async fn song_retry(app: AppHandle, db: State<'_, Db>, song_id: i64) -> Result<Song> {
    {
        let conn = db.lock();
        kie_key(&conn)?;
        set_status(&conn, song_id, "generating", None)?;
    }
    spawn_generation(app, song_id);
    load_song(&db.lock(), song_id)
}

#[tauri::command]
pub async fn song_delete(db: State<'_, Db>, song_id: i64) -> Result<()> {
    delete_song(&db.lock(), song_id)
}

/// Deletes a song, its takes and their files. Its takes leave every playlist.
pub fn delete_song(conn: &Connection, song_id: i64) -> Result<()> {
    for track in load_tracks(&conn, song_id, &[])? {
        let _ = std::fs::remove_file(&track.audio_path);
        if let Some(image) = track.image_path {
            let _ = std::fs::remove_file(image);
        }
    }
    conn.execute("DELETE FROM song_tracks WHERE song_id = ?1", [song_id])?;
    conn.execute("DELETE FROM songs WHERE id = ?1", [song_id])?;
    Ok(())
}

/// Removes one take, keeping the song and its other takes.
#[tauri::command]
pub async fn song_track_delete(db: State<'_, Db>, track_id: i64) -> Result<()> {
    let conn = db.lock();
    let paths: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT audio_path, image_path FROM song_tracks WHERE id = ?1",
            [track_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((audio, image)) = paths {
        let _ = std::fs::remove_file(audio);
        if let Some(image) = image {
            let _ = std::fs::remove_file(image);
        }
    }
    conn.execute("DELETE FROM song_tracks WHERE id = ?1", [track_id])?;
    Ok(())
}

#[tauri::command]
pub async fn song_track_played(db: State<'_, Db>, track_id: i64) -> Result<()> {
    db.lock().execute(
        "UPDATE song_tracks SET play_count = play_count + 1, last_played_at = ?2 WHERE id = ?1",
        rusqlite::params![track_id, db::now_secs()],
    )?;
    Ok(())
}

/// Fetches a take's word timings if it has none yet, and returns its timed
/// lines. kie.ai only keeps songs for 14 days, so older ones may have none.
#[tauri::command]
pub async fn song_track_lyrics(
    app: AppHandle,
    db: State<'_, Db>,
    track_id: i64,
) -> Result<Option<Vec<TimedLine>>> {
    let (song_id, has_words) = {
        let conn = db.lock();
        conn.query_row(
            "SELECT song_id, aligned_words IS NOT NULL FROM song_tracks WHERE id = ?1",
            [track_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?)),
        )
        .optional()?
        .ok_or_else(|| Error::msg("That song no longer exists."))?
    };
    if !has_words {
        sync_track_lyrics(&app, track_id).await?;
    }
    let song = load_song(&db.lock(), song_id)?;
    Ok(song.tracks.into_iter().find(|t| t.id == track_id).and_then(|t| t.lines))
}

#[tauri::command]
pub async fn kie_credits(db: State<'_, Db>) -> Result<f64> {
    let key = kie_key(&db.lock())?;
    credits(&key).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> SongDraft {
        SongDraft {
            title: "Kawa".into(),
            chorus_pl: "Poproszę kawę z mlekiem".into(),
            chorus_en: "A coffee with milk, please".into(),
            style: "acoustic pop".into(),
            vocal_gender: "".into(),
            topic_id: None,
            sections: vec![
                SongSection {
                    kind: "verse".into(),
                    lines: vec![
                        SongLine { en: "Good morning".into(), pl: "Dzień dobry".into() },
                        SongLine { en: "I am hungry".into(), pl: "Jestem głodny".into() },
                    ],
                },
                SongSection {
                    kind: "chorus".into(),
                    lines: vec![SongLine {
                        en: "A coffee with milk, please".into(),
                        pl: "Poproszę kawę z mlekiem".into(),
                    }],
                },
                SongSection {
                    kind: "verse".into(),
                    lines: vec![SongLine { en: "Thank you".into(), pl: "Dziękuję".into() }],
                },
            ],
        }
    }

    #[test]
    fn every_length_has_a_structure() {
        for length in SONG_LENGTHS {
            assert!(structure_for(length).contains("Structure"), "{length}");
        }
        assert!(structure_for("long").contains("4–5 minutes"));
        assert!(structure_for("nonsense").contains("4–5 minutes"), "unknown means the default");
    }

    #[test]
    fn verses_sing_english_then_polish_and_the_chorus_only_polish() {
        assert_eq!(
            render_lyrics(&draft()),
            "[Verse 1]\nGood morning\nDzień dobry\nI am hungry\nJestem głodny\n\n\
             [Chorus]\nPoproszę kawę z mlekiem\n\n\
             [Verse 2]\nThank you\nDziękuję"
        );
    }

    #[test]
    fn drafts_are_read_out_of_chatty_replies() {
        let reply = format!(
            "Here you go!\n```json\n{}\n```\nEnjoy.",
            serde_json::to_string(&draft()).unwrap()
        );
        assert_eq!(parse_draft(&reply).unwrap(), draft());
        assert!(parse_draft("no json here").is_err());
    }

    #[test]
    fn normalizing_fixes_what_a_model_gets_loosely_right() {
        let mut loose = draft();
        loose.sections[1].kind = "Chorus 1".into();
        loose.sections[0].kind = "VERSE".into();
        loose.vocal_gender = "female".into();
        loose.chorus_pl = String::new();
        loose.title = "x".repeat(200);
        loose.sections.push(SongSection { kind: "bridge".into(), lines: vec![SongLine::default()] });
        let clean = normalize_draft(loose).unwrap();
        assert_eq!(clean.sections[1].kind, "chorus");
        assert_eq!(clean.sections[0].kind, "verse");
        assert_eq!(clean.vocal_gender, "f");
        assert_eq!(clean.chorus_pl, "Poproszę kawę z mlekiem", "taken from the chorus");
        assert_eq!(clean.title.chars().count(), TITLE_LIMIT);
        assert_eq!(clean.sections.len(), 3, "empty sections are dropped");
    }

    #[test]
    fn finished_tasks_yield_their_tracks() {
        let data = serde_json::json!({
            "state": "success",
            "response": { "data": [
                { "id": "a", "audio_url": "https://x/a.mp3", "image_url": "https://x/a.jpeg", "duration": 182.5 },
                { "id": "b", "audio_url": "https://x/b.mp3", "duration": 175.0 }
            ]}
        });
        let TaskState::Done(tracks) = parse_task(&data) else { panic!("not done") };
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].image_url.as_deref(), Some("https://x/a.jpeg"));
        assert_eq!(tracks[1].image_url, None);

        // Older payloads only carry the result as a JSON string.
        let data = serde_json::json!({
            "state": "success",
            "resultJson": "{\"data\":[{\"id\":\"c\",\"audio_url\":\"https://x/c.mp3\"}]}"
        });
        assert!(matches!(parse_task(&data), TaskState::Done(t) if t[0].id == "c"));

        assert_eq!(parse_task(&serde_json::json!({ "state": "generating" })), TaskState::Pending);
        assert_eq!(credits_consumed(&serde_json::json!({ "creditsConsumed": 12 })), Some(12.0));
        assert_eq!(credits_consumed(&serde_json::json!({ "creditsConsumed": "6.5" })), Some(6.5));
        assert_eq!(credits_consumed(&serde_json::json!({})), None);
        assert_eq!(
            parse_task(&serde_json::json!({ "state": "fail", "failMsg": "lyrics rejected" })),
            TaskState::Failed("lyrics rejected".into())
        );
    }

    #[test]
    fn suno_word_timings_are_read_in_both_spellings() {
        // As documented: the answer nests a second status inside `data`.
        let direct = serde_json::json!({
            "code": 200, "msg": "success",
            "data": { "alignedWords": [
                { "word": "[Verse]\nWaggin'", "success": true, "startS": 1.36, "endS": 1.79, "palign": 0 }
            ], "waveformData": [0, 1], "hootCer": 0.38, "isStreamed": false }
        });
        assert_eq!(
            parse_aligned_words(&direct),
            Some(vec![TimedWord { text: "[Verse]\nWaggin'".into(), start: 1.36, end: 1.79 }])
        );
        let callback = serde_json::json!({ "aligned_words": [{ "word": "Hej", "start_s": 2.0, "end_s": 2.5 }] });
        assert_eq!(parse_aligned_words(&callback).unwrap()[0].start, 2.0);
        assert_eq!(parse_aligned_words(&serde_json::json!({ "data": {} })), None);
    }

    fn word(text: &str, start: f64, end: f64) -> TimedWord {
        TimedWord { text: text.into(), start, end }
    }

    #[test]
    fn timed_words_become_lines_with_meanings() {
        let words = vec![
            word("[Verse 1]\nGood ", 1.0, 1.4),
            word("morning\n", 1.4, 2.0),
            word("Dzień ", 2.2, 2.6),
            word("dobry!\n\n", 2.6, 3.1),
            word("[Chorus]\nPoproszę ", 10.0, 10.5),
            word("kawę ", 10.5, 11.0),
            word("z ", 11.0, 11.1),
            word("mlekiem\n", 11.1, 12.0),
            word("Hmm", 13.0, 13.5),
        ];
        let lines = timed_lines(&words, &draft().sections);
        let shape: Vec<(&str, &str, Option<&str>, Option<&str>, f64, f64)> = lines
            .iter()
            .map(|l| (l.text.as_str(), l.lang.as_str(), l.gloss.as_deref(), l.section.as_deref(), l.start, l.end))
            .collect();
        assert_eq!(
            shape,
            vec![
                ("Good morning", "en", None, Some("Verse 1"), 1.0, 2.0),
                ("Dzień dobry!", "pl", Some("Good morning"), None, 2.2, 3.1),
                ("Poproszę kawę z mlekiem", "pl", Some("A coffee with milk, please"), Some("Chorus"), 10.0, 12.0),
                // Ad-libs Suno adds are kept, just without a translation.
                ("Hmm", "", None, None, 13.0, 13.5),
            ]
        );
    }

    #[test]
    fn file_extensions_come_from_the_url() {
        assert_eq!(extension("https://cdn/x/abc.mp3?sig=1", "bin"), "mp3");
        assert_eq!(extension("https://cdn/x/abc", "mp3"), "mp3");
        assert_eq!(extension("https://cdn/x/cover.JPEG", "jpg"), "jpeg");
    }
}
