use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

use crate::content::{self, Conversation};
use crate::error::{Error, Result};

/// Guards the single SQLite connection. Every command locks it briefly; there is
/// no long-running work behind the lock.
pub struct Db(pub Mutex<Connection>);

impl Db {
    pub fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A poisoned lock means an earlier command panicked. The connection is
        // still usable, so recover rather than cascading the panic.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn open(path: &std::path::Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    migrate(&conn)?;
    seed(&conn)?;
    Ok(conn)
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS topics (
    id         INTEGER PRIMARY KEY,
    slug       TEXT    NOT NULL UNIQUE,
    ord        INTEGER NOT NULL,
    title_pl   TEXT    NOT NULL,
    title_en   TEXT    NOT NULL,
    title_da   TEXT    NOT NULL,
    summary_en TEXT    NOT NULL,
    summary_da TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS words (
    id       INTEGER PRIMARY KEY,
    topic_id INTEGER NOT NULL REFERENCES topics(id),
    ord      INTEGER NOT NULL,
    pl       TEXT    NOT NULL UNIQUE,
    en       TEXT    NOT NULL,
    da       TEXT    NOT NULL,
    pos      TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS words_topic ON words(topic_id, ord);

CREATE TABLE IF NOT EXISTS sentences (
    id       INTEGER PRIMARY KEY,
    topic_id INTEGER NOT NULL REFERENCES topics(id),
    ord      INTEGER NOT NULL,
    pl       TEXT    NOT NULL UNIQUE,
    en       TEXT    NOT NULL,
    da       TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS sentences_topic ON sentences(topic_id, ord);

CREATE TABLE IF NOT EXISTS conversation_lines (
    id       INTEGER PRIMARY KEY,
    topic_id INTEGER NOT NULL REFERENCES topics(id),
    ord      INTEGER NOT NULL,
    speaker  TEXT    NOT NULL,
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL,
    da       TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS conversation_topic ON conversation_lines(topic_id, ord);

-- Spaced repetition state, one row per item the learner has actually seen.
CREATE TABLE IF NOT EXISTS reviews (
    kind      TEXT    NOT NULL,          -- 'word' | 'sentence'
    item_id   INTEGER NOT NULL,
    due_day   INTEGER NOT NULL,
    interval  INTEGER NOT NULL,
    ease      REAL    NOT NULL,
    reps      INTEGER NOT NULL,
    lapses    INTEGER NOT NULL,
    last_day  INTEGER NOT NULL,
    PRIMARY KEY (kind, item_id)
);
CREATE INDEX IF NOT EXISTS reviews_due ON reviews(kind, due_day);

CREATE TABLE IF NOT EXISTS day_log (
    day       INTEGER PRIMARY KEY,
    xp        INTEGER NOT NULL DEFAULT 0,
    answered  INTEGER NOT NULL DEFAULT 0,
    correct   INTEGER NOT NULL DEFAULT 0,
    topic_id  INTEGER,
    completed INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS achievements (
    code         TEXT PRIMARY KEY,
    unlocked_day INTEGER NOT NULL,
    unlocked_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS chat_messages (
    id         INTEGER PRIMARY KEY,
    topic_id   INTEGER NOT NULL,
    role       TEXT    NOT NULL,
    content    TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS chat_topic ON chat_messages(topic_id, id);
"#;

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

/// Content version. Bump this when the TSV/JSON files change so existing
/// installs re-seed while keeping their review history (keyed by row id, which
/// is stable because rows are inserted in file order).
const CONTENT_VERSION: i64 = 1;

fn seed(conn: &Connection) -> Result<()> {
    let seeded: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'content_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;

    if seeded.as_deref() == Some(&CONTENT_VERSION.to_string()) {
        return Ok(());
    }

    for (index, topic) in content::TOPICS.iter().enumerate() {
        let convo: Conversation = serde_json::from_str(topic.conversation)?;
        if convo.slug != topic.slug {
            return Err(Error::msg(format!(
                "conversation file for '{}' declares slug '{}'",
                topic.slug, convo.slug
            )));
        }
        let ord = index as i64 + 1;

        conn.execute(
            "INSERT INTO topics (slug, ord, title_pl, title_en, title_da, summary_en, summary_da)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(slug) DO UPDATE SET
                 ord = excluded.ord, title_pl = excluded.title_pl,
                 title_en = excluded.title_en, title_da = excluded.title_da,
                 summary_en = excluded.summary_en, summary_da = excluded.summary_da",
            rusqlite::params![
                topic.slug,
                ord,
                convo.title_pl,
                convo.title_en,
                convo.title_da,
                convo.summary_en,
                convo.summary_da
            ],
        )?;

        let topic_id: i64 = conn.query_row(
            "SELECT id FROM topics WHERE slug = ?1",
            [topic.slug],
            |r| r.get(0),
        )?;

        for (i, w) in content::parse_words(topic.words).iter().enumerate() {
            conn.execute(
                "INSERT INTO words (topic_id, ord, pl, en, da, pos) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(pl) DO UPDATE SET
                     topic_id = excluded.topic_id, ord = excluded.ord,
                     en = excluded.en, da = excluded.da, pos = excluded.pos",
                rusqlite::params![topic_id, i as i64, w.pl, w.en, w.da, w.pos],
            )?;
        }

        for (i, s) in content::parse_sentences(topic.sentences).iter().enumerate() {
            conn.execute(
                "INSERT INTO sentences (topic_id, ord, pl, en, da) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(pl) DO UPDATE SET
                     topic_id = excluded.topic_id, ord = excluded.ord,
                     en = excluded.en, da = excluded.da",
                rusqlite::params![topic_id, i as i64, s.pl, s.en, s.da],
            )?;
        }

        // Conversation lines have no natural key, so replace them wholesale.
        conn.execute("DELETE FROM conversation_lines WHERE topic_id = ?1", [topic_id])?;
        for (i, line) in convo.lines.iter().enumerate() {
            conn.execute(
                "INSERT INTO conversation_lines (topic_id, ord, speaker, pl, en, da)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![topic_id, i as i64, line.speaker, line.pl, line.en, line.da],
            )?;
        }
    }

    set_setting(conn, "content_version", &CONTENT_VERSION.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

pub fn get_setting_i64(conn: &Connection, key: &str, fallback: i64) -> i64 {
    get_setting(conn, key)
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

// ---------------------------------------------------------------------------
// Day arithmetic
// ---------------------------------------------------------------------------

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Days since the epoch **in the learner's local time**. Determining the local
/// offset inside a multi-threaded process is not portable, so the frontend
/// reports it at startup and we store it; the fallback is UTC.
pub fn today(conn: &Connection) -> i64 {
    let offset_minutes = get_setting_i64(conn, "tz_offset_minutes", 0);
    (now_secs() + offset_minutes * 60).div_euclid(86_400)
}

/// The learner's chosen source language: "en" or "da".
pub fn source_language(conn: &Connection) -> String {
    get_setting(conn, "source_language")
        .ok()
        .flatten()
        .filter(|v| v == "en" || v == "da")
        .unwrap_or_else(|| "en".to_string())
}

pub fn require_topic_id(conn: &Connection, ord: i64) -> Result<i64> {
    conn.query_row("SELECT id FROM topics WHERE ord = ?1", [ord], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::msg(format!("no topic with position {ord}")))
}
