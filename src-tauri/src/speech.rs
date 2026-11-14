//! Speech: Polish read aloud by Gemini through OpenRouter, the voices to
//! choose from, and the audio cache that makes each line a one-time cost and
//! replays work offline.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use diesel::connection::SimpleConnection;
use diesel::dsl::sum;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::ai;
use crate::costs;
use crate::db::{self, Conn, Db};
use crate::schema::{audio_cache, course_lines, course_sentences, course_words, sentences, topics};

#[diesel::declare_sql_function]
extern "SQL" {
    /// SQLite's `length()` of a blob, in bytes.
    fn length(bytes: diesel::sql_types::Binary) -> diesel::sql_types::Integer;
}
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
fn audio_key(model: &str, voice: &str, text: &str) -> String {
    format!("{model}\u{1}{voice}\u{1}{text}")
}

fn cached_clip(conn: &mut Conn, key: &str) -> Result<Option<(String, Vec<u8>)>> {
    Ok(audio_cache::table
        .find(key)
        .select((audio_cache::mime, audio_cache::bytes))
        .first(conn)
        .optional()?)
}

fn store_clip(
    conn: &mut Conn,
    key: &str,
    model: &str,
    voice: &str,
    text: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<()> {
    diesel::insert_into(audio_cache::table)
        .values((
            audio_cache::cache_key.eq(key),
            audio_cache::model.eq(model),
            audio_cache::voice.eq(voice),
            audio_cache::text.eq(text),
            audio_cache::mime.eq(mime),
            audio_cache::bytes.eq(bytes),
            audio_cache::created_at.eq(db::now_secs()),
        ))
        .on_conflict(audio_cache::cache_key)
        .do_update()
        .set((audio_cache::mime.eq(mime), audio_cache::bytes.eq(bytes)))
        .execute(conn)?;
    Ok(())
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

/// Speaks a line of Polish, generating it only the first time.
#[tauri::command]
pub async fn speak(db: State<'_, Db>, text: String, gender: Option<String>) -> Result<AudioClip> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(Error::msg("Nothing to say."));
    }
    let gender = gender.unwrap_or_default();

    let (config, cached) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let config = speech_config(conn, &gender)?;
        let hit = cached_clip(conn, &audio_key(&config.model, &config.voice, &text))?;
        (config, hit)
    };

    if let Some((mime, bytes)) = cached {
        return Ok(AudioClip { mime, base64: BASE64.encode(bytes), cached: true });
    }

    let (mime, bytes) = ai::synthesize(
        &config.key,
        &config.model,
        &config.voice,
        &text,
        config.speed,
    )
    .await?;

    {
        let mut guard = db.lock();
        let conn = &mut *guard;
        record_speech(conn, &config.model, &text);
        store_clip(
            conn,
            &audio_key(&config.model, &config.voice, &text),
            &config.model,
            &config.voice,
            &text,
            &mime,
            &bytes,
        )?;
    }

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
/// goes. Used to warm a topic's sentences ahead of practice.
#[tauri::command]
pub async fn cache_audio( app: tauri::AppHandle, db: State<'_, Db>, lines: Vec<SpeechLine>, ) -> Result<CacheProgress> {
    // Drop duplicates and anything already stored before spending a request.
    // A line spoken by two genders is two clips, so the pair is the key.
    let mut pending: Vec<(SpeechConfig, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    {
        let mut guard = db.lock();
        let conn = &mut *guard;
        for line in lines {
            let text = line.text.trim().to_string();
            if text.is_empty() || !seen.insert((text.clone(), line.gender.clone())) {
                continue;
            }
            let config = speech_config(conn, &line.gender)?;
            let key = audio_key(&config.model, &config.voice, &text);
            if cached_clip(conn, &key)?.is_none() {
                pending.push((config, text));
            }
        }
    }

    let total = pending.len() as i64;
    let mut done = 0;
    let mut failed = 0;
    let mut first_error: Option<String> = None;

    for (config, line) in pending {
        match ai::synthesize(&config.key, &config.model, &config.voice, &line, config.speed).await {
            Ok((mime, bytes)) => {
                let mut guard = db.lock();
                let conn = &mut *guard;
                record_speech(conn, &config.model, &line);
                let key = audio_key(&config.model, &config.voice, &line);
                store_clip(conn, &key, &config.model, &config.voice, &line, &mime, &bytes)?;
                done += 1;
            }
            Err(error) => {
                failed += 1;
                if first_error.is_none() {
                    first_error = Some(error.to_string());
                }
                // A bad key or model fails on every line; stop rather than
                // hammering the API two hundred times.
                if failed >= 3 {
                    break;
                }
            }
        }
        let _ = app.emit(
            "tts://progress",
            CacheProgress { done, total, failed, finished: false, error: None },
        );
    }

    let progress = CacheProgress {
        done,
        total,
        failed,
        finished: true,
        error: first_error,
    };
    let _ = app.emit("tts://progress", progress.clone());
    Ok(progress)
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
    Ok(audio_cache::table
        .select((audio_cache::cache_key, length(audio_cache::bytes)))
        .load::<(String, i32)>(conn)?
        .into_iter()
        .map(|(key, size)| (key, i64::from(size)))
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
    let unused: Vec<String> = audio_cache::table
        .select(audio_cache::cache_key)
        .load::<String>(conn)?
        .into_iter()
        .filter(|key| !used.contains(key))
        .collect();
    let mut deleted = 0;
    for chunk in unused.chunks(500) {
        deleted += diesel::delete(audio_cache::table.filter(audio_cache::cache_key.eq_any(chunk))).execute(conn)?;
    }
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
    let clips: i64 = audio_cache::table.count().get_result(conn)?;
    let bytes = audio_cache::table.select(sum(length(audio_cache::bytes))).first::<Option<i64>>(conn)?.unwrap_or(0);
    // Sentences are narrated by the female voice, so that is what counts here.
    let sentences_cached = match speech_config(conn, "f") {
        Ok(config) => sentences::table
            .filter(
                sentences::pl.eq_any(
                    audio_cache::table
                        .filter(audio_cache::model.eq(&config.model))
                        .filter(audio_cache::voice.eq(&config.voice))
                        .select(audio_cache::text),
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
    diesel::delete(audio_cache::table).execute(conn)?;
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
