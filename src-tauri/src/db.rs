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

/// Bumped whenever the shape of the database changes in a way that
/// `CREATE TABLE IF NOT EXISTS` cannot express. Stored in `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 3;

const SCHEMA: &str = r#"
-- A topic is a course: a folder of blocks, with the words, sentences and
-- dialogue its drills practise. `kind` is always 'course' now (it once told
-- built-in topics from the learner's programmes).
CREATE TABLE IF NOT EXISTS topics (
    id         INTEGER PRIMARY KEY,
    slug       TEXT    NOT NULL UNIQUE,
    ord        INTEGER NOT NULL,
    kind       TEXT    NOT NULL DEFAULT 'course',
    title_pl   TEXT    NOT NULL,
    title_en   TEXT    NOT NULL,
    summary_en TEXT    NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0
);

-- A headword is unique within a topic, not globally: a course on colours
-- may well repeat a word the course already teaches.
CREATE TABLE IF NOT EXISTS words (
    id       INTEGER PRIMARY KEY,
    topic_id INTEGER NOT NULL REFERENCES topics(id),
    ord      INTEGER NOT NULL,
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL,
    pos      TEXT    NOT NULL,
    UNIQUE (topic_id, pl)
);
CREATE INDEX IF NOT EXISTS words_topic ON words(topic_id, ord);

CREATE TABLE IF NOT EXISTS sentences (
    id       INTEGER PRIMARY KEY,
    topic_id INTEGER NOT NULL REFERENCES topics(id),
    ord      INTEGER NOT NULL,
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL,
    UNIQUE (topic_id, pl)
);
CREATE INDEX IF NOT EXISTS sentences_topic ON sentences(topic_id, ord);

CREATE TABLE IF NOT EXISTS conversation_lines (
    id       INTEGER PRIMARY KEY,
    topic_id INTEGER NOT NULL REFERENCES topics(id),
    ord      INTEGER NOT NULL,
    speaker  TEXT    NOT NULL,
    gender   TEXT    NOT NULL DEFAULT 'f',
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL
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

-- Generated speech, so a sentence is only ever paid for once and replays work
-- offline. Keyed by the exact text plus the model and voice that produced it,
-- so changing voice does not serve stale audio.
CREATE TABLE IF NOT EXISTS audio_cache (
    cache_key  TEXT PRIMARY KEY,
    model      TEXT NOT NULL,
    voice      TEXT NOT NULL,
    text       TEXT NOT NULL,
    mime       TEXT NOT NULL,
    bytes      BLOB NOT NULL,
    created_at INTEGER NOT NULL
);

-- Blocks: the unit of learning (see blocks.rs). A drill practises the
-- words and sentences listed in block_items; a study block carries a lesson
-- and is checked by an examiner. Each has its own check-in schedule.
CREATE TABLE IF NOT EXISTS blocks (
    id            INTEGER PRIMARY KEY,
    slug          TEXT    UNIQUE,                -- split drills: topic/<id>/…; NULL for made ones
    kind          TEXT    NOT NULL,              -- drill | study
    format        TEXT    NOT NULL DEFAULT '',   -- drills: words | memory | typing | sentences | gaps | dialogue
    topic_id      INTEGER REFERENCES topics(id) ON DELETE CASCADE,
    ord           INTEGER NOT NULL DEFAULT 0,
    title         TEXT    NOT NULL,
    summary       TEXT    NOT NULL DEFAULT '',
    complexity    TEXT    NOT NULL,              -- low | medium | high
    lesson        TEXT    NOT NULL DEFAULT '',
    objectives    TEXT    NOT NULL DEFAULT '[]',
    period_days   INTEGER NOT NULL,
    interval_days INTEGER NOT NULL DEFAULT 0,
    due_day       INTEGER,                       -- NULL until first done
    last_day      INTEGER,
    last_result   TEXT,                          -- pass | partial | fail
    reps          INTEGER NOT NULL DEFAULT 0,
    origin        TEXT    NOT NULL DEFAULT 'tutor', -- starter | tutor | you
    created_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS blocks_topic ON blocks(topic_id, ord);
CREATE INDEX IF NOT EXISTS blocks_due ON blocks(due_day);

CREATE TABLE IF NOT EXISTS block_items (
    block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
    kind     TEXT    NOT NULL,                   -- word | sentence
    item_id  INTEGER NOT NULL,
    ord      INTEGER NOT NULL,
    PRIMARY KEY (block_id, kind, item_id)
);

-- A study block's check-in: an interview with the examiner, and its verdict.
CREATE TABLE IF NOT EXISTS checkins (
    id          INTEGER PRIMARY KEY,
    block_id    INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
    started_at  INTEGER NOT NULL,
    finished_at INTEGER,
    result      TEXT,
    feedback    TEXT
);
CREATE TABLE IF NOT EXISTS checkin_messages (
    id         INTEGER PRIMARY KEY,
    checkin_id INTEGER NOT NULL REFERENCES checkins(id) ON DELETE CASCADE,
    role       TEXT    NOT NULL,
    content    TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);

-- Conversations with the tutor: each has its own history, so a question
-- about grammar does not drag along the context of last week's course.
CREATE TABLE IF NOT EXISTS tutor_conversations (
    id         INTEGER PRIMARY KEY,
    title      TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- The messages of a conversation with the tutor. `actions` is a JSON list of what the tutor
-- did in the app during that turn (courses created, blocks made, …), so
-- the UI can offer buttons and later turns know what already happened.
CREATE TABLE IF NOT EXISTS tutor_messages (
    id         INTEGER PRIMARY KEY,
    role       TEXT    NOT NULL,
    content    TEXT    NOT NULL,
    actions    TEXT,
    created_at INTEGER NOT NULL
);

-- Learning songs. `sections` is the structured lyrics (JSON) the app shows;
-- `lyrics` is exactly what was sent to Suno.
CREATE TABLE IF NOT EXISTS songs (
    id           INTEGER PRIMARY KEY,
    title        TEXT    NOT NULL,
    chorus_pl    TEXT    NOT NULL,
    chorus_en    TEXT    NOT NULL,
    style        TEXT    NOT NULL,
    vocal_gender TEXT    NOT NULL DEFAULT '',
    sections     TEXT    NOT NULL,
    lyrics       TEXT    NOT NULL,
    suno_model   TEXT    NOT NULL,
    task_id      TEXT,
    status       TEXT    NOT NULL,          -- generating | ready | failed
    error        TEXT,
    created_at   INTEGER NOT NULL
);

-- Suno sings every song twice. Each take is stored here, bytes and all, so
-- the database alone is a complete backup; `audio_path` is a copy on disk
-- that the native player and Sonos stream from, and is rebuilt from the
-- blob whenever it goes missing.
CREATE TABLE IF NOT EXISTS song_tracks (
    id             INTEGER PRIMARY KEY,
    song_id        INTEGER NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
    ord            INTEGER NOT NULL,
    remote_id      TEXT    NOT NULL,
    audio_path     TEXT    NOT NULL,
    image_path     TEXT,
    duration       REAL    NOT NULL DEFAULT 0,
    play_count     INTEGER NOT NULL DEFAULT 0,
    last_played_at INTEGER,
    UNIQUE (song_id, remote_id)
);

-- Playlists hold takes, not songs: the two takes of a song can sound quite
-- different, and the learner picks the one they like. Removing a take
-- removes it from every playlist.
CREATE TABLE IF NOT EXISTS playlists (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS playlist_tracks (
    id          INTEGER PRIMARY KEY,
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id    INTEGER NOT NULL REFERENCES song_tracks(id) ON DELETE CASCADE,
    ord         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS playlist_tracks_order ON playlist_tracks(playlist_id, ord);

-- What each AI request cost, for the cost center in Settings. `cost_usd` is
-- NULL when the provider does not report a price (speech). `ref` dedupes a
-- charge that could be seen twice, such as a Suno task polled again.
CREATE TABLE IF NOT EXISTS ai_costs (
    id         INTEGER PRIMARY KEY,
    day        INTEGER NOT NULL,
    feature    TEXT NOT NULL,
    provider   TEXT NOT NULL,
    model      TEXT NOT NULL DEFAULT '',
    cost_usd   REAL,
    detail     TEXT NOT NULL DEFAULT '',
    ref        TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ai_costs_day ON ai_costs(day, feature);
CREATE UNIQUE INDEX IF NOT EXISTS ai_costs_ref ON ai_costs(ref) WHERE ref IS NOT NULL;

-- Long-term memory the agents share: one fact about the learner per row.
CREATE TABLE IF NOT EXISTS memories (
    id         INTEGER PRIMARY KEY,
    content    TEXT NOT NULL,
    category   TEXT NOT NULL DEFAULT 'context',
    source     TEXT NOT NULL DEFAULT 'tutor',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
"#;

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 3 && table_exists(conn, "words")? {
        upgrade_to_english_only(conn)?;
    }
    conn.execute_batch(SCHEMA)?;
    // `CREATE TABLE IF NOT EXISTS` leaves existing tables alone, so columns
    // added after the first release need an explicit, idempotent ALTER.
    // Suno's word timings per take (JSON), for lyrics that follow the music.
    add_column(conn, "song_tracks", "aligned_words", "TEXT")?;
    add_column(conn, "song_tracks", "audio_bytes", "BLOB")?;
    add_column(conn, "song_tracks", "image_bytes", "BLOB")?;
    add_column(
        conn,
        "tutor_messages",
        "conversation_id",
        "INTEGER REFERENCES tutor_conversations(id) ON DELETE CASCADE",
    )?;
    adopt_loose_tutor_messages(conn)?;
    // Listening blocks belong to a song, and go when it goes.
    add_column(conn, "blocks", "song_id", "INTEGER REFERENCES songs(id) ON DELETE CASCADE")?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

/// Messages from before conversations existed become one conversation, so
/// nothing the learner talked about disappears.
fn adopt_loose_tutor_messages(conn: &Connection) -> Result<()> {
    let loose: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tutor_messages WHERE conversation_id IS NULL",
        [],
        |r| r.get(0),
    )?;
    if loose == 0 {
        return Ok(());
    }
    let (first, last): (i64, i64) = conn.query_row(
        "SELECT MIN(created_at), MAX(created_at) FROM tutor_messages WHERE conversation_id IS NULL",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    conn.execute(
        "INSERT INTO tutor_conversations (title, created_at, updated_at) VALUES ('Earlier conversation', ?1, ?2)",
        [first, last],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "UPDATE tutor_messages SET conversation_id = ?1 WHERE conversation_id IS NULL",
        [id],
    )?;
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Databases from before version 3 carried a Danish column on every item and
/// made each Polish headword globally unique. Both go: the app teaches Polish
/// from English only, and programmes may repeat a course word. The tables are
/// rebuilt with their row ids intact, so review history survives.
fn upgrade_to_english_only(conn: &Connection) -> Result<()> {
    // Very old installs predate the speaker gender.
    add_column(conn, "conversation_lines", "gender", "TEXT NOT NULL DEFAULT 'f'")?;

    // Foreign keys cannot be toggled inside a transaction, and must be off
    // while tables they point at are dropped and recreated.
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let result = conn.execute_batch(
        r#"
        BEGIN;
        CREATE TABLE topics_v3 (
            id         INTEGER PRIMARY KEY,
            slug       TEXT    NOT NULL UNIQUE,
            ord        INTEGER NOT NULL,
            kind       TEXT    NOT NULL DEFAULT 'course',
            title_pl   TEXT    NOT NULL,
            title_en   TEXT    NOT NULL,
            summary_en TEXT    NOT NULL,
            created_at INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO topics_v3 (id, slug, ord, kind, title_pl, title_en, summary_en)
            SELECT id, slug, ord, 'course', title_pl, title_en, summary_en FROM topics;
        DROP TABLE topics;
        ALTER TABLE topics_v3 RENAME TO topics;

        CREATE TABLE words_v3 (
            id       INTEGER PRIMARY KEY,
            topic_id INTEGER NOT NULL REFERENCES topics(id),
            ord      INTEGER NOT NULL,
            pl       TEXT    NOT NULL,
            en       TEXT    NOT NULL,
            pos      TEXT    NOT NULL,
            UNIQUE (topic_id, pl)
        );
        INSERT INTO words_v3 (id, topic_id, ord, pl, en, pos)
            SELECT id, topic_id, ord, pl, en, pos FROM words;
        DROP TABLE words;
        ALTER TABLE words_v3 RENAME TO words;

        CREATE TABLE sentences_v3 (
            id       INTEGER PRIMARY KEY,
            topic_id INTEGER NOT NULL REFERENCES topics(id),
            ord      INTEGER NOT NULL,
            pl       TEXT    NOT NULL,
            en       TEXT    NOT NULL,
            UNIQUE (topic_id, pl)
        );
        INSERT INTO sentences_v3 (id, topic_id, ord, pl, en)
            SELECT id, topic_id, ord, pl, en FROM sentences;
        DROP TABLE sentences;
        ALTER TABLE sentences_v3 RENAME TO sentences;

        CREATE TABLE conversation_lines_v3 (
            id       INTEGER PRIMARY KEY,
            topic_id INTEGER NOT NULL REFERENCES topics(id),
            ord      INTEGER NOT NULL,
            speaker  TEXT    NOT NULL,
            gender   TEXT    NOT NULL DEFAULT 'f',
            pl       TEXT    NOT NULL,
            en       TEXT    NOT NULL
        );
        INSERT INTO conversation_lines_v3 (id, topic_id, ord, speaker, gender, pl, en)
            SELECT id, topic_id, ord, speaker, gender, pl, en FROM conversation_lines;
        DROP TABLE conversation_lines;
        ALTER TABLE conversation_lines_v3 RENAME TO conversation_lines;

        -- The old per-topic chats were Polish role-play; the tutor is now an
        -- assistant for the whole app and starts a fresh conversation.
        DROP TABLE IF EXISTS chat_messages;
        DELETE FROM settings WHERE key = 'source_language';
        COMMIT;
        "#,
    );
    if result.is_err() {
        let _ = conn.execute_batch("ROLLBACK");
    }
    conn.pragma_update(None, "foreign_keys", "ON")?;
    result?;
    Ok(())
}

fn add_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    let exists = conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == column);
    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

/// The starter content (the ten topics and the lessons in `content/`) is
/// written once. After that it is ordinary data: courses and blocks the
/// learner and the tutor can change or delete, never seeded over again.
///
/// Installs from before this had the content as read-only built-ins: they are
/// converted to courses first, and the starter content is then written over
/// them by its natural keys (it could not have been changed), which keeps row
/// ids and so review history.
fn seed(conn: &Connection) -> Result<()> {
    if get_setting(conn, "starter_seeded")?.is_some() {
        return Ok(());
    }
    unify_courses(conn)?;
    seed_starter(conn)?;
    unify_courses(conn)?;
    conn.execute("DELETE FROM settings WHERE key = 'content_version'", [])?;
    set_setting(conn, "starter_seeded", "1")?;
    Ok(())
}

fn seed_starter(conn: &Connection) -> Result<()> {
    for (index, topic) in content::TOPICS.iter().enumerate() {
        let convo: Conversation = serde_json::from_str(topic.conversation)?;
        if convo.slug != topic.slug {
            return Err(Error::msg(format!(
                "conversation file for '{}' declares slug '{}'",
                topic.slug, convo.slug
            )));
        }
        conn.execute(
            "INSERT INTO topics (slug, ord, kind, title_pl, title_en, summary_en)
             VALUES (?1, ?2, 'course', ?3, ?4, ?5)
             ON CONFLICT(slug) DO UPDATE SET
                 ord = excluded.ord, kind = 'course', title_pl = excluded.title_pl,
                 title_en = excluded.title_en, summary_en = excluded.summary_en",
            rusqlite::params![topic.slug, index as i64 + 1, convo.title_pl, convo.title_en, convo.summary_en],
        )?;
        let topic_id: i64 =
            conn.query_row("SELECT id FROM topics WHERE slug = ?1", [topic.slug], |r| r.get(0))?;

        for (i, w) in content::parse_words(topic.words).iter().enumerate() {
            conn.execute(
                "INSERT INTO words (topic_id, ord, pl, en, pos) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(topic_id, pl) DO UPDATE SET
                     ord = excluded.ord, en = excluded.en, pos = excluded.pos",
                rusqlite::params![topic_id, i as i64, w.pl, w.en, w.pos],
            )?;
        }
        for (i, s) in content::parse_sentences(topic.sentences).iter().enumerate() {
            conn.execute(
                "INSERT INTO sentences (topic_id, ord, pl, en) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(topic_id, pl) DO UPDATE SET ord = excluded.ord, en = excluded.en",
                rusqlite::params![topic_id, i as i64, s.pl, s.en],
            )?;
        }
        // Conversation lines have no natural key, so they are replaced.
        conn.execute("DELETE FROM conversation_lines WHERE topic_id = ?1", [topic_id])?;
        for (i, line) in convo.lines.iter().enumerate() {
            let gender = convo.speakers.get(&line.speaker).ok_or_else(|| {
                Error::msg(format!(
                    "conversation '{}' has no gender for speaker '{}'",
                    topic.slug, line.speaker
                ))
            })?;
            conn.execute(
                "INSERT INTO conversation_lines (topic_id, ord, speaker, gender, pl, en)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![topic_id, i as i64, line.speaker, gender, line.pl, line.en],
            )?;
        }
        crate::blocks::split_topic(conn, topic_id, &crate::blocks::split_prefix(topic_id), "starter")?;
    }
    crate::blocks::seed_lessons(conn)?;
    Ok(())
}

/// One kind of folder: every topic is a course, every block is in one.
///
/// Built-in topics and the learner's programmes both become plain courses
/// (programmes after the starter ones, in the order they were made). Blocks
/// without a topic are filed: the starter lessons in "Grammar essentials",
/// listening blocks in "Listening", the tutor's lessons in "Lessons". Split
/// drills get a slug prefix of their own course id, so cutting a course again
/// later matches them. Safe to run twice.
pub fn unify_courses(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    // Starter courses keep their order; former programmes follow.
    let ids: Vec<i64> = {
        let mut stmt = tx.prepare(
            "SELECT id FROM topics ORDER BY kind = 'programme', ord, created_at, id",
        )?;
        let ids = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids
    };
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE topics SET ord = ?2, kind = 'course' WHERE id = ?1",
            rusqlite::params![id, i as i64 + 1],
        )?;
    }

    // Blocks without a course.
    let loose = |filter: &str| -> Result<bool> {
        Ok(tx.query_row(
            &format!("SELECT EXISTS (SELECT 1 FROM blocks WHERE topic_id IS NULL AND {filter})"),
            [],
            |r| r.get(0),
        )?)
    };
    if loose("slug LIKE 'lesson/%'")? {
        let id = crate::courses::ensure(
            &tx,
            "grammar-essentials",
            "Grammar essentials",
            "Podstawy gramatyki",
            "Być and mieć, verb patterns, gender, the cases, and when to say ty or Pan.",
        )?;
        // Dated like the other starter courses, so it sorts with them.
        tx.execute("UPDATE topics SET created_at = 0 WHERE id = ?1", [id])?;
        tx.execute(
            "UPDATE blocks SET topic_id = ?1 WHERE topic_id IS NULL AND slug LIKE 'lesson/%'",
            [id],
        )?;
    }
    if loose("song_id IS NOT NULL")? {
        let id = crate::courses::listening_course(&tx)?;
        tx.execute("UPDATE blocks SET topic_id = ?1 WHERE topic_id IS NULL AND song_id IS NOT NULL", [id])?;
    }
    if loose("1")? {
        let id = crate::courses::lessons_course(&tx)?;
        tx.execute("UPDATE blocks SET topic_id = ?1 WHERE topic_id IS NULL", [id])?;
    }

    // Split drills: "course/<slug>/…" and "programme/<id>/…" become "topic/<id>/…".
    tx.execute(
        "UPDATE blocks SET slug = 'topic/' || topic_id || substr(slug, instr(substr(slug, 8), '/') + 7)
         WHERE slug LIKE 'course/%/%'",
        [],
    )?;
    tx.execute(
        "UPDATE blocks SET slug = 'topic/' || topic_id || substr(slug, instr(substr(slug, 11), '/') + 10)
         WHERE slug LIKE 'programme/%/%'",
        [],
    )?;
    tx.execute("UPDATE blocks SET origin = 'starter' WHERE origin IN ('course', 'builtin')", [])?;
    tx.commit()?;
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

/// The id of the course topic at a position in the course (1-based).
#[cfg(test)]
pub fn require_topic_id(conn: &Connection, ord: i64) -> Result<i64> {
    conn.query_row(
        "SELECT id FROM topics WHERE ord = ?1",
        [ord],
        |r| r.get(0),
    )
        .optional()?
        .ok_or_else(|| Error::msg(format!("no topic with position {ord}")))
}
