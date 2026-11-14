//! Learning songs: an AI songwriter drafts bilingual lyrics, Suno (through
//! kie.ai) sings them, and the finished tracks are downloaded so they can be
//! replayed forever — kie.ai deletes generated files after 14 days.
//!
//! The song shape is the one that works for learning: verses pair a short
//! English line with the same line in Polish, sung right after it, and the
//! chorus is a single Polish sentence repeated until it sticks.


use std::time::Duration;

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::ai;
use crate::files;
use crate::db::{self, Conn, Db};
use crate::schema::{course_sentences, course_words, reviews, sentences, song_tracks, songs, topics, words};
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
    /// How the song came about: what was asked and with what. Saved with the
    /// song, so it can be made again or varied later.
    pub origin: SongOrigin,
}

/// Everything that went into making a song, besides the lyrics and style it
/// ended up with (which the song itself holds).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SongOrigin {
    /// What the song was asked to be about.
    pub brief: String,
    /// The style, singer and chorus asked for (empty: left to the songwriter).
    pub requested_style: String,
    pub requested_vocal_gender: String,
    pub requested_chorus: String,
    /// "short" | "medium" | "long", as written.
    pub length: String,
    /// The model that wrote the lyrics.
    pub songwriter_model: String,
    /// Changes asked for while refining the draft, in order.
    pub revisions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongTrack {
    pub id: i64,
    pub song_id: i64,
    pub ord: i64,
    /// The take's audio and cover art: file ids, which the webview loads
    /// through the media protocol.
    pub audio_file_id: i64,
    pub image_file_id: Option<i64>,
    /// The copies on disk, only for Android's native player.
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
    /// The course it was written from.
    pub topic_id: Option<i64>,
    /// The Suno model that sang it.
    pub suno_model: String,
    pub origin: SongOrigin,
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

pub(crate) struct SongContext {
    topic: Option<String>,
    words: Vec<String>,
    sentences: Vec<String>,
    weak_words: Vec<String>,
    existing_choruses: Vec<String>,
}

pub(crate) fn gather_context(conn: &mut Conn, topic_id: Option<i64>) -> Result<SongContext> {
    let pair = |(pl, en): (String, String)| format!("{pl} = {en}");
    let topic = match topic_id {
        Some(id) => topics::table.find(id).select(topics::title_en).first(conn).optional()?,
        None => None,
    };
    let met = |kind: &'static str| reviews::kind.eq(kind);
    let (words, sentences) = match topic_id {
        Some(id) => (
            course_words::table
                .filter(course_words::topic_id.eq(id))
                .order(course_words::ord)
                .limit(40)
                .select((course_words::pl, course_words::en))
                .load(conn)?,
            course_sentences::table
                .filter(course_sentences::topic_id.eq(id))
                .order(course_sentences::ord)
                .limit(20)
                .select((course_sentences::pl, course_sentences::en))
                .load(conn)?,
        ),
        // Without a topic, sing what the learner has actually met.
        None => (
            words::table
                .inner_join(reviews::table.on(reviews::item_id.eq(words::id).and(met("word"))))
                .order(reviews::last_day.desc())
                .limit(40)
                .select((words::pl, words::en))
                .load(conn)?,
            sentences::table
                .inner_join(reviews::table.on(reviews::item_id.eq(sentences::id).and(met("sentence"))))
                .order(reviews::last_day.desc())
                .limit(20)
                .select((sentences::pl, sentences::en))
                .load(conn)?,
        ),
    };
    Ok(SongContext {
        topic,
        words: words.into_iter().map(pair).collect(),
        sentences: sentences.into_iter().map(pair).collect(),
        weak_words: words::table
            .inner_join(reviews::table.on(reviews::item_id.eq(words::id).and(met("word"))))
            .order((reviews::lapses.desc(), reviews::ease.asc()))
            .limit(10)
            .select(words::pl)
            .load(conn)?,
        existing_choruses: songs::table.order(songs::id.desc()).limit(30).select(songs::chorus_pl).load(conn)?,
    })
}

fn songwriter_messages(request: &DraftRequest, context: &SongContext, length: &str) -> Vec<ai::Wire> {
    let mut brief = String::new();
    if let Some(previous) = &request.previous {
        brief.push_str("Here is the current draft:\n");
        // How it came about is for the record, not for the songwriter.
        let previous = SongDraft { origin: SongOrigin::default(), ..previous.clone() };
        brief.push_str(&serde_json::to_string_pretty(&previous).unwrap_or_default());
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
pub fn songwriter_model(conn: &mut Conn) -> Result<String> {
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

pub fn song_length(conn: &mut Conn) -> String {
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

pub fn suno_model(conn: &mut Conn) -> String {
    let chosen = ai::stored_model(conn, "suno_model");
    if SUNO_MODELS.contains(&chosen.as_str()) {
        chosen
    } else {
        DEFAULT_SUNO_MODEL.to_string()
    }
}

pub fn kie_key(conn: &mut Conn) -> Result<String> {
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

/// Downloads a file and returns its bytes and type; the caller stores them in
/// the files table.
async fn download(url: &str) -> Result<(Option<String>, Vec<u8>)> {
    let response = crate::http::downloads().get(url).send().await.map_err(unreachable)?;
    if !response.status().is_success() {
        return Err(Error::msg(format!("Could not download the song ({}).", response.status())));
    }
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").trim().to_string())
        .filter(|v| v.starts_with("audio/") || v.starts_with("image/"));
    let bytes = response.bytes().await.map_err(unreachable)?;
    if bytes.is_empty() {
        return Err(Error::msg("The downloaded song was empty."));
    }
    Ok((mime, bytes.to_vec()))
}

/// A MIME type from a file extension, when the server did not say.
fn mime_for(ext: &str, fallback: &str) -> String {
    match ext {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => fallback,
    }
    .to_string()
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
        let mut guard = db.lock();
        let conn = &mut *guard;
        let row: Option<(Option<String>, String)> = song_tracks::table
            .inner_join(songs::table)
            .filter(song_tracks::id.eq(track_id))
            .select((songs::task_id, song_tracks::remote_id))
            .first(conn)
            .optional()?;
        match row {
            Some((Some(task_id), audio_id)) => (kie_key(conn)?, task_id, audio_id),
            _ => return Err(Error::msg("This song has no Suno task to ask for timings.")),
        }
    };
    let words = fetch_timestamps(&key, &task_id, &audio_id).await?;
    let db = app.state::<Db>();
    diesel::update(song_tracks::table.find(track_id))
        .set(song_tracks::aligned_words.eq(serde_json::to_string(&words)?))
        .execute(&mut *db.lock())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// A take as stored, before its word timings are matched to the lyrics.
#[derive(Queryable, Selectable)]
#[diesel(table_name = song_tracks, check_for_backend(diesel::sqlite::Sqlite))]
struct TrackRow {
    id: i64,
    song_id: i64,
    ord: i64,
    audio_file_id: Option<i64>,
    image_file_id: Option<i64>,
    duration: f64,
    play_count: i64,
    aligned_words: Option<String>,
}

fn load_tracks(conn: &mut Conn, song_id: i64, sections: &[SongSection]) -> Result<Vec<SongTrack>> {
    let rows: Vec<TrackRow> = song_tracks::table
        .filter(song_tracks::song_id.eq(song_id))
        .filter(song_tracks::audio_file_id.is_not_null())
        .order(song_tracks::ord)
        .select(TrackRow::as_select())
        .load(conn)?;
    // The players stream from each file's copy on disk.
    let ids: Vec<i64> = rows.iter().flat_map(|r| [r.audio_file_id, r.image_file_id]).flatten().collect();
    let paths = files::paths(conn, &ids)?;
    let path = |id: Option<i64>| id.and_then(|id| paths.get(&id).cloned());
    Ok(rows
        .into_iter()
        .map(|row| SongTrack {
            id: row.id,
            song_id: row.song_id,
            ord: row.ord,
            audio_file_id: row.audio_file_id.unwrap_or_default(),
            image_file_id: row.image_file_id,
            audio_path: path(row.audio_file_id).unwrap_or_default(),
            image_path: path(row.image_file_id),
            duration: row.duration,
            play_count: row.play_count,
            lines: row
                .aligned_words
                .and_then(|json| serde_json::from_str::<Vec<TimedWord>>(&json).ok())
                .filter(|words| !words.is_empty())
                .map(|words| timed_lines(&words, sections)),
        })
        .collect())
}

/// A song as stored, before its lyrics are parsed and its takes loaded.
#[derive(Queryable, Selectable)]
#[diesel(table_name = songs, check_for_backend(diesel::sqlite::Sqlite))]
struct SongRow {
    id: i64,
    title: String,
    chorus_pl: String,
    chorus_en: String,
    style: String,
    vocal_gender: String,
    sections: String,
    status: String,
    error: Option<String>,
    created_at: i64,
    topic_id: Option<i64>,
    suno_model: String,
    origin: String,
}

fn with_tracks(conn: &mut Conn, row: SongRow) -> Result<Song> {
    let sections: Vec<SongSection> = serde_json::from_str(&row.sections).unwrap_or_default();
    Ok(Song {
        tracks: load_tracks(conn, row.id, &sections)?,
        id: row.id,
        title: row.title,
        chorus_pl: row.chorus_pl,
        chorus_en: row.chorus_en,
        style: row.style,
        vocal_gender: row.vocal_gender,
        sections,
        status: row.status,
        error: row.error,
        created_at: row.created_at,
        topic_id: row.topic_id,
        suno_model: row.suno_model,
        origin: serde_json::from_str(&row.origin).unwrap_or_default(),
    })
}

pub fn load_song(conn: &mut Conn, id: i64) -> Result<Song> {
    let row = songs::table
        .find(id)
        .select(SongRow::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg("That song no longer exists."))?;
    with_tracks(conn, row)
}

pub fn list_songs(conn: &mut Conn) -> Result<Vec<Song>> {
    let rows = songs::table.order(songs::id.desc()).select(SongRow::as_select()).load(conn)?;
    rows.into_iter().map(|row| with_tracks(conn, row)).collect()
}

pub fn insert_song(conn: &mut Conn, draft: &SongDraft, suno_model: &str) -> Result<i64> {
    // A course that no longer exists is simply not remembered.
    let topic_id: Option<i64> = match draft.topic_id {
        Some(id) => topics::table.find(id).select(topics::id).first(conn).optional()?,
        None => None,
    };
    Ok(diesel::insert_into(songs::table)
        .values((
            songs::title.eq(&draft.title),
            songs::chorus_pl.eq(&draft.chorus_pl),
            songs::chorus_en.eq(&draft.chorus_en),
            songs::style.eq(&draft.style),
            songs::vocal_gender.eq(&draft.vocal_gender),
            songs::sections.eq(serde_json::to_string(&draft.sections)?),
            songs::lyrics.eq(render_lyrics(draft)),
            songs::suno_model.eq(suno_model),
            songs::status.eq("generating"),
            songs::created_at.eq(db::now_secs()),
            songs::topic_id.eq(topic_id),
            songs::origin.eq(serde_json::to_string(&draft.origin)?),
        ))
        .returning(songs::id)
        .get_result(conn)?)
}

fn set_status(conn: &mut Conn, id: i64, status: &str, error: Option<&str>) -> Result<()> {
    diesel::update(songs::table.find(id)).set((songs::status.eq(status), songs::error.eq(error))).execute(conn)?;
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
        origin: song.origin.clone(),
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
            let mut guard = db.lock();
            let _ = set_status(&mut guard, song_id, "failed", Some(&error.to_string()));
            drop(guard);
            notify(&app, song_id);
        }
    });
}

async fn generate(app: &AppHandle, song_id: i64) -> Result<()> {
    let (key, task_id, model, draft) = {
        let db = app.state::<Db>();
        let mut guard = db.lock();
        let conn = &mut *guard;
        let song = load_song(conn, song_id)?;
        let (task_id, model): (Option<String>, String) =
            songs::table.find(song_id).select((songs::task_id, songs::suno_model)).first(conn)?;
        (kie_key(conn)?, task_id, model, draft_of(&song))
    };

    let task_id = match task_id {
        Some(id) if !id.is_empty() => id,
        _ => {
            let id = create_task(&key, &model, &draft).await?;
            let db = app.state::<Db>();
            diesel::update(songs::table.find(song_id)).set(songs::task_id.eq(&id)).execute(&mut *db.lock())?;
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
                    &mut db.lock(),
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
                diesel::update(songs::table.find(song_id))
                    .set(songs::task_id.eq(None::<String>))
                    .execute(&mut *db.lock())?;
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

    let dir = files::media_dir(app)?;
    for (ord, track) in tracks.iter().enumerate() {
        let (audio_mime, audio) = download(&track.audio_url).await?;
        // Cover art is a nicety; a failed download must not lose the song.
        let image = match &track.image_url {
            Some(url) => download(url).await.ok().map(|(mime, bytes)| (url, mime, bytes)),
            None => None,
        };
        let db = app.state::<Db>();
        let mut guard = db.lock();
        let conn = &mut *guard;
        let audio_ext = extension(&track.audio_url, "mp3");
        let audio_mime = audio_mime.unwrap_or_else(|| mime_for(&audio_ext, "audio/mpeg"));
        let audio_id =
            files::store(conn, files::SONG_AUDIO, &format!("song-{song_id}-{ord}.{audio_ext}"), &audio_mime, &audio)?;
        files::materialize(conn, &dir, audio_id)?;
        let image_id = match image {
            Some((url, mime, bytes)) => {
                let ext = extension(url, "jpg");
                let mime = mime.unwrap_or_else(|| mime_for(&ext, "image/jpeg"));
                let id = files::store(conn, files::SONG_IMAGE, &format!("song-{song_id}-{ord}.{ext}"), &mime, &bytes)?;
                files::materialize(conn, &dir, id)?;
                Some(id)
            }
            None => None,
        };
        // Downloading a take again replaces its files.
        let old: Option<(Option<i64>, Option<i64>)> = song_tracks::table
            .filter(song_tracks::song_id.eq(song_id))
            .filter(song_tracks::remote_id.eq(&track.id))
            .select((song_tracks::audio_file_id, song_tracks::image_file_id))
            .first(conn)
            .optional()?;
        diesel::insert_into(song_tracks::table)
            .values((
                song_tracks::song_id.eq(song_id),
                song_tracks::ord.eq(ord as i64),
                song_tracks::remote_id.eq(&track.id),
                song_tracks::duration.eq(track.duration),
                song_tracks::audio_file_id.eq(audio_id),
                song_tracks::image_file_id.eq(image_id),
            ))
            .on_conflict((song_tracks::song_id, song_tracks::remote_id))
            .do_update()
            .set((
                song_tracks::duration.eq(track.duration),
                song_tracks::audio_file_id.eq(audio_id),
                song_tracks::image_file_id.eq(image_id),
            ))
            .execute(conn)?;
        if let Some((old_audio, old_image)) = old {
            files::delete(conn, &[old_audio, old_image].into_iter().flatten().collect::<Vec<_>>())?;
        }
    }

    let db = app.state::<Db>();
    set_status(&mut db.lock(), song_id, "ready", None)?;
    notify(app, song_id);

    // Word timings, so the lyrics can follow the music. A song without them
    // still plays, and playing it later tries again.
    let track_ids: Vec<i64> = {
        let db = app.state::<Db>();
        let mut guard = db.lock();
        let conn = &mut *guard;
        song_tracks::table.filter(song_tracks::song_id.eq(song_id)).select(song_tracks::id).load(conn)?
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
        let mut guard = db.lock();
        let conn = &mut *guard;
        songs::table.filter(songs::status.eq("generating")).select(songs::id).load(conn).unwrap_or_default()
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
    list_songs(&mut db.lock())
}

/// Asks the songwriter for a draft, or a revision of one.
#[tauri::command]
pub async fn song_draft(db: State<'_, Db>, request: DraftRequest) -> Result<SongDraft> {
    write_draft(&db, request).await
}

/// The songwriter at work: used by the composer screen and by the tutor.
pub async fn write_draft(db: &Db, request: DraftRequest) -> Result<SongDraft> {
    let (key, model, messages) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let context = gather_context(conn, request.topic_id)?;
        let length = if SONG_LENGTHS.contains(&request.length.as_str()) {
            request.length.clone()
        } else {
            song_length(conn)
        };
        (
            ai::api_key(conn)?,
            songwriter_model(conn)?,
            (songwriter_messages(&request, &context, &length), length),
        )
    };
    let (messages, length) = messages;
    // A long song needs room for its lyrics in the answer.
    let reply = ai::complete_with(&key, &model, &messages, 0.8, 5000).await?;
    let _ = crate::costs::record(
        &mut db.lock(),
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
    draft.origin = match &request.previous {
        // A revision keeps how the song began and adds what was changed.
        Some(previous) => {
            let mut origin = previous.origin.clone();
            origin.revisions.push(match request.feedback.trim() {
                "" => "Make it catchier.".to_string(),
                feedback => feedback.to_string(),
            });
            origin.songwriter_model = model.clone();
            origin
        }
        None => SongOrigin {
            brief: request.brief.trim().to_string(),
            requested_style: request.style.trim().to_string(),
            requested_vocal_gender: request.vocal_gender.trim().to_string(),
            requested_chorus: request.chorus_pl.trim().to_string(),
            length: length.clone(),
            songwriter_model: model.clone(),
            revisions: Vec::new(),
        },
    };
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
    load_song(&mut db.lock(), id)
}

/// Saves a finished draft and hands it to Suno in the background.
pub fn start_song(app: &AppHandle, db: &Db, draft: SongDraft) -> Result<i64> {
    let draft = normalize_draft(draft)?;
    let id = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        kie_key(conn)?;
        let model = suno_model(conn);
        insert_song(conn, &draft, &model)?
    };
    spawn_generation(app.clone(), id);
    notify(app, id);
    Ok(id)
}

#[tauri::command]
pub async fn song_retry(app: AppHandle, db: State<'_, Db>, song_id: i64) -> Result<Song> {
    {
        let mut guard = db.lock();
        let conn = &mut *guard;
        kie_key(conn)?;
        set_status(conn, song_id, "generating", None)?;
    }
    spawn_generation(app, song_id);
    load_song(&mut db.lock(), song_id)
}

#[tauri::command]
pub async fn song_delete(db: State<'_, Db>, song_id: i64) -> Result<()> {
    delete_song(&mut db.lock(), song_id)
}

/// Deletes a song, its takes and their files. Its takes leave every playlist.
pub fn delete_song(conn: &mut Conn, song_id: i64) -> Result<()> {
    let takes = song_tracks::table.filter(song_tracks::song_id.eq(song_id));
    let ids = take_files(takes.select((song_tracks::audio_file_id, song_tracks::image_file_id)).load(conn)?);
    diesel::delete(takes).execute(conn)?;
    files::delete(conn, &ids)?;
    diesel::delete(songs::table.find(song_id)).execute(conn)?;
    Ok(())
}

/// The file ids of takes' audio and cover art.
fn take_files(rows: Vec<(Option<i64>, Option<i64>)>) -> Vec<i64> {
    rows.into_iter().flat_map(|(audio, image)| [audio, image]).flatten().collect()
}

/// Removes one take, keeping the song and its other takes.
#[tauri::command]
pub async fn song_track_delete(db: State<'_, Db>, track_id: i64) -> Result<()> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    let take = song_tracks::table.find(track_id);
    let ids = take_files(take.select((song_tracks::audio_file_id, song_tracks::image_file_id)).load(conn)?);
    diesel::delete(take).execute(conn)?;
    files::delete(conn, &ids)?;
    Ok(())
}

#[tauri::command]
pub async fn song_track_played(db: State<'_, Db>, track_id: i64) -> Result<()> {
    diesel::update(song_tracks::table.find(track_id))
        .set((song_tracks::play_count.eq(song_tracks::play_count + 1), song_tracks::last_played_at.eq(db::now_secs())))
        .execute(&mut *db.lock())?;
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
        let mut guard = db.lock();
        let conn = &mut *guard;
        song_tracks::table
            .find(track_id)
            .select((song_tracks::song_id, song_tracks::aligned_words.is_not_null()))
            .first::<(i64, bool)>(conn)
            .optional()?
            .ok_or_else(|| Error::msg("That song no longer exists."))?
    };
    if !has_words {
        sync_track_lyrics(&app, track_id).await?;
    }
    let song = load_song(&mut db.lock(), song_id)?;
    Ok(song.tracks.into_iter().find(|t| t.id == track_id).and_then(|t| t.lines))
}

#[tauri::command]
pub async fn kie_credits(db: State<'_, Db>) -> Result<f64> {
    let key = kie_key(&mut db.lock())?;
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
            origin: SongOrigin::default(),
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
