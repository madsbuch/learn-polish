use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::ai;
use crate::blocks;
use crate::db::{self, Db};
use crate::error::{Error, Result};
use crate::models::*;
use crate::programmes;
use crate::session;
use crate::songs;
use crate::srs;
use crate::stats;

// ---------------------------------------------------------------------------
// Answer checking
// ---------------------------------------------------------------------------

/// Folds away everything a learner should not be penalised for: case,
/// punctuation, doubled spaces and missing Polish diacritics. Typing "czesc"
/// counts as "cześć" — the correct spelling is shown in the feedback anyway.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_space = true;
    for ch in text.trim().chars() {
        let folded = match ch.to_lowercase().next().unwrap_or(ch) {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            other => other,
        };
        if folded.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else if folded.is_alphanumeric() {
            out.push(folded);
            last_was_space = false;
        }
        // Punctuation is dropped entirely.
    }
    out.trim_end().to_string()
}

fn answers_match(expected: &str, given: &str) -> bool {
    let given = normalize(given);
    if normalize(expected).is_empty() {
        return true;
    }
    if normalize(expected) == given {
        return true;
    }
    // Glosses often carry alternatives ("piece, bit"); any one of them will do.
    // The split happens before normalising, which strips the separator.
    expected
        .split(',')
        .map(normalize)
        .any(|alternative| !alternative.is_empty() && alternative == given)
}

// ---------------------------------------------------------------------------
// Setup and settings
// ---------------------------------------------------------------------------

fn read_settings(conn: &Connection) -> Result<Settings> {
    Ok(Settings {
        daily_goal: db::get_setting_i64(conn, "daily_goal", stats::DEFAULT_DAILY_GOAL),
        reminder_hour: db::get_setting_i64(conn, "reminder_hour", 19),
        reminder_minute: db::get_setting_i64(conn, "reminder_minute", 0),
        reminders_enabled: db::get_setting_i64(conn, "reminders_enabled", 1) == 1,
        has_api_key: db::get_setting(conn, "openrouter_api_key")?
            .is_some_and(|k| !k.trim().is_empty()),
        ai_model: ai::stored_model(conn, "ai_model"),
        tts_voice_female: ai::voice_for(conn, "f"),
        tts_voice_male: ai::voice_for(conn, "m"),
        tts_speed: db::get_setting(conn, "tts_speed")?
            .and_then(|value| value.parse().ok())
            .unwrap_or(1.0),
        tts_prefetch: db::get_setting_i64(conn, "tts_prefetch", 1) == 1,
        tz_offset_minutes: db::get_setting_i64(conn, "tz_offset_minutes", 0),
        songwriter_model: ai::stored_model(conn, "songwriter_model"),
        has_kie_key: db::get_setting(conn, "kie_api_key")?.is_some_and(|k| !k.trim().is_empty()),
        suno_model: songs::suno_model(conn),
        suno_models: songs::SUNO_MODELS.iter().map(|m| m.to_string()).collect(),
        song_length: songs::song_length(conn),
        theme: db::get_setting(conn, "theme")?
            .filter(|t| matches!(t.as_str(), "system" | "light" | "dark"))
            .unwrap_or_default(),
        player_repeat: db::get_setting(conn, "player_repeat")?
            .filter(|t| matches!(t.as_str(), "off" | "all" | "one"))
            .unwrap_or_default(),
    })
}

/// Called once at startup. The frontend reports the device's UTC offset because
/// resolving it inside a multi-threaded process is not portable — every "day"
/// boundary in the app depends on this value.
#[tauri::command]
pub async fn init_app(db: State<'_, Db>, tz_offset_minutes: i64) -> Result<Settings> {
    let conn = db.lock();
    db::set_setting(&conn, "tz_offset_minutes", &tz_offset_minutes.to_string())?;
    if db::get_setting(&conn, "first_launch_day")?.is_none() {
        let today = db::today(&conn);
        db::set_setting(&conn, "first_launch_day", &today.to_string())?;
    }
    read_settings(&conn)
}

#[tauri::command]
pub async fn get_settings(db: State<'_, Db>) -> Result<Settings> {
    let conn = db.lock();
    read_settings(&conn)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub daily_goal: Option<i64>,
    pub reminder_hour: Option<i64>,
    pub reminder_minute: Option<i64>,
    pub reminders_enabled: Option<bool>,
    pub ai_model: Option<String>,
    pub tts_voice_female: Option<String>,
    pub tts_voice_male: Option<String>,
    pub tts_speed: Option<f32>,
    pub tts_prefetch: Option<bool>,
    pub songwriter_model: Option<String>,
    pub suno_model: Option<String>,
    pub song_length: Option<String>,
    pub theme: Option<String>,
    pub player_repeat: Option<String>,
}

#[tauri::command]
pub async fn update_settings(db: State<'_, Db>, patch: SettingsPatch) -> Result<Settings> {
    let conn = db.lock();
    if let Some(theme) = patch.theme {
        if !matches!(theme.as_str(), "system" | "light" | "dark") {
            return Err(Error::msg(format!("Unknown theme '{theme}'.")));
        }
        db::set_setting(&conn, "theme", &theme)?;
    }
    if let Some(repeat) = patch.player_repeat {
        if !matches!(repeat.as_str(), "off" | "all" | "one") {
            return Err(Error::msg(format!("Unknown repeat mode '{repeat}'.")));
        }
        db::set_setting(&conn, "player_repeat", &repeat)?;
    }
    if let Some(goal) = patch.daily_goal {
        db::set_setting(&conn, "daily_goal", &goal.clamp(10, 1000).to_string())?;
    }
    if let Some(hour) = patch.reminder_hour {
        db::set_setting(&conn, "reminder_hour", &hour.clamp(0, 23).to_string())?;
    }
    if let Some(minute) = patch.reminder_minute {
        db::set_setting(&conn, "reminder_minute", &minute.clamp(0, 59).to_string())?;
    }
    if let Some(enabled) = patch.reminders_enabled {
        db::set_setting(&conn, "reminders_enabled", if enabled { "1" } else { "0" })?;
    }
    if let Some(model) = patch.ai_model {
        db::set_setting(&conn, "ai_model", model.trim())?;
    }
    if let Some(voice) = patch.tts_voice_female {
        db::set_setting(&conn, "tts_voice_female", voice.trim())?;
    }
    if let Some(voice) = patch.tts_voice_male {
        db::set_setting(&conn, "tts_voice_male", voice.trim())?;
    }
    if let Some(speed) = patch.tts_speed {
        db::set_setting(&conn, "tts_speed", &speed.clamp(0.25, 2.0).to_string())?;
    }
    if let Some(prefetch) = patch.tts_prefetch {
        db::set_setting(&conn, "tts_prefetch", if prefetch { "1" } else { "0" })?;
    }
    if let Some(model) = patch.songwriter_model {
        db::set_setting(&conn, "songwriter_model", model.trim())?;
    }
    if let Some(length) = patch.song_length {
        if !songs::SONG_LENGTHS.contains(&length.as_str()) {
            return Err(Error::msg(format!("Unknown song length '{length}'.")));
        }
        db::set_setting(&conn, "song_length", &length)?;
    }
    if let Some(model) = patch.suno_model {
        if !songs::SUNO_MODELS.contains(&model.as_str()) {
            return Err(Error::msg(format!("Unknown Suno model '{model}'.")));
        }
        db::set_setting(&conn, "suno_model", &model)?;
    }
    read_settings(&conn)
}

// ---------------------------------------------------------------------------
// The daily session
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitAnswer {
    pub kind: String,
    pub item_kind: String,
    pub item_id: i64,
    pub is_new: bool,
    pub expected: String,
    pub given: String,
    /// Optional override; normally derived from whether the answer was right.
    pub grade: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerResult {
    pub correct: bool,
    pub expected: String,
    pub xp_gained: i64,
    pub total_xp: i64,
    pub day_xp: i64,
    pub level: i64,
}

#[tauri::command]
pub async fn submit_answer(db: State<'_, Db>, answer: SubmitAnswer) -> Result<AnswerResult> {
    let conn = db.lock();
    let today = db::today(&conn);

    let correct = answers_match(&answer.expected, &answer.given);
    let grade = answer
        .grade
        .unwrap_or(if correct { srs::GOOD } else { srs::AGAIN })
        .clamp(srs::AGAIN, srs::EASY);

    // Conversation gaps exercise the dialogue rather than a scheduled item, so
    // they earn XP without creating review rows.
    if answer.item_kind == "word" || answer.item_kind == "sentence" {
        let state = srs::load(&conn, &answer.item_kind, answer.item_id)?
            .unwrap_or_else(|| srs::ReviewState::fresh(today));
        let next = srs::grade(state, grade, today);
        srs::save(&conn, &answer.item_kind, answer.item_id, &next)?;
    }

    let xp = stats::xp_for_answer(&answer.kind, correct, answer.is_new);
    stats::record_answer(&conn, today, xp, correct)?;

    let total_xp = stats::total_xp(&conn)?;
    let day_xp = conn
        .query_row("SELECT xp FROM day_log WHERE day = ?1", [today], |r| r.get(0))
        .unwrap_or(0);

    Ok(AnswerResult {
        correct,
        expected: answer.expected,
        xp_gained: xp,
        total_xp,
        day_xp,
        level: stats::level_from_xp(total_xp),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResult {
    pub first_completion_today: bool,
    pub newly_unlocked: Vec<Achievement>,
    pub stats: Stats,
}

/// Achievements unlocked by whatever just happened, with fresh stats.
fn finish_up(conn: &Connection, before: &std::collections::HashSet<String>, first: bool) -> Result<SessionResult> {
    // collect() writes newly earned achievements as a side effect.
    let stats = stats::collect(conn)?;
    let newly_unlocked = stats
        .achievements
        .iter()
        .filter(|a| a.unlocked && !before.contains(&a.code))
        .cloned()
        .collect();
    Ok(SessionResult { first_completion_today: first, newly_unlocked, stats })
}

fn unlocked_codes(conn: &Connection) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare("SELECT code FROM achievements")?;
    let codes = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(codes)
}

// ---------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn blocks_overview(db: State<'_, Db>) -> Result<blocks::Overview> {
    blocks::overview(&db.lock())
}

#[tauri::command]
pub async fn blocks_for_topic(db: State<'_, Db>, topic_id: i64) -> Result<Vec<blocks::Block>> {
    blocks::for_topic(&db.lock(), topic_id)
}

#[tauri::command]
pub async fn block_detail(db: State<'_, Db>, block_id: i64) -> Result<blocks::BlockDetail> {
    blocks::detail(&db.lock(), block_id)
}

/// The exercises of a drill block.
#[tauri::command]
pub async fn block_session(db: State<'_, Db>, block_id: i64) -> Result<SessionPlan> {
    session::build_block(&db.lock(), block_id)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockFinished {
    pub block: blocks::Block,
    /// "pass" | "partial" | "fail"
    pub result: String,
    #[serde(flatten)]
    pub session: SessionResult,
}

/// A drill is done: its score becomes a result that schedules the next
/// check-in, and the day counts toward the streak.
#[tauri::command]
pub async fn block_finish(db: State<'_, Db>, block_id: i64, correct: i64, total: i64) -> Result<BlockFinished> {
    let conn = db.lock();
    let before = unlocked_codes(&conn)?;
    let result = blocks::result_from_score(correct, total);
    let block = blocks::record(&conn, block_id, result)?;
    let first = stats::mark_practised(&conn, db::today(&conn))?;
    Ok(BlockFinished { block, result: result.into(), session: finish_up(&conn, &before, first)? })
}

/// The learner changes how often a block checks in.
#[tauri::command]
pub async fn block_set_period(db: State<'_, Db>, block_id: i64, period_days: i64) -> Result<blocks::Block> {
    blocks::update(
        &db.lock(),
        block_id,
        blocks::BlockChanges { period_days: Some(period_days), ..Default::default() },
    )
}

#[tauri::command]
pub async fn block_due_now(db: State<'_, Db>, block_id: i64) -> Result<blocks::Block> {
    blocks::due_now(&db.lock(), block_id)
}

/// The listening block for a song, made on first ask.
#[tauri::command]
pub async fn block_for_song(db: State<'_, Db>, song_id: i64) -> Result<blocks::Block> {
    blocks::create_listening(&db.lock(), song_id, None, None, "tutor")
}

#[tauri::command]
pub async fn block_delete(db: State<'_, Db>, block_id: i64) -> Result<()> {
    blocks::delete(&db.lock(), block_id).map(|_| ())
}

/// Stats after a study check-in, for the summary screen.
#[tauri::command]
pub async fn checkin_summary(db: State<'_, Db>, first_completion_today: bool) -> Result<SessionResult> {
    let conn = db.lock();
    finish_up(&conn, &Default::default(), first_completion_today)
}

// ---------------------------------------------------------------------------
// Progress and browsing
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_stats(db: State<'_, Db>) -> Result<Stats> {
    let conn = db.lock();
    stats::collect(&conn)
}

#[tauri::command]
pub async fn get_topic_progress(db: State<'_, Db>) -> Result<Vec<TopicProgress>> {
    let conn = db.lock();
    stats::topic_progress(&conn)
}

#[tauri::command]
pub async fn get_dialogue(db: State<'_, Db>, topic_id: i64) -> Result<Dialogue> {
    let conn = db.lock();
    Ok(Dialogue {
        topic: session::load_topic(&conn, topic_id)?,
        lines: session::dialogue_lines(&conn, topic_id)?,
    })
}

#[tauri::command]
pub async fn browse_words(
    db: State<'_, Db>,
    topic_id: Option<i64>,
    query: String,
    limit: i64,
) -> Result<Vec<Word>> {
    let conn = db.lock();
    let meaning = "en";
    let pattern = format!("%{}%", query.trim());
    let sql = format!(
        "SELECT id, pl, {meaning}, pos FROM words
         WHERE (?1 IS NULL OR topic_id = ?1)
           AND (?2 = '%%' OR pl LIKE ?2 OR {meaning} LIKE ?2)
         ORDER BY topic_id, ord LIMIT ?3"
    );
    let mut stmt = conn.prepare(&sql)?;
    let words = stmt
        .query_map(rusqlite::params![topic_id, pattern, limit.clamp(1, 500)], |r| {
            Ok(Word { id: r.get(0)?, pl: r.get(1)?, meaning: r.get(2)?, pos: r.get(3)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(words)
}

/// One catalogue page: words and sentences matching a search, plus how many
/// matched in total so the UI can say what it is not showing.
#[tauri::command]
pub async fn browse_catalogue(
    db: State<'_, Db>,
    topic_id: Option<i64>,
    query: String,
    limit: i64,
) -> Result<CataloguePage> {
    let conn = db.lock();
    let meaning = "en";
    let needle = query.trim();
    let pattern = format!("%{needle}%");
    let limit = limit.clamp(1, 1000);

    // `?2 = ''` short-circuits an empty search to "everything".
    let filter = format!(
        "(?1 IS NULL OR topic_id = ?1) AND (?2 = '' OR pl LIKE ?3 OR {meaning} LIKE ?3)"
    );

    let mut stmt = conn.prepare(&format!(
        "SELECT id, pl, {meaning}, pos FROM words WHERE {filter} ORDER BY topic_id, ord LIMIT ?4"
    ))?;
    let words = stmt
        .query_map(rusqlite::params![topic_id, needle, pattern, limit], |r| {
            Ok(Word { id: r.get(0)?, pl: r.get(1)?, meaning: r.get(2)?, pos: r.get(3)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    let mut stmt = conn.prepare(&format!(
        "SELECT id, pl, {meaning} FROM sentences WHERE {filter} ORDER BY topic_id, ord LIMIT ?4"
    ))?;
    let sentences = stmt
        .query_map(rusqlite::params![topic_id, needle, pattern, limit], |r| {
            Ok(Sentence { id: r.get(0)?, pl: r.get(1)?, meaning: r.get(2)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    let count = |table: &str| -> Result<i64> {
        Ok(conn.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE {filter}"),
            rusqlite::params![topic_id, needle, pattern],
            |r| r.get(0),
        )?)
    };

    Ok(CataloguePage {
        total_words: count("words")?,
        total_sentences: count("sentences")?,
        words,
        sentences,
    })
}

#[tauri::command]
pub async fn browse_sentences(db: State<'_, Db>, topic_id: i64) -> Result<Vec<Sentence>> {
    let conn = db.lock();
    let meaning = "en";
    let mut stmt = conn.prepare(&format!(
        "SELECT id, pl, {meaning} FROM sentences WHERE topic_id = ?1 ORDER BY ord"
    ))?;
    let sentences = stmt
        .query_map([topic_id], |r| {
            Ok(Sentence { id: r.get(0)?, pl: r.get(1)?, meaning: r.get(2)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(sentences)
}

#[tauri::command]
pub async fn reset_progress(db: State<'_, Db>) -> Result<()> {
    let conn = db.lock();
    conn.execute_batch(
        "DELETE FROM reviews;
         DELETE FROM day_log;
         DELETE FROM achievements;
         DELETE FROM tutor_messages;
         DELETE FROM tutor_conversations;
         DELETE FROM checkins;
         UPDATE blocks SET due_day = NULL, last_day = NULL, last_result = NULL, reps = 0, interval_days = 0;
         DELETE FROM settings WHERE key IN ('lesson_index', 'focus_topic_id');",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// AI — all network access happens here, never in the webview
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn ai_set_key(db: State<'_, Db>, key: String) -> Result<Settings> {
    let conn = db.lock();
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::msg("The API key cannot be empty."));
    }
    db::set_setting(&conn, "openrouter_api_key", key)?;
    read_settings(&conn)
}

#[tauri::command]
pub async fn ai_clear_key(db: State<'_, Db>) -> Result<Settings> {
    let conn = db.lock();
    conn.execute("DELETE FROM settings WHERE key = 'openrouter_api_key'", [])?;
    read_settings(&conn)
}

#[tauri::command]
pub async fn kie_set_key(db: State<'_, Db>, key: String) -> Result<Settings> {
    let conn = db.lock();
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::msg("The API key cannot be empty."));
    }
    db::set_setting(&conn, "kie_api_key", key)?;
    read_settings(&conn)
}

#[tauri::command]
pub async fn kie_clear_key(db: State<'_, Db>) -> Result<Settings> {
    let conn = db.lock();
    conn.execute("DELETE FROM settings WHERE key = 'kie_api_key'", [])?;
    read_settings(&conn)
}

#[tauri::command]
pub async fn ai_list_models() -> Result<Vec<AiModel>> {
    ai::list_models().await
}

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

/// True when this exact line is already stored, so the UI can show which lines
/// will play instantly and which will cost a request.
#[tauri::command]
pub async fn audio_is_cached(db: State<'_, Db>, text: String, gender: Option<String>) -> Result<bool> {
    let conn = db.lock();
    let Ok(config) = speech_config(&conn, &gender.unwrap_or_default()) else {
        return Ok(false);
    };
    Ok(cached_clip(&conn, &audio_key(&config.model, &config.voice, text.trim()))?.is_some())
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
pub async fn cache_audio(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    lines: Vec<SpeechLine>,
) -> Result<CacheProgress> {
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
#[tauri::command]
pub async fn topic_audio_lines(db: State<'_, Db>, topic_id: i64) -> Result<Vec<SpeechLine>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT pl, '' FROM sentences WHERE topic_id = ?1
         UNION ALL
         SELECT pl, gender FROM conversation_lines WHERE topic_id = ?1",
    )?;
    let lines = stmt
        .query_map([topic_id], |r| {
            Ok(SpeechLine { text: r.get(0)?, gender: r.get(1)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(lines)
}

#[tauri::command]
pub async fn audio_cache_stats(db: State<'_, Db>) -> Result<AudioCacheStats> {
    let conn = db.lock();
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
pub async fn clear_audio_cache(db: State<'_, Db>) -> Result<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM audio_cache", [])?;
    // Reclaim the space; audio is by far the largest thing in the database.
    conn.execute_batch("VACUUM")?;
    Ok(())
}

/// Verifies the stored key with the cheapest possible round trip.
#[tauri::command]
pub async fn ai_test_key(db: State<'_, Db>) -> Result<String> {
    let (key, model) = {
        let conn = db.lock();
        (ai::api_key(&conn)?, ai::require_model(&conn)?)
    };
    ai::complete(
        &key,
        &model,
        &[ai::Wire::user("Reply with exactly: OK")],
    )
    .await
    .map(|reply| reply.trim().to_string())
}

/// Pushes a streamed answer to the frontend as it arrives.
///
/// Every stream carries a caller-supplied id, so a screen that fires two
/// requests (or is re-entered) never renders another one's tokens.
pub fn stream_to_frontend(
    app: &tauri::AppHandle,
    stream_id: &str,
    delta: Option<&str>,
    done: bool,
    error: Option<String>,
) {
    let _ = app.emit(
        "ai://stream",
        StreamEvent {
            id: stream_id.to_string(),
            delta: delta.unwrap_or_default().to_string(),
            done,
            error,
        },
    );
}

async fn run_stream(
    app: tauri::AppHandle,
    stream_id: String,
    key: String,
    model: String,
    messages: Vec<ai::Wire>,
) -> Result<String> {
    let emitter = app.clone();
    let id = stream_id.clone();
    let result = ai::complete_streaming(&key, &model, &messages, move |delta| {
        stream_to_frontend(&emitter, &id, Some(delta), false, None);
    })
    .await;

    match &result {
        Ok(_) => stream_to_frontend(&app, &stream_id, None, true, None),
        Err(error) => {
            stream_to_frontend(&app, &stream_id, None, true, Some(error.to_string()))
        }
    }
    result
}

/// Explains a word or sentence. Text arrives as `ai://stream` events.
#[tauri::command]
pub async fn ai_explain_stream(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    item_kind: String,
    item_id: i64,
    stream_id: String,
) -> Result<String> {
    let (key, model, messages) = {
        let conn = db.lock();
        let key = ai::api_key(&conn)?;
        let model = ai::require_model(&conn)?;
        let table = match item_kind.as_str() {
            "word" => "words",
            "sentence" => "sentences",
            other => return Err(Error::msg(format!("cannot explain a '{other}'"))),
        };
        let (polish, gloss): (String, String) = conn.query_row(
            &format!("SELECT pl, en FROM {table} WHERE id = ?1"),
            [item_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        (key, model, ai::explain_prompt(&polish, &gloss))
    };
    run_stream(app, stream_id, key, model, messages).await
}

// ---------------------------------------------------------------------------
// Programmes and the daily focus
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn programme_delete(db: State<'_, Db>, topic_id: i64) -> Result<()> {
    programmes::delete(&db.lock(), topic_id).map(|_| ())
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// A civil date from days since the epoch (Howard Hinnant's algorithm), for
/// the export's file name.
fn civil_date(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Writes a consistent copy of the whole database — progress, programmes,
/// tutor conversation, settings, cached audio and songs — to a file the
/// learner picks. API keys are left out unless asked for. Returns where it
/// went, or `None` if the learner cancelled the dialog.
#[tauri::command]
pub async fn export_database(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    include_keys: bool,
) -> Result<Option<String>> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;
    use tauri_plugin_fs::FsExt;

    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| Error::msg(format!("no cache directory: {e}")))?;
    std::fs::create_dir_all(&dir)?;
    let snapshot = dir.join("export.sqlite");
    let _ = std::fs::remove_file(&snapshot);

    let file_name = {
        let conn = db.lock();
        // VACUUM INTO writes a compact, self-contained copy that includes
        // whatever is still in the write-ahead log.
        conn.execute("VACUUM INTO ?1", [snapshot.to_string_lossy()])?;
        let (y, m, d) = civil_date(db::today(&conn));
        format!("polski-{y:04}-{m:02}-{d:02}.sqlite")
    };

    if !include_keys {
        let copy = rusqlite::Connection::open(&snapshot)?;
        // Overwrite freed pages too, so the keys are really gone from the file.
        copy.pragma_update(None, "secure_delete", "ON")?;
        copy.execute(
            "DELETE FROM settings WHERE key IN ('openrouter_api_key', 'kie_api_key')",
            [],
        )?;
    }

    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Export your data")
        .set_file_name(&file_name)
        .add_filter("SQLite database", &["sqlite", "db"])
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(target) = receiver.await.ok().flatten() else {
        let _ = std::fs::remove_file(&snapshot);
        return Ok(None);
    };

    let shown = target.to_string();
    let result = (|| -> std::io::Result<u64> {
        let mut options = tauri_plugin_fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        let mut out = app.fs().open(target, options)?;
        let mut source = std::fs::File::open(&snapshot)?;
        let copied = std::io::copy(&mut source, &mut out)?;
        out.sync_all().ok();
        Ok(copied)
    })();
    let _ = std::fs::remove_file(&snapshot);
    result.map_err(|e| Error::msg(format!("Could not write the export: {e}")))?;
    Ok(Some(shown))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_is_forgiving_about_diacritics_and_case() {
        assert!(answers_match("cześć", "Czesc"));
        assert!(answers_match("dzień dobry", "  dzien   dobry "));
        assert!(answers_match("Do widzenia!", "do widzenia"));
        assert!(!answers_match("kot", "pies"));
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

    #[test]
    fn export_names_carry_the_calendar_date() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(20_405), (2025, 11, 13));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
    }

    #[test]
    fn any_listed_gloss_counts() {
        assert!(answers_match("piece, bit", "bit"));
        assert!(answers_match("piece, bit", "piece"));
        assert!(answers_match("piece, bit", "piece, bit"));
        assert!(!answers_match("piece, bit", "chunk"));
    }
}
