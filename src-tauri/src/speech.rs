//! Speech: Polish read aloud by Gemini through OpenRouter, the voices to
//! choose from, and the audio cache that makes each line a one-time cost and
//! replays work offline.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::ai;
use crate::costs;
use crate::db::{self, Conn, Db};
use crate::files;
use crate::schema::{course_lines, course_sentences, course_words, files as files_table, sentences, speech_clips, topics};
use crate::error::{Error, Result};
use crate::models::*;

/// The voices the app can read Polish in. Shipped with the binary rather than
/// fetched, because the speech model is pinned.
#[tauri::command]
pub async fn list_voices() -> Result<Vec<Voice>> {
    Ok(ai::GEMINI_VOICES
        .iter()
        .map(|(name, gender, description)| Voice {
            name: (*name).to_string(),
            gender: (*gender).to_string(),
            description: (*description).to_string(),
        })
        .collect())
}

/// Plays a sample so a voice can be judged on Polish before it is chosen.
#[tauri::command]
pub async fn preview_voice(db: State<'_, Db>, voice: String, text: String) -> Result<AudioClip> {
    let (key, speed) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let speed = db::get_setting(conn, "tts_speed")?
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| (*value - 1.0).abs() > f32::EPSILON);
        (ai::api_key(conn)?, speed)
    };

    let cached = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        cached_clip(conn, &audio_key(ai::SPEECH_MODEL, &voice, &text))?
    };
    if let Some((mime, bytes)) = cached {
        return Ok(AudioClip { mime, base64: BASE64.encode(bytes), cached: true });
    }

    let (mime, bytes) =
        ai::synthesize(&key, ai::SPEECH_MODEL, &voice, &text, speed).await?;
    {
        let mut guard = db.lock();
        let conn = &mut *guard;
        record_speech(conn, ai::SPEECH_MODEL, &text);
        store_clip(
            conn,
            &audio_key(ai::SPEECH_MODEL, &voice, &text),
            ai::SPEECH_MODEL,
            &voice,
            &text,
            &mime,
            &bytes,
        )?;
    }
    Ok(AudioClip { mime, base64: BASE64.encode(bytes), cached: false })
}

// ---------------------------------------------------------------------------
// Speech
// ---------------------------------------------------------------------------

/// Cache identity: the same text spoken by a different model or voice is a
/// different clip. Unit separators keep the parts from running together.
pub(crate) fn audio_key(model: &str, voice: &str, text: &str) -> String {
    format!("{model}\u{1}{voice}\u{1}{text}")
}

fn cached_clip(conn: &mut Conn, key: &str) -> Result<Option<(String, Vec<u8>)>> {
    Ok(speech_clips::table
        .inner_join(files_table::table)
        .filter(speech_clips::cache_key.eq(key))
        .select((files_table::mime, files_table::bytes))
        .first(conn)
        .optional()?)
}

pub(crate) fn store_clip(
    conn: &mut Conn,
    key: &str,
    model: &str,
    voice: &str,
    text: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<()> {
    conn.transaction::<_, Error, _>(|tx| {
        // A clip made again replaces the old one, file and all.
        let old: Option<i64> =
            speech_clips::table.find(key).select(speech_clips::file_id).first(tx).optional()?;
        if let Some(old) = old {
            files::delete(tx, &[old])?;
        }
        let name = format!("speech-{}.{}", db::now_secs(), files::extension_for(mime));
        let file_id = files::store(tx, files::SPEECH, &name, mime, bytes)?;
        diesel::insert_into(speech_clips::table)
            .values((
                speech_clips::cache_key.eq(key),
                speech_clips::model.eq(model),
                speech_clips::voice.eq(voice),
                speech_clips::text.eq(text),
                speech_clips::file_id.eq(file_id),
                speech_clips::created_at.eq(db::now_secs()),
            ))
            .execute(tx)?;
        Ok(())
    })
}

/// Everything needed to synthesise, read in one lock so the await is clean.
struct SpeechConfig {
    key: String,
    model: String,
    voice: String,
    speed: Option<f32>,
}

/// `gender` is "f" or "m" for dialogue lines; everything else reads in the
/// female voice.
fn speech_config(conn: &mut Conn, gender: &str) -> Result<SpeechConfig> {
    let speed = db::get_setting(conn, "tts_speed")?
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| (*value - 1.0).abs() > f32::EPSILON);
    Ok(SpeechConfig {
        key: ai::api_key(conn)?,
        model: ai::SPEECH_MODEL.to_string(),
        voice: ai::voice_for(conn, gender),
        speed,
    })
}

/// One lock per clip being made, so two asks for the same line — a 🔊 tap
/// while the background download is on it — make it once, not twice.
fn clip_lock(key: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    static LOCKS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>> =
        std::sync::OnceLock::new();
    let mut locks = LOCKS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(key).and_then(|weak| weak.upgrade()) {
        return lock;
    }
    let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(key.to_string(), std::sync::Arc::downgrade(&lock));
    lock
}

/// A line's clip in the voice its gender gets: the stored one, or made now
/// and stored (`force` makes it again even when stored). Returns its type,
/// bytes, and whether it was already stored.
async fn clip(db: &Db, text: &str, gender: &str, force: bool) -> Result<(String, Vec<u8>, bool)> {
    let config = speech_config(&mut db.lock(), gender)?;
    let key = audio_key(&config.model, &config.voice, text);
    let lock = clip_lock(&key);
    let _making = lock.lock().await;
    if !force {
        if let Some((mime, bytes)) = cached_clip(&mut db.lock(), &key)? {
            return Ok((mime, bytes, true));
        }
    }
    let (mime, bytes) = ai::synthesize(&config.key, &config.model, &config.voice, text, config.speed).await?;
    let mut guard = db.lock();
    let conn = &mut *guard;
    record_speech(conn, &config.model, text);
    store_clip(conn, &key, &config.model, &config.voice, text, &mime, &bytes)?;
    Ok((mime, bytes, false))
}

/// Speaks a line of Polish, generating it only the first time.
#[tauri::command]
pub async fn speak(db: State<'_, Db>, text: String, gender: Option<String>) -> Result<AudioClip> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(Error::msg("Nothing to say."));
    }
    let (mime, bytes, cached) = clip(&db, &text, &gender.unwrap_or_default(), false).await?;
    Ok(AudioClip { mime, base64: BASE64.encode(bytes), cached })
}

/// Makes a line's audio again, replacing the stored clip — for one that came
/// out wrong (an English accent, the wrong words).
#[tauri::command]
pub async fn respeak(db: State<'_, Db>, text: String, gender: Option<String>) -> Result<AudioClip> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(Error::msg("Nothing to say."));
    }
    let (mime, bytes, _) = clip(&db, &text, &gender.unwrap_or_default(), true).await?;
    Ok(AudioClip { mime, base64: BASE64.encode(bytes), cached: false })
}

/// A line to synthesise, with the gender that decides which voice reads it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechLine {
    pub text: String,
    #[serde(default)]
    pub gender: String,
}

/// Generates and stores audio for a batch of lines, reporting progress as it
/// goes (`tts://progress`). Used to download a course's audio ahead of time.
#[tauri::command]
pub async fn cache_audio(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    lines: Vec<SpeechLine>,
    // Make them again even when stored (to replace clips that came out wrong).
    force: Option<bool>,
) -> Result<CacheProgress> {
    let _busy = crate::work::begin(&app, "Downloading audio…");
    Ok(fill(&app, &db, lines, force.unwrap_or(false)).await)
}

/// Downloads the audio a practice session is about to use, in the
/// background, and returns at once. Lines come in the order they will be
/// heard, so the first ones are ready first; it carries on with the screen
/// off.
#[tauri::command]
pub async fn prefetch_audio(app: tauri::AppHandle, lines: Vec<SpeechLine>) -> Result<()> {
    let busy = crate::work::begin(&app, "Downloading the lesson's audio…");
    let app_ref = app.clone();
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        use tauri::Manager;
        let db = app_ref.state::<Db>();
        fill(&app_ref, &db, lines, false).await;
    });
    Ok(())
}

/// Makes every missing clip among `lines`, a few at a time, in order.
async fn fill(app: &tauri::AppHandle, db: &Db, lines: Vec<SpeechLine>, force: bool) -> CacheProgress {
    use futures_util::StreamExt;
    // Drop duplicates and, unless forced, anything already stored before
    // spending a request. A line in two voices is two clips.
    let mut seen = std::collections::HashSet::new();
    let pending: Vec<SpeechLine> = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        lines
            .into_iter()
            .map(|line| SpeechLine { text: line.text.trim().to_string(), gender: line.gender })
            .filter(|line| !line.text.is_empty() && seen.insert((line.text.clone(), line.gender.clone())))
            .filter(|line| {
                force
                    || speech_config(conn, &line.gender)
                        .map(|c| cached_clip(conn, &audio_key(&c.model, &c.voice, &line.text)).ok().flatten().is_none())
                        .unwrap_or(false)
            })
            .collect()
    };
    let total = pending.len() as i64;
    let (mut done, mut failed, mut first_error) = (0, 0, None::<String>);
    // Three at a time: much faster than one by one, gentle on the API.
    let mut results = futures_util::stream::iter(pending)
        .map(|line| async move { clip(db, &line.text, &line.gender, force).await })
        .buffered(3);
    while let Some(result) = results.next().await {
        match result {
            Ok(_) => done += 1,
            Err(error) => {
                failed += 1;
                first_error.get_or_insert_with(|| error.to_string());
                // A bad key or model fails every line; stop rather than
                // hammering the API a hundred times.
                if failed >= 3 {
                    break;
                }
            }
        }
        let _ = app.emit("tts://progress", CacheProgress { done, total, failed, finished: false, error: None });
    }
    let progress = CacheProgress { done, total, failed, finished: true, error: first_error };
    let _ = app.emit("tts://progress", progress.clone());
    progress
}

/// Every Polish line of a topic: its words, its sentences, then its
/// conversation with the gender each line should be read in.
/// The query behind [`topic_audio_lines`], callable without Tauri (tests).
pub fn topic_audio_lines_in(conn: &mut Conn, topic_id: i64) -> Result<Vec<SpeechLine>> {
    Ok(course_audio_items(conn, topic_id)?
        .into_iter()
        .map(|(_, text, gender)| SpeechLine { text, gender })
        .collect())
}

/// A course's spoken items as (kind, text, gender), without repeats: words,
/// sentences, then dialogue lines.
fn course_audio_items(conn: &mut Conn, topic_id: i64) -> Result<Vec<(&'static str, String, String)>> {
    let words = course_words::table
        .filter(course_words::topic_id.eq(topic_id))
        .order((course_words::block_id, course_words::ord))
        .select(course_words::pl)
        .load::<String>(conn)?
        .into_iter()
        .map(|text| ("word", text, String::new()));
    let sentences = course_sentences::table
        .filter(course_sentences::topic_id.eq(topic_id))
        .order((course_sentences::block_id, course_sentences::ord))
        .select(course_sentences::pl)
        .load::<String>(conn)?
        .into_iter()
        .map(|text| ("sentence", text, String::new()));
    let lines = course_lines::table
        .filter(course_lines::topic_id.eq(topic_id))
        .order((course_lines::block_id, course_lines::ord))
        .select((course_lines::pl, course_lines::gender))
        .load::<(String, String)>(conn)?
        .into_iter()
        .map(|(text, gender)| ("line", text, gender));
    let mut seen = std::collections::HashSet::new();
    Ok(words
        .chain(sentences)
        .chain(lines)
        .map(|(kind, text, gender)| (kind, text.trim().to_string(), gender))
        .filter(|(_, text, gender)| !text.is_empty() && seen.insert((text.clone(), gender.clone())))
        .collect())
}

/// Every stored clip's key and size, without loading the audio itself.
fn stored_clips(conn: &mut Conn) -> Result<std::collections::HashMap<String, i64>> {
    Ok(speech_clips::table
        .inner_join(files_table::table)
        .select((speech_clips::cache_key, files_table::size))
        .load::<(String, i64)>(conn)?
        .into_iter()
        .collect())
}

/// The cache key an item is stored under with the voices chosen now.
struct Voices {
    female: String,
    male: String,
}

impl Voices {
    fn current(conn: &mut Conn) -> Self {
        Voices { female: ai::voice_for(conn, "f"), male: ai::voice_for(conn, "m") }
    }
    fn key(&self, text: &str, gender: &str) -> String {
        audio_key(ai::SPEECH_MODEL, if gender == "m" { &self.male } else { &self.female }, text)
    }
}

/// How much of one course can be played without the network.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseAudio {
    pub topic_id: i64,
    pub title: String,
    pub title_pl: String,
    pub total: i64,
    pub stored: i64,
    pub bytes: i64,
}

/// Downloaded audio at a glance: per course, and what no course uses.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioOverview {
    pub clips: i64,
    pub bytes: i64,
    pub courses: Vec<CourseAudio>,
    /// Clips no course item uses with the voices chosen now: other voices,
    /// deleted items, voice samples.
    pub unused_clips: i64,
    pub unused_bytes: i64,
}

/// One spoken item of a course and whether its audio is stored.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioItem {
    /// "word" | "sentence" | "line"
    pub kind: String,
    pub text: String,
    pub gender: String,
    pub stored: bool,
    pub bytes: i64,
}

pub fn audio_overview_in(conn: &mut Conn) -> Result<AudioOverview> {
    let stored = stored_clips(conn)?;
    let voices = Voices::current(conn);
    let mut used = std::collections::HashSet::new();
    let mut courses = Vec::new();
    let list: Vec<(i64, String, String)> = topics::table
        .order((topics::ord, topics::id))
        .select((topics::id, topics::title_en, topics::title_pl))
        .load(conn)?;
    for (topic_id, title, title_pl) in list {
        let items = course_audio_items(conn, topic_id)?;
        if items.is_empty() {
            continue;
        }
        let (mut count, mut bytes) = (0, 0);
        for (_, text, gender) in &items {
            let key = voices.key(text, gender);
            if let Some(size) = stored.get(&key) {
                count += 1;
                bytes += size;
            }
            used.insert(key);
        }
        courses.push(CourseAudio { topic_id, title, title_pl, total: items.len() as i64, stored: count, bytes });
    }
    let unused: Vec<i64> = stored.iter().filter(|(key, _)| !used.contains(*key)).map(|(_, size)| *size).collect();
    Ok(AudioOverview {
        clips: stored.len() as i64,
        bytes: stored.values().sum(),
        courses,
        unused_clips: unused.len() as i64,
        unused_bytes: unused.iter().sum(),
    })
}

pub fn course_audio_in(conn: &mut Conn, topic_id: i64) -> Result<Vec<AudioItem>> {
    let stored = stored_clips(conn)?;
    let voices = Voices::current(conn);
    Ok(course_audio_items(conn, topic_id)?
        .into_iter()
        .map(|(kind, text, gender)| {
            let size = stored.get(&voices.key(&text, &gender)).copied();
            AudioItem { kind: kind.into(), stored: size.is_some(), bytes: size.unwrap_or(0), text, gender }
        })
        .collect())
}

/// Deletes the clips no course uses with the current voices.
pub fn delete_unused_audio_in(conn: &mut Conn) -> Result<i64> {
    let voices = Voices::current(conn);
    let topic_ids: Vec<i64> = topics::table.select(topics::id).load(conn)?;
    let mut used = std::collections::HashSet::new();
    for topic_id in topic_ids {
        for (_, text, gender) in course_audio_items(conn, topic_id)? {
            used.insert(voices.key(&text, &gender));
        }
    }
    let unused: Vec<i64> = speech_clips::table
        .select((speech_clips::cache_key, speech_clips::file_id))
        .load::<(String, i64)>(conn)?
        .into_iter()
        .filter(|(key, _)| !used.contains(key))
        .map(|(_, file_id)| file_id)
        .collect();
    // Deleting the file deletes its clip row with it.
    let deleted = files::delete(conn, &unused)?;
    if deleted > 0 {
        conn.batch_execute("VACUUM")?;
    }
    Ok(deleted as i64)
}

#[tauri::command]
pub async fn audio_overview(db: State<'_, Db>) -> Result<AudioOverview> {
    audio_overview_in(&mut db.lock())
}

#[tauri::command]
pub async fn course_audio(db: State<'_, Db>, topic_id: i64) -> Result<Vec<AudioItem>> {
    course_audio_in(&mut db.lock(), topic_id)
}

#[tauri::command]
pub async fn delete_unused_audio(db: State<'_, Db>) -> Result<i64> {
    delete_unused_audio_in(&mut db.lock())
}

#[tauri::command]
pub async fn topic_audio_lines(db: State<'_, Db>, topic_id: i64) -> Result<Vec<SpeechLine>> {
    topic_audio_lines_in(&mut db.lock(), topic_id)
}

/// The query behind [`audio_cache_stats`], callable without Tauri (tests).
pub fn audio_cache_stats_in(conn: &mut Conn) -> Result<AudioCacheStats> {
    let clips: i64 = speech_clips::table.count().get_result(conn)?;
    let bytes: i64 = stored_clips(conn)?.values().sum();
    // Sentences are narrated by the female voice, so that is what counts here.
    let sentences_cached = match speech_config(conn, "f") {
        Ok(config) => sentences::table
            .filter(
                sentences::pl.eq_any(
                    speech_clips::table
                        .filter(speech_clips::model.eq(&config.model))
                        .filter(speech_clips::voice.eq(&config.voice))
                        .select(speech_clips::text),
                ),
            )
            .count()
            .get_result(conn)?,
        Err(_) => 0,
    };
    Ok(AudioCacheStats {
        clips,
        bytes,
        sentences_cached,
        sentences_total: sentences::table.count().get_result(conn)?,
    })
}

#[tauri::command]
pub async fn audio_cache_stats(db: State<'_, Db>) -> Result<AudioCacheStats> {
    audio_cache_stats_in(&mut db.lock())
}

#[tauri::command]
pub async fn clear_audio_cache(db: State<'_, Db>) -> Result<()> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    diesel::delete(files_table::table.filter(files_table::kind.eq(files::SPEECH))).execute(conn)?;
    // Reclaim the space; audio is by far the largest thing in the database.
    conn.batch_execute("VACUUM")?;
    Ok(())
}

/// OpenRouter's speech endpoint reports no price, so speech is logged as a
/// count with the characters spoken.
fn record_speech(conn: &mut Conn, model: &str, text: &str) {
    let detail = format!("{} characters", text.chars().count());
    let _ = costs::record(conn, costs::SPEECH, "openrouter", model, None, &detail);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_for_the_same_clip_share_one_lock() {
        let a = clip_lock("m\u{1}Kore\u{1}Cześć");
        let b = clip_lock("m\u{1}Kore\u{1}Cześć");
        let other = clip_lock("m\u{1}Puck\u{1}Cześć");
        assert!(std::sync::Arc::ptr_eq(&a, &b), "a 🔊 tap waits for the download already making it");
        assert!(!std::sync::Arc::ptr_eq(&a, &other));
        drop((a, b));
        // Once nobody holds it, the next ask starts afresh.
        let c = clip_lock("m\u{1}Kore\u{1}Cześć");
        assert_eq!(std::sync::Arc::strong_count(&c), 1);
    }

    #[test]
    fn cache_keys_separate_voices_and_models() {
        let a = audio_key("google/tts", "Kore", "Cześć");
        assert_eq!(a, audio_key("google/tts", "Kore", "Cześć"));
        assert_ne!(a, audio_key("google/tts", "Puck", "Cześć"));
        assert_ne!(a, audio_key("openai/tts", "Kore", "Cześć"));
        assert_ne!(a, audio_key("google/tts", "Kore", "Dobranoc"));
        // The separator must stop adjacent fields from colliding.
        assert_ne!(audio_key("a", "b", "c"), audio_key("ab", "", "c"));
    }
}
