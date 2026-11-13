//! Speech: Polish read aloud by Gemini through OpenRouter, the voices to
//! choose from, and the audio cache that makes each line a one-time cost and
//! replays work offline.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::ai;
use crate::costs;
use crate::db::{self, Db};
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
        let conn = db.lock();
        let speed = db::get_setting(&conn, "tts_speed")?
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| (*value - 1.0).abs() > f32::EPSILON);
        (ai::api_key(&conn)?, speed)
    };

    let cached = {
        let conn = db.lock();
        cached_clip(&conn, &audio_key(ai::SPEECH_MODEL, &voice, &text))?
    };
    if let Some((mime, bytes)) = cached {
        return Ok(AudioClip { mime, base64: BASE64.encode(bytes), cached: true });
    }

    let (mime, bytes) =
        ai::synthesize(&key, ai::SPEECH_MODEL, &voice, &text, speed).await?;
    {
        let conn = db.lock();
        record_speech(&conn, ai::SPEECH_MODEL, &text);
        store_clip(
            &conn,
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

fn cached_clip(conn: &Connection, key: &str) -> Result<Option<(String, Vec<u8>)>> {
    Ok(conn
        .query_row(
            "SELECT mime, bytes FROM audio_cache WHERE cache_key = ?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

fn store_clip(
    conn: &Connection,
    key: &str,
    model: &str,
    voice: &str,
    text: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<()> {
    conn.execute(
        "INSERT INTO audio_cache (cache_key, model, voice, text, mime, bytes, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(cache_key) DO UPDATE SET
             mime = excluded.mime, bytes = excluded.bytes",
        rusqlite::params![key, model, voice, text, mime, bytes, db::now_secs()],
    )?;
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
fn speech_config(conn: &Connection, gender: &str) -> Result<SpeechConfig> {
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
        let conn = db.lock();
        let config = speech_config(&conn, &gender)?;
        let hit = cached_clip(&conn, &audio_key(&config.model, &config.voice, &text))?;
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
        let conn = db.lock();
        record_speech(&conn, &config.model, &text);
        store_clip(
            &conn,
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
        let conn = db.lock();
        for line in lines {
            let text = line.text.trim().to_string();
            if text.is_empty() || !seen.insert((text.clone(), line.gender.clone())) {
                continue;
            }
            let config = speech_config(&conn, &line.gender)?;
            let key = audio_key(&config.model, &config.voice, &text);
            if cached_clip(&conn, &key)?.is_none() {
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
                let conn = db.lock();
                record_speech(&conn, &config.model, &line);
                let key = audio_key(&config.model, &config.voice, &line);
                store_clip(&conn, &key, &config.model, &config.voice, &line, &mime, &bytes)?;
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

/// Every Polish line of a topic: its sentences, then its conversation with the
/// gender each line should be read in.
/// The query behind [`topic_audio_lines`], callable without Tauri (tests).
pub fn topic_audio_lines_in(conn: &Connection, topic_id: i64) -> Result<Vec<SpeechLine>> {
    let mut stmt = conn.prepare(
        "SELECT pl, '' FROM course_sentences WHERE topic_id = ?1
         UNION ALL
         SELECT pl, gender FROM course_lines WHERE topic_id = ?1",
    )?;
    let lines = stmt
        .query_map([topic_id], |r| {
            Ok(SpeechLine { text: r.get(0)?, gender: r.get(1)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(lines)
}

#[tauri::command]
pub async fn topic_audio_lines(db: State<'_, Db>, topic_id: i64) -> Result<Vec<SpeechLine>> {
    topic_audio_lines_in(&db.lock(), topic_id)
}

/// The query behind [`audio_cache_stats`], callable without Tauri (tests).
pub fn audio_cache_stats_in(conn: &Connection) -> Result<AudioCacheStats> {
    let (clips, bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(LENGTH(bytes)), 0) FROM audio_cache",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    // Sentences are narrated by the female voice, so that is what counts here.
    let sentences_cached = match speech_config(&conn, "f") {
        Ok(config) => conn.query_row(
            "SELECT COUNT(*) FROM sentences s
             JOIN audio_cache a ON a.text = s.pl
             WHERE a.model = ?1 AND a.voice = ?2",
            rusqlite::params![config.model, config.voice],
            |r| r.get(0),
        )?,
        Err(_) => 0,
    };
    Ok(AudioCacheStats {
        clips,
        bytes,
        sentences_cached,
        sentences_total: conn.query_row("SELECT COUNT(*) FROM sentences", [], |r| r.get(0))?,
    })
}

#[tauri::command]
pub async fn audio_cache_stats(db: State<'_, Db>) -> Result<AudioCacheStats> {
    audio_cache_stats_in(&db.lock())
}

#[tauri::command]
pub async fn clear_audio_cache(db: State<'_, Db>) -> Result<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM audio_cache", [])?;
    // Reclaim the space; audio is by far the largest thing in the database.
    conn.execute_batch("VACUUM")?;
    Ok(())
}

/// OpenRouter's speech endpoint reports no price, so speech is logged as a
/// count with the characters spoken.
fn record_speech(conn: &Connection, model: &str, text: &str) {
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
