//! App-level commands: setup and settings, API keys and models, stats and
//! browsing the catalogue, explanations, reset and export.
//!
//! Commands that belong to one part of the app live in that part's module
//! (blocks, courses, practice, speech, songs, playlists, tutor, examiner,
//! memories, costs, cast); this is what is left over.

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::Text;
use serde::Deserialize;
use tauri::State;

use crate::agent::{Agent, ToolFuture, ToolOutput};
use crate::ai;
use crate::costs;
use crate::db::{self, Conn, Db};
use crate::schema::{blocks, course_sentences, course_words, sentences, words};
use crate::error::{Error, Result};
use crate::models::*;
use crate::songs;
use crate::stats;

// ---------------------------------------------------------------------------
// Setup and settings
// ---------------------------------------------------------------------------

fn read_settings(conn: &mut Conn) -> Result<Settings> {
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
    let mut guard = db.lock();
    let conn = &mut *guard;
    db::set_setting(conn, "tz_offset_minutes", &tz_offset_minutes.to_string())?;
    if db::get_setting(conn, "first_launch_day")?.is_none() {
        let today = db::today(conn);
        db::set_setting(conn, "first_launch_day", &today.to_string())?;
    }
    read_settings(conn)
}

#[tauri::command]
pub async fn get_settings(db: State<'_, Db>) -> Result<Settings> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    read_settings(conn)
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
    let mut guard = db.lock();
    let conn = &mut *guard;
    if let Some(theme) = patch.theme {
        if !matches!(theme.as_str(), "system" | "light" | "dark") {
            return Err(Error::msg(format!("Unknown theme '{theme}'.")));
        }
        db::set_setting(conn, "theme", &theme)?;
    }
    if let Some(repeat) = patch.player_repeat {
        if !matches!(repeat.as_str(), "off" | "all" | "one") {
            return Err(Error::msg(format!("Unknown repeat mode '{repeat}'.")));
        }
        db::set_setting(conn, "player_repeat", &repeat)?;
    }
    if let Some(goal) = patch.daily_goal {
        db::set_setting(conn, "daily_goal", &goal.clamp(10, 1000).to_string())?;
    }
    if let Some(hour) = patch.reminder_hour {
        db::set_setting(conn, "reminder_hour", &hour.clamp(0, 23).to_string())?;
    }
    if let Some(minute) = patch.reminder_minute {
        db::set_setting(conn, "reminder_minute", &minute.clamp(0, 59).to_string())?;
    }
    if let Some(enabled) = patch.reminders_enabled {
        db::set_setting(conn, "reminders_enabled", if enabled { "1" } else { "0" })?;
    }
    if let Some(model) = patch.ai_model {
        db::set_setting(conn, "ai_model", model.trim())?;
    }
    if let Some(voice) = patch.tts_voice_female {
        db::set_setting(conn, "tts_voice_female", voice.trim())?;
    }
    if let Some(voice) = patch.tts_voice_male {
        db::set_setting(conn, "tts_voice_male", voice.trim())?;
    }
    if let Some(speed) = patch.tts_speed {
        db::set_setting(conn, "tts_speed", &speed.clamp(0.25, 2.0).to_string())?;
    }
    if let Some(prefetch) = patch.tts_prefetch {
        db::set_setting(conn, "tts_prefetch", if prefetch { "1" } else { "0" })?;
    }
    if let Some(model) = patch.songwriter_model {
        db::set_setting(conn, "songwriter_model", model.trim())?;
    }
    if let Some(length) = patch.song_length {
        if !songs::SONG_LENGTHS.contains(&length.as_str()) {
            return Err(Error::msg(format!("Unknown song length '{length}'.")));
        }
        db::set_setting(conn, "song_length", &length)?;
    }
    if let Some(model) = patch.suno_model {
        if !songs::SUNO_MODELS.contains(&model.as_str()) {
            return Err(Error::msg(format!("Unknown Suno model '{model}'.")));
        }
        db::set_setting(conn, "suno_model", &model)?;
    }
    read_settings(conn)
}

// ---------------------------------------------------------------------------
// Progress and browsing
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_stats(db: State<'_, Db>) -> Result<Stats> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    stats::collect(conn)
}

#[tauri::command]
pub async fn get_topic_progress(db: State<'_, Db>) -> Result<Vec<TopicProgress>> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    stats::topic_progress(conn)
}

/// Words in one course, or in all of them, whose Polish or English contains
/// `needle`, in course order. A word in a block two courses list is one word.
fn matching_words(conn: &mut Conn, topic_id: Option<i64>, needle: &str) -> Result<Vec<Word>> {
    let mut query = course_words::table
        .inner_join(blocks::table)
        .order((course_words::topic_id, course_words::ord))
        .select((
            course_words::id,
            course_words::pl,
            course_words::en,
            course_words::pos,
            course_words::block_id,
            blocks::title,
        ))
        .into_boxed();
    if let Some(id) = topic_id {
        query = query.filter(course_words::topic_id.eq(id));
    }
    if !needle.is_empty() {
        let pattern = format!("%{needle}%");
        query = query.filter(course_words::pl.like(pattern.clone()).or(course_words::en.like(pattern)));
    }
    let mut seen = std::collections::HashSet::new();
    Ok(query
        .load::<(i64, String, String, String, i64, String)>(conn)?
        .into_iter()
        .filter(|row| seen.insert(row.0))
        .map(|(id, pl, meaning, pos, block_id, block)| Word { id, pl, meaning, pos, block_id, block })
        .collect())
}

/// The same for sentences.
fn matching_sentences(conn: &mut Conn, topic_id: Option<i64>, needle: &str) -> Result<Vec<Sentence>> {
    let mut query = course_sentences::table
        .inner_join(blocks::table)
        .order((course_sentences::topic_id, course_sentences::ord))
        .select((
            course_sentences::id,
            course_sentences::pl,
            course_sentences::en,
            course_sentences::block_id,
            blocks::title,
        ))
        .into_boxed();
    if let Some(id) = topic_id {
        query = query.filter(course_sentences::topic_id.eq(id));
    }
    if !needle.is_empty() {
        let pattern = format!("%{needle}%");
        query = query.filter(course_sentences::pl.like(pattern.clone()).or(course_sentences::en.like(pattern)));
    }
    let mut seen = std::collections::HashSet::new();
    Ok(query
        .load::<(i64, String, String, i64, String)>(conn)?
        .into_iter()
        .filter(|row| seen.insert(row.0))
        .map(|(id, pl, meaning, block_id, block)| Sentence { id, pl, meaning, block_id, block })
        .collect())
}

/// The query behind [`browse_words`], callable without Tauri (tests).
pub fn browse_words_in(conn: &mut Conn, topic_id: Option<i64>, query: String, limit: i64) -> Result<Vec<Word>> {
    let mut words = matching_words(conn, topic_id, query.trim())?;
    words.truncate(limit.clamp(1, 500) as usize);
    Ok(words)
}

#[tauri::command]
pub async fn browse_words(db: State<'_, Db>, topic_id: Option<i64>, query: String, limit: i64) -> Result<Vec<Word>> {
    browse_words_in(&mut db.lock(), topic_id, query, limit)
}

/// One catalogue page: words and sentences matching a search, plus how many
/// matched in total so the UI can say what it is not showing.
pub fn browse_catalogue_in(conn: &mut Conn, topic_id: Option<i64>, query: String, limit: i64) -> Result<CataloguePage> {
    let limit = limit.clamp(1, 1000) as usize;
    let mut words = matching_words(conn, topic_id, query.trim())?;
    let mut sentences = matching_sentences(conn, topic_id, query.trim())?;
    let (total_words, total_sentences) = (words.len() as i64, sentences.len() as i64);
    words.truncate(limit);
    sentences.truncate(limit);
    Ok(CataloguePage { words, sentences, total_words, total_sentences })
}

#[tauri::command]
pub async fn browse_catalogue(db: State<'_, Db>, topic_id: Option<i64>, query: String, limit: i64) -> Result<CataloguePage> {
    browse_catalogue_in(&mut db.lock(), topic_id, query, limit)
}

/// A course's sentences, in order.
pub fn browse_sentences_in(conn: &mut Conn, topic_id: i64) -> Result<Vec<Sentence>> {
    matching_sentences(conn, Some(topic_id), "")
}

#[tauri::command]
pub async fn browse_sentences(db: State<'_, Db>, topic_id: i64) -> Result<Vec<Sentence>> {
    browse_sentences_in(&mut db.lock(), topic_id)
}

#[tauri::command]
pub async fn reset_progress(db: State<'_, Db>) -> Result<()> {
    use crate::schema::{achievements, checkins, day_log, reviews, tutor_conversations, tutor_messages};
    let mut guard = db.lock();
    guard.transaction::<_, Error, _>(|conn| {
        diesel::delete(reviews::table).execute(conn)?;
        diesel::delete(day_log::table).execute(conn)?;
        diesel::delete(achievements::table).execute(conn)?;
        diesel::delete(tutor_messages::table).execute(conn)?;
        diesel::delete(tutor_conversations::table).execute(conn)?;
        diesel::delete(checkins::table).execute(conn)?;
        diesel::update(blocks::table)
            .set((
                blocks::due_day.eq(None::<i64>),
                blocks::last_day.eq(None::<i64>),
                blocks::last_result.eq(None::<String>),
                blocks::reps.eq(0),
                blocks::interval_days.eq(0),
            ))
            .execute(conn)?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Keys, models and explanations (all network access is in Rust, never the webview)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn ai_set_key(db: State<'_, Db>, key: String) -> Result<Settings> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::msg("The API key cannot be empty."));
    }
    db::set_setting(conn, "openrouter_api_key", key)?;
    read_settings(conn)
}

#[tauri::command]
pub async fn ai_clear_key(db: State<'_, Db>) -> Result<Settings> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    db::delete_setting(conn, "openrouter_api_key")?;
    read_settings(conn)
}

#[tauri::command]
pub async fn kie_set_key(db: State<'_, Db>, key: String) -> Result<Settings> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::msg("The API key cannot be empty."));
    }
    db::set_setting(conn, "kie_api_key", key)?;
    read_settings(conn)
}

#[tauri::command]
pub async fn kie_clear_key(db: State<'_, Db>) -> Result<Settings> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    db::delete_setting(conn, "kie_api_key")?;
    read_settings(conn)
}

#[tauri::command]
pub async fn ai_list_models() -> Result<Vec<AiModel>> {
    ai::list_models().await
}

/// Verifies the stored key with the cheapest possible round trip.
#[tauri::command]
pub async fn ai_test_key(db: State<'_, Db>) -> Result<String> {
    let (key, model) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        (ai::api_key(conn)?, ai::require_model(conn)?)
    };
    let reply = ai::complete(&key, &model, &[ai::Wire::user("Reply with exactly: OK")]).await?;
    let _ = costs::record(&mut db.lock(), costs::KEY_TEST, "openrouter", &model, reply.cost, "");
    Ok(reply.text.trim().to_string())
}

/// Explains a word or sentence. Text arrives as `ai://stream` events.
#[tauri::command]
pub async fn ai_explain_stream( app: tauri::AppHandle, db: State<'_, Db>, item_kind: String, item_id: i64, stream_id: String, ) -> Result<String> {
    let messages = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let (polish, gloss): (String, String) = match item_kind.as_str() {
            "word" => words::table.find(item_id).select((words::pl, words::en)).first(conn)?,
            "sentence" => sentences::table.find(item_id).select((sentences::pl, sentences::en)).first(conn)?,
            other => return Err(Error::msg(format!("cannot explain a '{other}'"))),
        };
        ai::explain_prompt(&polish, &gloss)
    };
    let agent = Agent {
        app: &app,
        db: &db,
        name: costs::EXPLAIN,
        stream_id: &stream_id,
        tools: None,
        max_rounds: 1,
        stop_once_answered: true,
        show_status: false,
    };
    let outcome = agent
        .run(messages, |_| -> ToolFuture<'_, ()> { Box::pin(async { ToolOutput::reply(serde_json::Value::Null) }) })
        .await;
    match outcome.failed {
        Some(error) => Err(error),
        None if outcome.text.trim().is_empty() => Err(Error::msg("OpenRouter returned an empty answer.")),
        None => Ok(outcome.text),
    }
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

/// Writes a consistent copy of the whole database — progress, courses,
/// tutor conversation, settings, cached audio and songs — to a file the
/// learner picks. API keys are left out unless asked for. Returns where it
/// went, or `None` if the learner cancelled the dialog.
#[tauri::command]
pub async fn export_database( app: tauri::AppHandle, db: State<'_, Db>, include_keys: bool, ) -> Result<Option<String>> {
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
        let mut guard = db.lock();
        let conn = &mut *guard;
        // VACUUM INTO writes a compact, self-contained copy that includes
        // whatever is still in the write-ahead log.
        diesel::sql_query("VACUUM INTO ?")
            .bind::<Text, _>(snapshot.to_string_lossy().to_string())
            .execute(conn)?;
        let (y, m, d) = civil_date(db::today(conn));
        format!("polski-{y:04}-{m:02}-{d:02}.sqlite")
    };

    if !include_keys {
        let mut copy = diesel::SqliteConnection::establish(&snapshot.to_string_lossy())?;
        // Overwrite freed pages too, so the keys are really gone from the file.
        copy.batch_execute("PRAGMA secure_delete = ON")?;
        db::delete_setting(&mut copy, "openrouter_api_key")?;
        db::delete_setting(&mut copy, "kie_api_key")?;
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
    fn export_names_carry_the_calendar_date() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(20_405), (2025, 11, 13));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
    }
}
