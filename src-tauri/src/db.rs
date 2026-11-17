//! The database: one SQLite file, opened once, behind a mutex.
//!
//! Queries elsewhere go through Diesel against `crate::schema`, so they are
//! checked at compile time. The raw SQL here is the schema itself and the
//! one-off upgrades of older databases, whose tables no longer exist in the
//! schema; what those upgrades read back is still typed (`legacy` rows).

use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};

#[cfg(test)]
use crate::content::{self, Conversation};
use crate::error::{Error, Result};
use crate::schema::{lessons, course_lessons, settings, song_tracks, courses, tutor_conversations, tutor_messages};

/// The app's database connection.
pub type Conn = SqliteConnection;

/// Guards the single SQLite connection. Every command locks it briefly; there is
/// no long-running work behind the lock.
///
/// It is registered with the app before anything runs and filled in once the
/// database has opened: on Android the webview can call a command while
/// startup is still opening it, and such a call waits for it rather than
/// failing with "state not managed".
pub struct Db {
    conn: OnceLock<Mutex<Conn>>,
    ready: (Mutex<()>, Condvar),
    /// The same news for async commands, which must not lesson a thread.
    opened: tokio::sync::watch::Sender<bool>,
}

impl Default for Db {
    fn default() -> Self {
        Self { conn: OnceLock::new(), ready: Default::default(), opened: tokio::sync::watch::channel(false).0 }
    }
}

impl Db {
    /// A database that is open already (tests, tools).
    #[allow(dead_code)]
    pub fn new(conn: Conn) -> Self {
        let db = Self::default();
        db.fill(conn);
        db
    }

    /// Hands over the opened connection and wakes every command waiting for it.
    pub fn fill(&self, conn: Conn) {
        let _ = self.conn.set(Mutex::new(conn));
        let _guard = self.ready.0.lock().unwrap_or_else(|e| e.into_inner());
        self.ready.1.notify_all();
        self.opened.send_replace(true);
    }

    /// Waits, without holding a thread, until the database has opened. The
    /// first command at startup waits here; giving up after `limit` lets the
    /// app say so instead of hanging.
    pub async fn opened(&self, limit: std::time::Duration) -> Result<()> {
        if self.conn.get().is_some() {
            return Ok(());
        }
        let mut news = self.opened.subscribe();
        let opened = tokio::time::timeout(limit, news.wait_for(|open| *open)).await.map(|r| r.is_ok());
        if opened == Ok(true) {
            Ok(())
        } else {
            Err(Error::msg("Your data did not open in time. Try again."))
        }
    }

    fn conn(&self) -> &Mutex<Conn> {
        if let Some(conn) = self.conn.get() {
            return conn;
        }
        let mut guard = self.ready.0.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(conn) = self.conn.get() {
                return conn;
            }
            guard = self.ready.1.wait(guard).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, Conn> {
        // A poisoned lock means an earlier command panicked. The connection is
        // still usable, so recover rather than cascading the panic.
        self.conn().lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn open(path: &std::path::Path) -> Result<Conn> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = SqliteConnection::establish(&path.to_string_lossy())?;
    conn.batch_execute(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    )?;
    migrate(&mut conn)?;
    initialize_courses(&mut conn)?;
    // Files whose owner is gone (a crash between deleting a row and its
    // file, say) are not kept.
    crate::files::collect_orphans(&mut conn)?;
    // History left behind by items deleted before the triggers existed.
    conn.batch_execute(
        "DELETE FROM reviews WHERE kind = 'word' AND item_id NOT IN (SELECT id FROM words);
         DELETE FROM reviews WHERE kind = 'sentence' AND item_id NOT IN (SELECT id FROM sentences);",
    )?;
    match foreign_key_violations(&mut conn) {
        Ok(0) => {}
        Ok(n) => crate::diagnostics::note("database", format!("{n} rows point at rows that do not exist")),
        Err(error) => crate::diagnostics::note("database", format!("The integrity check failed: {error}")),
    }
    Ok(conn)
}

/// Rows read back by raw SQL (pragmas and old-schema upgrades).
mod legacy {
    use diesel::sql_types::{BigInt, Text};

    #[derive(diesel::QueryableByName)]
    pub struct Count {
        #[diesel(sql_type = BigInt)]
        pub n: i64,
    }

    #[derive(diesel::QueryableByName)]
    pub struct Id {
        #[diesel(sql_type = BigInt)]
        pub id: i64,
    }

    #[derive(diesel::QueryableByName)]
    pub struct Name {
        #[diesel(sql_type = Text)]
        pub name: String,
    }

    #[derive(diesel::QueryableByName)]
    pub struct UserVersion {
        #[diesel(sql_type = BigInt)]
        pub user_version: i64,
    }

    #[derive(diesel::QueryableByName)]
    pub struct Course {
        #[diesel(sql_type = BigInt)]
        pub id: i64,
        #[diesel(sql_type = Text)]
        pub title_pl: String,
    }
}

const SCHEMA_VERSION: i64 = 6;

const SCHEMA: &str = r#"
-- A course is a course: a folder of lessons (see course_lessons). `kind` is
-- always 'course' now (it once told built-in courses from programmes).
CREATE TABLE IF NOT EXISTS courses (
    id         INTEGER PRIMARY KEY,
    slug       TEXT    NOT NULL UNIQUE,
    ord        INTEGER NOT NULL,
    kind       TEXT    NOT NULL DEFAULT 'course',
    title_pl   TEXT    NOT NULL,
    title_en   TEXT    NOT NULL,
    summary_en TEXT    NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    -- A picture of what the course is about (pictures.rs).
    image_file_id INTEGER REFERENCES files(id) ON DELETE SET NULL,
    -- When it was first completed and first mastered (courses::milestones),
    -- so each is celebrated once.
    completed_at INTEGER,
    mastered_at  INTEGER,
    -- When the learner closed it as mastered: it is kept as a trophy and its
    -- lessons leave the check-ins (courses::close).
    closed_at    INTEGER
);

-- Words, sentences and dialogue lines belong to a lesson — the drill that
-- practises them — and go when it goes. A course reaches them through its
-- lessons. A headword is unique within a lesson only: two courses may well
-- teach the same word, each with its own history.
CREATE TABLE IF NOT EXISTS words (
    id       INTEGER PRIMARY KEY,
    lesson_id INTEGER NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL,
    pos      TEXT    NOT NULL,
    -- A picture of it (picture lessons, or added to a word drill).
    image_file_id INTEGER REFERENCES files(id) ON DELETE SET NULL,
    UNIQUE (lesson_id, pl)
);
CREATE INDEX IF NOT EXISTS words_lesson ON words(lesson_id, ord);

CREATE TABLE IF NOT EXISTS sentences (
    id       INTEGER PRIMARY KEY,
    lesson_id INTEGER NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL,
    UNIQUE (lesson_id, pl)
);
CREATE INDEX IF NOT EXISTS sentences_lesson ON sentences(lesson_id, ord);

CREATE TABLE IF NOT EXISTS conversation_lines (
    id       INTEGER PRIMARY KEY,
    lesson_id INTEGER NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    speaker  TEXT    NOT NULL,
    gender   TEXT    NOT NULL DEFAULT 'f',
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS conversation_lesson ON conversation_lines(lesson_id, ord);

-- A word or sentence that goes takes its review history with it.
CREATE TRIGGER IF NOT EXISTS words_forget AFTER DELETE ON words BEGIN
    DELETE FROM reviews WHERE kind = 'word' AND item_id = OLD.id;
END;
CREATE TRIGGER IF NOT EXISTS sentences_forget AFTER DELETE ON sentences BEGIN
    DELETE FROM reviews WHERE kind = 'sentence' AND item_id = OLD.id;
END;

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
    course_id  INTEGER,                    -- unused: the old daily session's course
    -- The day is done: the daily goal reached, or nothing left due.
    completed INTEGER NOT NULL DEFAULT 0,
    -- Missed, but a streak freeze kept the run going.
    frozen    INTEGER NOT NULL DEFAULT 0,
    -- Minutes of scheduled check-ins done (by the lessons' size), and how
    -- many of them were done before their day — the daily budget learns
    -- from the first, the "ahead" balance shows the second.
    minutes   INTEGER NOT NULL DEFAULT 0,
    early_minutes INTEGER NOT NULL DEFAULT 0
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

-- Every file the app keeps — speech clips, song audio, cover art — bytes and
-- all, so the database alone is a complete backup. Other tables reference a
-- file by id. `path` is a copy on disk for the players that need a real file;
-- it is only a cache, written from `bytes` when missing (see files.rs).
CREATE TABLE IF NOT EXISTS files (
    id         INTEGER PRIMARY KEY,
    kind       TEXT    NOT NULL,          -- speech | song_audio | song_image
    name       TEXT    NOT NULL,          -- e.g. "song-3-0.mp3"
    mime       TEXT    NOT NULL,
    size       INTEGER NOT NULL,          -- bytes
    sha256     TEXT    NOT NULL,          -- hex
    bytes      BLOB    NOT NULL,
    path       TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS files_kind ON files(kind);

-- Generated speech, so a line is only ever paid for once and replays work
-- offline. Keyed by the exact text plus the model and voice that produced it,
-- so changing voice does not serve stale audio. The audio is a file.
CREATE TABLE IF NOT EXISTS speech_clips (
    cache_key  TEXT PRIMARY KEY,
    model      TEXT    NOT NULL,
    voice      TEXT    NOT NULL,
    text       TEXT    NOT NULL,
    file_id    INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);

-- Lessons: the unit of learning (see lessons.rs). A drill practises its own
-- words, sentences or dialogue; a study lesson carries a lesson and is checked
-- by an examiner. Each has its own check-in schedule, and is listed in one
-- or more courses.
CREATE TABLE IF NOT EXISTS lessons (
    id            INTEGER PRIMARY KEY,
    slug          TEXT    UNIQUE,                -- starter lessons: lesson/<slug>
    -- What it is and how it is practised: one of lesson_types::REGISTRY
    -- (words | pictures | typing | memory | sentences | gaps | dialogue |
    -- study | listening | writing).
    lesson_type   TEXT    NOT NULL DEFAULT '',
    ord           INTEGER NOT NULL DEFAULT 0,
    title         TEXT    NOT NULL,
    summary       TEXT    NOT NULL DEFAULT '',
    complexity    TEXT    NOT NULL,              -- low | medium | high
    content       TEXT    NOT NULL DEFAULT '',
    objectives    TEXT    NOT NULL DEFAULT '[]',
    period_days   INTEGER NOT NULL,
    interval_days INTEGER NOT NULL DEFAULT 0,
    due_day       INTEGER,                       -- NULL until first done
    last_day      INTEGER,
    last_result   TEXT,                          -- pass | partial | fail
    reps          INTEGER NOT NULL DEFAULT 0,
    origin        TEXT    NOT NULL DEFAULT 'tutor', -- starter | tutor | you
    created_at    INTEGER NOT NULL,
    song_id       INTEGER REFERENCES songs(id) ON DELETE CASCADE, -- listening lessons
    -- What the last fact check against the lesson's sources changed, and when.
    fact_note       TEXT,
    fact_checked_at INTEGER,
    -- How fast its gaps grow: up when check-ins go easily, down when they
    -- don't (see plan.rs).
    ease          REAL NOT NULL DEFAULT 2.5,
    -- Writing lessons: how much help the learner gets — 0 guided (word bank,
    -- hints, simple questions), 1 supported (hints), 2 free. Moves with
    -- their results.
    support_level INTEGER NOT NULL DEFAULT 0,
    -- Writing lessons: the word bank, made once (JSON).
    scaffold      TEXT,
    -- Beyond the course's level: not needed to complete the course, only to
    -- master it.
    challenge     INTEGER NOT NULL DEFAULT 0,
    -- Passed at least once.
    passed        INTEGER NOT NULL DEFAULT 0,
    -- Mastery points, 0–100, earned by passing it (see lessons::mastery_gain).
    mastery       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS lessons_due ON lessons(due_day);

-- Which courses list a lesson, in what order. A lesson may be in several; it is
-- deleted when it is taken out of the last one (courses.rs).
CREATE TABLE IF NOT EXISTS course_lessons (
    course_id INTEGER NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    lesson_id INTEGER NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    PRIMARY KEY (course_id, lesson_id)
);
CREATE INDEX IF NOT EXISTS course_lessons_lesson ON course_lessons(lesson_id);

-- A course's words, sentences and dialogue: whatever its lessons own, in the
-- course's lesson order. A lesson in two courses shows in both.
CREATE VIEW IF NOT EXISTS course_words AS
    SELECT c.course_id, w.id, w.lesson_id, c.ord * 1000 + w.ord AS ord, w.pl, w.en, w.pos
    FROM words w JOIN course_lessons c ON c.lesson_id = w.lesson_id;
CREATE VIEW IF NOT EXISTS course_sentences AS
    SELECT c.course_id, s.id, s.lesson_id, c.ord * 1000 + s.ord AS ord, s.pl, s.en
    FROM sentences s JOIN course_lessons c ON c.lesson_id = s.lesson_id;
CREATE VIEW IF NOT EXISTS course_lines AS
    SELECT c.course_id, l.id, l.lesson_id, c.ord * 1000 + l.ord AS ord, l.speaker, l.gender, l.pl, l.en
    FROM conversation_lines l JOIN course_lessons c ON c.lesson_id = l.lesson_id;

-- A study lesson's check-in: an interview with the examiner, and its verdict.
CREATE TABLE IF NOT EXISTS checkins (
    id          INTEGER PRIMARY KEY,
    lesson_id    INTEGER NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
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
    updated_at INTEGER NOT NULL,
    -- The tutor answers in the background: set while it works on a reply.
    job_started_at INTEGER,
    -- Why the last reply failed, until the next one is asked for.
    job_error  TEXT,
    -- What kind of conversation: NULL for an ordinary one, 'word' for a
    -- chat about one word or sentence ("Talk about it").
    tag        TEXT,
    -- What it is about, for a tagged one: "gratulacje — congratulations".
    about      TEXT
);

-- The messages of a conversation with the tutor. `actions` is a JSON list of what the tutor
-- did in the app during that turn (courses created, lessons made, …), so
-- the UI can offer buttons and later turns know what already happened.
CREATE TABLE IF NOT EXISTS tutor_messages (
    id         INTEGER PRIMARY KEY,
    role       TEXT    NOT NULL,
    content    TEXT    NOT NULL,
    actions    TEXT,
    created_at INTEGER NOT NULL,
    conversation_id INTEGER REFERENCES tutor_conversations(id) ON DELETE CASCADE
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
    created_at   INTEGER NOT NULL,
    -- The course whose words it uses, where its listening lesson goes.
    course_id     INTEGER REFERENCES courses(id) ON DELETE SET NULL,
    -- JSON: what was asked for and with what (brief, requested style,
    -- length, songwriter model, revisions).
    origin       TEXT    NOT NULL DEFAULT '{}'
);

-- Suno sings every song twice; each take is a row, its audio and cover art
-- files. A take without its audio file cannot be played and is not kept.
CREATE TABLE IF NOT EXISTS song_tracks (
    id             INTEGER PRIMARY KEY,
    song_id        INTEGER NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
    ord            INTEGER NOT NULL,
    remote_id      TEXT    NOT NULL,
    duration       REAL    NOT NULL DEFAULT 0,
    play_count     INTEGER NOT NULL DEFAULT 0,
    last_played_at INTEGER,
    aligned_words  TEXT,                -- Suno's word timings (JSON)
    audio_file_id  INTEGER REFERENCES files(id),
    image_file_id  INTEGER REFERENCES files(id) ON DELETE SET NULL,
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
    created_at INTEGER NOT NULL,
    -- Tokens read (of them, from the provider's cache) and written, when
    -- reported: what the model picker estimates a month with another model from.
    tokens_in     INTEGER NOT NULL DEFAULT 0,
    tokens_cached INTEGER NOT NULL DEFAULT 0,
    tokens_out    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS ai_costs_day ON ai_costs(day, feature);

-- Benchmark runs (bench.rs): a model's scores on the app's own jobs.
CREATE TABLE IF NOT EXISTS bench_runs (
    id         INTEGER PRIMARY KEY,
    model      TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    score      INTEGER NOT NULL,
    polish     INTEGER NOT NULL,
    tools      INTEGER NOT NULL,
    rules      INTEGER NOT NULL,
    seconds    REAL    NOT NULL,
    cost_usd   REAL,
    items      TEXT    NOT NULL   -- every item's outcome (JSON)
);
CREATE UNIQUE INDEX IF NOT EXISTS ai_costs_ref ON ai_costs(ref) WHERE ref IS NOT NULL;

-- Wikipedia articles a study lesson is built on: its sources. The text is
-- kept (offline reading, fact checks), with the revision it was read at;
-- a Polish version alongside when there is one. They go with their lesson.
CREATE TABLE IF NOT EXISTS wiki_sources (
    id            INTEGER PRIMARY KEY,
    -- Whose source it is: a study lesson's or a podcast episode's.
    lesson_id     INTEGER REFERENCES lessons(id) ON DELETE CASCADE,
    song_id       INTEGER REFERENCES songs(id) ON DELETE CASCADE,
    ord           INTEGER NOT NULL DEFAULT 0,
    lang          TEXT    NOT NULL,     -- the article's Wikipedia: en, pl, …
    title         TEXT    NOT NULL,
    url           TEXT    NOT NULL,
    pageid        INTEGER NOT NULL,
    revid         INTEGER NOT NULL,
    summary       TEXT    NOT NULL,     -- the lead section
    content       TEXT    NOT NULL,     -- the whole article, as markdown
    image_file_id INTEGER REFERENCES files(id) ON DELETE SET NULL,
    pl_title      TEXT,                 -- the same article on Polish Wikipedia
    pl_url        TEXT,
    pl_content    TEXT,
    html          TEXT,                 -- the article as Wikipedia lays it out, for the reader
    pl_html       TEXT,
    fetched_at    INTEGER NOT NULL,
    checked_at    INTEGER NOT NULL,     -- last compared with Wikipedia
    UNIQUE (lesson_id, lang, pageid),
    UNIQUE (song_id, lang, pageid)
);

-- A word or sentence takes its practice history with it, however it goes
-- (with its lesson, its course, or edited out), so the counts on the
-- progress screen only ever count what exists — and an id SQLite reuses
-- never inherits someone else's history.
CREATE TRIGGER IF NOT EXISTS words_take_their_reviews AFTER DELETE ON words
BEGIN DELETE FROM reviews WHERE kind = 'word' AND item_id = OLD.id; END;
CREATE TRIGGER IF NOT EXISTS sentences_take_their_reviews AFTER DELETE ON sentences
BEGIN DELETE FROM reviews WHERE kind = 'sentence' AND item_id = OLD.id; END;

-- Long-term memory the agents share: one fact about the learner per row.
CREATE TABLE IF NOT EXISTS memories (
    id         INTEGER PRIMARY KEY,
    content    TEXT NOT NULL,
    category   TEXT NOT NULL DEFAULT 'context',
    source     TEXT NOT NULL DEFAULT 'tutor',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Example sentences for a word, shown when it is answered wrong: made once
-- by the AI (a JSON list of {pl, en}) and kept.
CREATE TABLE IF NOT EXISTS word_examples (
    word_id    INTEGER PRIMARY KEY REFERENCES words(id) ON DELETE CASCADE,
    examples   TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);
"#;

fn migrate(conn: &mut Conn) -> Result<()> {
    let version = diesel::sql_query("PRAGMA user_version").get_result::<legacy::UserVersion>(conn)?.user_version;
    if version < 3 && table_exists(conn, "words")? {
        upgrade_to_english_only(conn)?;
    }
    if version < 4 && table_exists(conn, "words")? {
        upgrade_to_block_owned_items(conn)?;
    }
    if version < 5 && table_exists(conn, "song_tracks")? {
        upgrade_to_files(conn)?;
    }
    if version < 6 && table_exists(conn, "blocks")? {
        upgrade_to_lessons_and_courses(conn)?;
    }
    conn.batch_execute(SCHEMA)?;
    flatten_lesson_types(conn)?;
    // `CREATE TABLE IF NOT EXISTS` leaves existing tables alone, so columns
    // added after the first release need an explicit, idempotent ALTER.
    add_column(conn, "song_tracks", "aligned_words", "TEXT")?;
    add_column(
        conn,
        "tutor_messages",
        "conversation_id",
        "INTEGER REFERENCES tutor_conversations(id) ON DELETE CASCADE",
    )?;
    adopt_loose_tutor_messages(conn)?;
    add_column(conn, "songs", "course_id", "INTEGER REFERENCES courses(id) ON DELETE SET NULL")?;
    add_column(conn, "songs", "origin", "TEXT NOT NULL DEFAULT '{}'")?;
    add_column(conn, "day_log", "frozen", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "lessons", "fact_note", "TEXT")?;
    add_column(conn, "lessons", "fact_checked_at", "INTEGER")?;
    add_column(conn, "lessons", "ease", "REAL NOT NULL DEFAULT 2.5")?;
    add_column(conn, "lessons", "support_level", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "courses", "completed_at", "INTEGER")?;
    add_column(conn, "courses", "mastered_at", "INTEGER")?;
    add_column(conn, "lessons", "scaffold", "TEXT")?;
    add_column(conn, "lessons", "challenge", "INTEGER NOT NULL DEFAULT 0")?;
    if add_column(conn, "lessons", "passed", "INTEGER NOT NULL DEFAULT 0")? {
        // Before this was kept: a lesson last passed, or done more than once,
        // has been passed.
        conn.batch_execute("UPDATE lessons SET passed = 1 WHERE last_result = 'pass' OR reps > 1")?;
    }
    if add_column(conn, "lessons", "mastery", "INTEGER NOT NULL DEFAULT 0")? {
        // Mastery used to mean coming back a month apart. A lesson that already
        // did keeps its mastery; any other passed lesson keeps credit for its
        // passes (30 for the first, 25 for each one after), short of mastered.
        conn.batch_execute(
            "UPDATE lessons SET mastery = CASE
                 WHEN passed = 1 AND interval_days >= 30 THEN 100
                 WHEN passed = 1 THEN min(90, 30 + 25 * max(reps - 1, 0))
                 ELSE 0 END",
        )?;
    }
    add_column(conn, "words", "image_file_id", "INTEGER REFERENCES files(id) ON DELETE SET NULL")?;
    add_column(conn, "day_log", "minutes", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "ai_costs", "tokens_in", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "ai_costs", "tokens_cached", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "ai_costs", "tokens_out", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "day_log", "early_minutes", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "tutor_conversations", "job_started_at", "INTEGER")?;
    add_column(conn, "tutor_conversations", "job_error", "TEXT")?;
    add_column(conn, "tutor_conversations", "tag", "TEXT")?;
    add_column(conn, "tutor_conversations", "about", "TEXT")?;
    add_column(conn, "courses", "image_file_id", "INTEGER REFERENCES files(id) ON DELETE SET NULL")?;
    add_column(conn, "courses", "closed_at", "INTEGER")?;
    add_column(conn, "songs", "kind", "TEXT NOT NULL DEFAULT 'song'")?;
    add_column(conn, "songs", "podcast", "TEXT NOT NULL DEFAULT '{}'")?;
    add_column(conn, "song_tracks", "timed_lines", "TEXT")?;
    if !column_exists(conn, "wiki_sources", "song_id")? {
        upgrade_sources_for_podcasts(conn)?;
    }
    conn.batch_execute(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
    Ok(())
}

/// Version 6: blocks are lessons and topics are courses, in the database too.
/// Tables and columns are renamed in place, so every row keeps its id and
/// every reference follows; the views and indexes are made again under the
/// new names by the schema. The catch-all "Lessons" course becomes "Grammar &
/// culture", so "lessons" means one thing.
fn upgrade_to_lessons_and_courses(conn: &mut Conn) -> Result<()> {
    conn.transaction::<_, Error, _>(|conn| {
        conn.batch_execute(
            "DROP VIEW IF EXISTS course_words;
             DROP VIEW IF EXISTS course_sentences;
             DROP VIEW IF EXISTS course_lines;
             DROP INDEX IF EXISTS words_block;
             DROP INDEX IF EXISTS sentences_block;
             DROP INDEX IF EXISTS conversation_block;
             DROP INDEX IF EXISTS blocks_due;
             DROP INDEX IF EXISTS course_blocks_block;",
        )?;
        for (old, new) in [("topics", "courses"), ("blocks", "lessons"), ("course_blocks", "course_lessons")] {
            if table_exists(conn, old)? && !table_exists(conn, new)? {
                conn.batch_execute(&format!("ALTER TABLE {old} RENAME TO {new}"))?;
            }
        }
        for (table, old, new) in [
            ("course_lessons", "topic_id", "course_id"),
            ("course_lessons", "block_id", "lesson_id"),
            ("words", "block_id", "lesson_id"),
            ("sentences", "block_id", "lesson_id"),
            ("conversation_lines", "block_id", "lesson_id"),
            ("checkins", "block_id", "lesson_id"),
            ("wiki_sources", "block_id", "lesson_id"),
            ("songs", "topic_id", "course_id"),
            ("day_log", "topic_id", "course_id"),
            ("lessons", "lesson", "content"),
        ] {
            if table_exists(conn, table)? && column_exists(conn, table, old)? && !column_exists(conn, table, new)? {
                conn.batch_execute(&format!("ALTER TABLE {table} RENAME COLUMN {old} TO {new}"))?;
            }
        }
        if table_exists(conn, "courses")? {
            diesel::sql_query("UPDATE courses SET title_en = ?, title_pl = ? WHERE slug = 'lessons' AND title_en = 'Lessons'")
                .bind::<Text, _>(crate::courses::GRAMMAR_CULTURE.0)
                .bind::<Text, _>(crate::courses::GRAMMAR_CULTURE.1)
                .execute(conn)?;
        }
        Ok(())
    })
}

/// Version 5: every file is a row in `files`. Song takes move their bytes (or
/// the file on disk, for takes from before bytes were stored) into it and
/// reference it by id; the old speech cache is dropped — clips are made
/// again, with the current voices, as lines are played.
/// Sources used to belong only to study lessons; podcast episodes have them
/// too now, and every source keeps the article's layout for the reader. SQLite
/// cannot make a column nullable in place, so the table is rebuilt, ids intact.
fn upgrade_sources_for_podcasts(conn: &mut Conn) -> Result<()> {
    conn.batch_execute("PRAGMA foreign_keys = OFF")?;
    let result = conn.batch_execute(
        r#"
        BEGIN;
        CREATE TABLE wiki_sources_v7 (
            id            INTEGER PRIMARY KEY,
            lesson_id     INTEGER REFERENCES lessons(id) ON DELETE CASCADE,
            song_id       INTEGER REFERENCES songs(id) ON DELETE CASCADE,
            ord           INTEGER NOT NULL DEFAULT 0,
            lang          TEXT    NOT NULL,
            title         TEXT    NOT NULL,
            url           TEXT    NOT NULL,
            pageid        INTEGER NOT NULL,
            revid         INTEGER NOT NULL,
            summary       TEXT    NOT NULL,
            content       TEXT    NOT NULL,
            image_file_id INTEGER REFERENCES files(id) ON DELETE SET NULL,
            pl_title      TEXT,
            pl_url        TEXT,
            pl_content    TEXT,
            html          TEXT,
            pl_html       TEXT,
            fetched_at    INTEGER NOT NULL,
            checked_at    INTEGER NOT NULL,
            UNIQUE (lesson_id, lang, pageid),
            UNIQUE (song_id, lang, pageid)
        );
        INSERT INTO wiki_sources_v7 (id, lesson_id, ord, lang, title, url, pageid, revid, summary, content,
                                     image_file_id, pl_title, pl_url, pl_content, fetched_at, checked_at)
            SELECT id, lesson_id, ord, lang, title, url, pageid, revid, summary, content,
                   image_file_id, pl_title, pl_url, pl_content, fetched_at, checked_at FROM wiki_sources;
        DROP TABLE wiki_sources;
        ALTER TABLE wiki_sources_v7 RENAME TO wiki_sources;
        COMMIT;
        "#,
    );
    if result.is_err() {
        let _ = conn.batch_execute("ROLLBACK");
    }
    conn.batch_execute("PRAGMA foreign_keys = ON")?;
    Ok(result?)
}

fn upgrade_to_files(conn: &mut Conn) -> Result<()> {
    #[derive(diesel::QueryableByName)]
    struct OldTake {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        id: i64,
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        song_id: i64,
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        ord: i64,
        #[diesel(sql_type = diesel::sql_types::Text)]
        audio_path: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        image_path: Option<String>,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Binary>)]
        audio_bytes: Option<Vec<u8>>,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Binary>)]
        image_bytes: Option<Vec<u8>>,
    }
    if !column_exists(conn, "song_tracks", "audio_path")? {
        return Ok(());
    }
    // Very old databases lack the blob columns; the files on disk are read then.
    add_column(conn, "song_tracks", "audio_bytes", "BLOB")?;
    add_column(conn, "song_tracks", "image_bytes", "BLOB")?;
    conn.batch_execute(
        "CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL, mime TEXT NOT NULL,
            size INTEGER NOT NULL, sha256 TEXT NOT NULL, bytes BLOB NOT NULL, path TEXT,
            created_at INTEGER NOT NULL
        );",
    )?;
    add_column(conn, "song_tracks", "audio_file_id", "INTEGER REFERENCES files(id)")?;
    add_column(conn, "song_tracks", "image_file_id", "INTEGER REFERENCES files(id) ON DELETE SET NULL")?;

    conn.transaction::<_, Error, _>(|tx| {
        let takes: Vec<OldTake> = diesel::sql_query(
            "SELECT id, song_id, ord, audio_path, image_path, audio_bytes, image_bytes FROM song_tracks",
        )
        .load(tx)?;
        for take in takes {
            let audio = take.audio_bytes.or_else(|| std::fs::read(&take.audio_path).ok()).filter(|b| !b.is_empty());
            let Some(audio) = audio else {
                // Nothing left to play: the take goes (and leaves its playlists).
                diesel::delete(song_tracks::table.find(take.id)).execute(tx)?;
                continue;
            };
            let audio_id = moved_file(tx, crate::files::SONG_AUDIO, &take.audio_path, take.song_id, take.ord, &audio, "mp3")?;
            let image = take
                .image_bytes
                .or_else(|| take.image_path.as_ref().and_then(|p| std::fs::read(p).ok()))
                .filter(|b| !b.is_empty());
            let image_id = match (image, &take.image_path) {
                (Some(bytes), Some(path)) => {
                    Some(moved_file(tx, crate::files::SONG_IMAGE, path, take.song_id, take.ord, &bytes, "jpg")?)
                }
                _ => None,
            };
            diesel::update(song_tracks::table.find(take.id))
                .set((song_tracks::audio_file_id.eq(audio_id), song_tracks::image_file_id.eq(image_id)))
                .execute(tx)?;
        }
        Ok(())
    })?;
    conn.batch_execute(
        "ALTER TABLE song_tracks DROP COLUMN audio_path;
         ALTER TABLE song_tracks DROP COLUMN image_path;
         ALTER TABLE song_tracks DROP COLUMN audio_bytes;
         ALTER TABLE song_tracks DROP COLUMN image_bytes;
         DROP TABLE IF EXISTS audio_cache;",
    )?;
    // The old speech cache and the duplicated song bytes leave free pages.
    conn.batch_execute("VACUUM")?;
    Ok(())
}

/// A song file from before the files table, as a file row. The copy on disk
/// is kept where it is.
fn moved_file(
    conn: &mut Conn,
    kind: &str,
    old_path: &str,
    song_id: i64,
    ord: i64,
    bytes: &[u8],
    fallback_ext: &str,
) -> Result<i64> {
    let ext = std::path::Path::new(old_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or(fallback_ext)
        .to_lowercase();
    let mime = match ext.as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "png" => "image/png",
        "webp" => "image/webp",
        "jpg" | "jpeg" => "image/jpeg",
        _ if kind == crate::files::SONG_AUDIO => "audio/mpeg",
        _ => "image/jpeg",
    };
    let id = crate::files::store(conn, kind, &format!("song-{song_id}-{ord}.{ext}"), mime, bytes)?;
    if std::path::Path::new(old_path).exists() {
        diesel::update(crate::schema::files::table.find(id))
            .set(crate::schema::files::path.eq(old_path))
            .execute(conn)?;
    }
    Ok(id)
}

fn column_exists(conn: &mut Conn, table: &str, column: &str) -> Result<bool> {
    Ok(diesel::sql_query(format!("SELECT name FROM pragma_table_info('{table}')"))
        .load::<legacy::Name>(conn)?
        .iter()
        .any(|row| row.name == column))
}

/// Messages from before conversations existed become one conversation, so
/// nothing the learner talked about disappears.
fn adopt_loose_tutor_messages(conn: &mut Conn) -> Result<()> {
    let loose = tutor_messages::table.filter(tutor_messages::conversation_id.is_null());
    let times: Vec<i64> = loose.select(tutor_messages::created_at).load(conn)?;
    let (Some(first), Some(last)) = (times.iter().min(), times.iter().max()) else {
        return Ok(());
    };
    let id: i64 = diesel::insert_into(tutor_conversations::table)
        .values((
            tutor_conversations::title.eq("Earlier conversation"),
            tutor_conversations::created_at.eq(first),
            tutor_conversations::updated_at.eq(last),
        ))
        .returning(tutor_conversations::id)
        .get_result(conn)?;
    diesel::update(loose).set(tutor_messages::conversation_id.eq(id)).execute(conn)?;
    Ok(())
}

/// Lessons had a kind (drill | study) and, under it, a format — two levels,
/// where writing and listening were "study" with a format. Now one type says
/// it all: a drill's format is its type; study, writing and listening are
/// their own.
fn flatten_lesson_types(conn: &mut Conn) -> Result<()> {
    if !add_column(conn, "lessons", "lesson_type", "TEXT NOT NULL DEFAULT ''")? {
        return Ok(());
    }
    conn.batch_execute(
        "UPDATE lessons SET lesson_type = CASE
             WHEN kind = 'drill' THEN format
             WHEN format = 'writing' THEN 'writing'
             WHEN format = 'song' THEN 'listening'
             ELSE 'study' END;
         ALTER TABLE lessons DROP COLUMN kind;
         ALTER TABLE lessons DROP COLUMN format;",
    )?;
    Ok(())
}

fn table_exists(conn: &mut Conn, table: &str) -> Result<bool> {
    let found = diesel::sql_query("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?")
        .bind::<Text, _>(table)
        .load::<legacy::Name>(conn)?;
    Ok(!found.is_empty())
}

/// Databases from before version 3 carried a Danish column on every item and
/// made each Polish headword globally unique. Both go: the app teaches Polish
/// from English only, and programmes may repeat a course word. The tables are
/// rebuilt with their row ids intact, so review history survives.
fn upgrade_to_english_only(conn: &mut Conn) -> Result<()> {
    // Very old installs predate the speaker gender.
    add_column(conn, "conversation_lines", "gender", "TEXT NOT NULL DEFAULT 'f'")?;

    // Foreign keys cannot be toggled inside a transaction, and must be off
    // while tables they point at are dropped and recreated.
    conn.batch_execute("PRAGMA foreign_keys = OFF")?;
    let result = conn.batch_execute(
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
        let _ = conn.batch_execute("ROLLBACK");
    }
    conn.batch_execute("PRAGMA foreign_keys = ON")?;
    result?;
    Ok(())
}

/// Before version 4, words, sentences and dialogue lines belonged to a topic
/// (a course), drills pointed at some of them (block_items), and each block
/// (a lesson) sat in one topic. Now each item belongs to one lesson and lessons are listed in
/// courses. Row ids are kept, so review history survives.
///
/// A word two drills shared (a tutor-made drill over words that the course's
/// own word drills also picked up) goes to the tutor's drill; an automatic
/// drill left with nothing goes. Items no drill held — or every item, in a
/// database from before lessons — are cut into new drills of their course.
fn upgrade_to_block_owned_items(conn: &mut Conn) -> Result<()> {
    conn.batch_execute("PRAGMA foreign_keys = OFF")?;
    let result = conn.transaction::<_, Error, _>(|tx| {
        let had_lessons = table_exists(tx, "blocks")?;
        if had_lessons {
            add_column(tx, "blocks", "song_id", "INTEGER")?;
        }
        tx.batch_execute(
            r#"
            CREATE TABLE blocks_v4 (
                id            INTEGER PRIMARY KEY,
                slug          TEXT    UNIQUE,
                kind          TEXT    NOT NULL,
                format        TEXT    NOT NULL DEFAULT '',
                ord           INTEGER NOT NULL DEFAULT 0,
                title         TEXT    NOT NULL,
                summary       TEXT    NOT NULL DEFAULT '',
                complexity    TEXT    NOT NULL,
                lesson        TEXT    NOT NULL DEFAULT '',
                objectives    TEXT    NOT NULL DEFAULT '[]',
                period_days   INTEGER NOT NULL,
                interval_days INTEGER NOT NULL DEFAULT 0,
                due_day       INTEGER,
                last_day      INTEGER,
                last_result   TEXT,
                reps          INTEGER NOT NULL DEFAULT 0,
                origin        TEXT    NOT NULL DEFAULT 'tutor',
                created_at    INTEGER NOT NULL,
                song_id       INTEGER REFERENCES songs(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS course_blocks (
                topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
                block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
                ord      INTEGER NOT NULL,
                PRIMARY KEY (topic_id, block_id)
            );
            CREATE TABLE words_v4 (
                id INTEGER PRIMARY KEY, block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
                ord INTEGER NOT NULL, pl TEXT NOT NULL, en TEXT NOT NULL, pos TEXT NOT NULL, UNIQUE (block_id, pl)
            );
            CREATE TABLE sentences_v4 (
                id INTEGER PRIMARY KEY, block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
                ord INTEGER NOT NULL, pl TEXT NOT NULL, en TEXT NOT NULL, UNIQUE (block_id, pl)
            );
            CREATE TABLE conversation_lines_v4 (
                id INTEGER PRIMARY KEY, block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
                ord INTEGER NOT NULL, speaker TEXT NOT NULL, gender TEXT NOT NULL DEFAULT 'f',
                pl TEXT NOT NULL, en TEXT NOT NULL
            );
            "#,
        )?;
        if had_lessons {
            tx.batch_execute(
                r#"
                INSERT INTO blocks_v4 (id, slug, kind, format, ord, title, summary, complexity, lesson,
                        objectives, period_days, interval_days, due_day, last_day, last_result, reps,
                        origin, created_at, song_id)
                    SELECT id, slug, kind, format, ord, title, summary, complexity, lesson,
                        objectives, period_days, interval_days, due_day, last_day, last_result, reps,
                        origin, created_at, song_id FROM blocks;
                INSERT OR IGNORE INTO course_blocks (topic_id, block_id, ord)
                    SELECT topic_id, id, ord FROM blocks WHERE topic_id IS NOT NULL;

                -- Each item goes to one drill that held it: a made one first.
                INSERT OR IGNORE INTO words_v4 (id, block_id, ord, pl, en, pos)
                    SELECT w.id, o.block_id, o.ord, w.pl, w.en, w.pos FROM words w
                    JOIN (SELECT i.item_id, i.block_id, i.ord,
                                 ROW_NUMBER() OVER (PARTITION BY i.item_id
                                     ORDER BY b.slug IS NOT NULL, b.id) AS pick
                          FROM block_items i JOIN blocks b ON b.id = i.block_id
                          WHERE i.kind = 'word') o ON o.item_id = w.id AND o.pick = 1;
                INSERT OR IGNORE INTO sentences_v4 (id, block_id, ord, pl, en)
                    SELECT s.id, o.block_id, o.ord, s.pl, s.en FROM sentences s
                    JOIN (SELECT i.item_id, i.block_id, i.ord,
                                 ROW_NUMBER() OVER (PARTITION BY i.item_id
                                     ORDER BY b.slug IS NOT NULL, b.id) AS pick
                          FROM block_items i JOIN blocks b ON b.id = i.block_id
                          WHERE i.kind = 'sentence') o ON o.item_id = s.id AND o.pick = 1;
                -- A topic's dialogue goes to its dialogue drill.
                INSERT INTO conversation_lines_v4 (id, block_id, ord, speaker, gender, pl, en)
                    SELECT l.id, d.block_id, l.ord, l.speaker, l.gender, l.pl, l.en FROM conversation_lines l
                    JOIN (SELECT topic_id, MIN(id) AS block_id FROM blocks
                          WHERE format = 'dialogue' AND topic_id IS NOT NULL GROUP BY topic_id) d
                      ON d.topic_id = l.topic_id;

                -- Automatic drills left empty by the above go.
                DELETE FROM blocks_v4 WHERE slug IS NOT NULL AND kind = 'drill'
                    AND NOT EXISTS (SELECT 1 FROM words_v4 w WHERE w.block_id = blocks_v4.id)
                    AND NOT EXISTS (SELECT 1 FROM sentences_v4 s WHERE s.block_id = blocks_v4.id)
                    AND NOT EXISTS (SELECT 1 FROM conversation_lines_v4 l WHERE l.block_id = blocks_v4.id);
                DELETE FROM course_blocks WHERE block_id NOT IN (SELECT id FROM blocks_v4);
                DELETE FROM checkin_messages WHERE checkin_id IN
                    (SELECT id FROM checkins WHERE block_id NOT IN (SELECT id FROM blocks_v4));
                DELETE FROM checkins WHERE block_id NOT IN (SELECT id FROM blocks_v4);
                -- Drill slugs were only for re-cutting topics, which no longer happens.
                UPDATE blocks_v4 SET slug = NULL WHERE slug NOT LIKE 'lesson/%';
                "#,
            )?;
        }

        // Whatever no drill held is cut into new drills of its course.
        let courses = diesel::sql_query("SELECT id, title_pl FROM topics ORDER BY ord").load::<legacy::Course>(tx)?;
        let now = now_secs();
        let new_lesson = |tx: &mut Conn, course: i64, title: &str, format: &str, complexity: &str| -> Result<i64> {
            let id = diesel::sql_query(
                "INSERT INTO blocks_v4 (kind, format, title, complexity, period_days, origin, created_at)
                 VALUES ('drill', ?, ?, ?, ?, 'starter', ?) RETURNING id",
            )
            .bind::<Text, _>(format)
            .bind::<Text, _>(title)
            .bind::<Text, _>(complexity)
            .bind::<BigInt, _>(crate::lesson_types::get("words").map_or(2, |t| t.default_period(complexity)))
            .bind::<BigInt, _>(now)
            .get_result::<legacy::Id>(tx)?
            .id;
            diesel::sql_query(
                "INSERT INTO course_blocks (topic_id, block_id, ord)
                 VALUES (?1, ?2, (SELECT COALESCE(MAX(ord), 0) + 1 FROM course_blocks WHERE topic_id = ?1))",
            )
            .bind::<BigInt, _>(course)
            .bind::<BigInt, _>(id)
            .execute(tx)?;
            Ok(id)
        };
        for course in courses {
            let lines = diesel::sql_query(
                "SELECT COUNT(*) AS n FROM conversation_lines WHERE topic_id = ?
                   AND id NOT IN (SELECT id FROM conversation_lines_v4)",
            )
            .bind::<BigInt, _>(course.id)
            .get_result::<legacy::Count>(tx)?
            .n;
            if lines > 0 {
                let lesson = new_lesson(tx, course.id, &format!("Conversation: {}", course.title_pl), "dialogue", "low")?;
                diesel::sql_query(
                    "INSERT INTO conversation_lines_v4 (id, block_id, ord, speaker, gender, pl, en)
                     SELECT id, ?2, ord, speaker, gender, pl, en FROM conversation_lines
                     WHERE topic_id = ?1 AND id NOT IN (SELECT id FROM conversation_lines_v4)",
                )
                .bind::<BigInt, _>(course.id)
                .bind::<BigInt, _>(lesson)
                .execute(tx)?;
            }
            for (table, title, format, complexity, size, columns) in [
                ("words", "Words", "words", "low", crate::lessons::WORDS_PER_LESSON, "pl, en, pos"),
                ("sentences", "Sentences", "sentences", "medium", crate::lessons::SENTENCES_PER_LESSON, "pl, en"),
            ] {
                let loose: Vec<i64> = diesel::sql_query(format!(
                    "SELECT id FROM {table} WHERE topic_id = ?
                       AND id NOT IN (SELECT id FROM {table}_v4) ORDER BY ord, id"
                ))
                .bind::<BigInt, _>(course.id)
                .load::<legacy::Id>(tx)?
                .into_iter()
                .map(|row| row.id)
                .collect();
                let taken = diesel::sql_query(format!(
                    "SELECT COUNT(*) AS n FROM blocks_v4 b JOIN course_blocks c ON c.block_id = b.id
                     WHERE c.topic_id = ? AND b.format = '{format}'"
                ))
                .bind::<BigInt, _>(course.id)
                .get_result::<legacy::Count>(tx)?
                .n;
                for (n, chunk) in loose.chunks(size).enumerate() {
                    let lesson = new_lesson(tx, course.id, &format!("{title} {}", taken + n as i64 + 1), format, complexity)?;
                    for (ord, id) in chunk.iter().enumerate() {
                        diesel::sql_query(format!(
                            "INSERT OR IGNORE INTO {table}_v4 (id, block_id, ord, {columns})
                             SELECT id, ?2, ?3, {columns} FROM {table} WHERE id = ?1"
                        ))
                        .bind::<BigInt, _>(id)
                        .bind::<BigInt, _>(lesson)
                        .bind::<BigInt, _>(ord as i64)
                        .execute(tx)?;
                    }
                }
            }
        }

        tx.batch_execute(
            r#"
            DROP TABLE IF EXISTS block_items;
            DROP TABLE IF EXISTS blocks;
            DROP TABLE words;
            DROP TABLE sentences;
            DROP TABLE conversation_lines;
            ALTER TABLE blocks_v4 RENAME TO blocks;
            ALTER TABLE words_v4 RENAME TO words;
            ALTER TABLE sentences_v4 RENAME TO sentences;
            ALTER TABLE conversation_lines_v4 RENAME TO conversation_lines;
            -- Review rows whose item did not survive.
            DELETE FROM reviews WHERE kind = 'word' AND item_id NOT IN (SELECT id FROM words);
            DELETE FROM reviews WHERE kind = 'sentence' AND item_id NOT IN (SELECT id FROM sentences);
            "#,
        )?;
        Ok(())
    });
    conn.batch_execute("PRAGMA foreign_keys = ON")?;
    result
}

/// Adds a column if it is missing; true when it was just added.
fn add_column(conn: &mut Conn, table: &str, column: &str, definition: &str) -> Result<bool> {
    let exists = diesel::sql_query(format!("SELECT name FROM pragma_table_info('{table}')"))
        .load::<legacy::Name>(conn)?
        .iter()
        .any(|row| row.name == column);
    if !exists {
        conn.batch_execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))?;
    }
    Ok(!exists)
}

/// Rows whose foreign keys point at nothing — none, in a healthy database.
pub fn foreign_key_violations(conn: &mut Conn) -> Result<i64> {
    Ok(diesel::sql_query("SELECT count(*) AS n FROM pragma_foreign_key_check")
        .get_result::<legacy::Count>(conn)?
        .n)
}

fn initialize_courses(conn: &mut Conn) -> Result<()> {
    if get_setting(conn, "starter_seeded")?.is_none() {
        // Retain migrations for old user-owned data, without adding curriculum.
        unify_courses(conn)?;
        diesel::delete(settings::table.filter(settings::key.eq_any(["content_version", "lesson_index", "focus_topic_id"])))
            .execute(conn)?;
        set_setting(conn, "starter_seeded", "1")?;
    }
    crate::starter_retirement::run(conn)?;
    crate::demo_course::seed_once(conn)
}

/// Historical release data used only by regression tests, never on app startup.
#[cfg(test)]
pub(crate) fn seed_test_courses(conn: &mut Conn) -> Result<()> {
    // Historical regression fixtures model the release before the small demo.
    let demo = courses::table.filter(courses::slug.eq(crate::demo_course::SLUG)).select(courses::id).first::<i64>(conn).optional()?;
    if let Some(demo) = demo {
        crate::courses::delete(conn, demo)?;
    }
    for (index, course) in content::COURSES.iter().enumerate() {
        let convo: Conversation = serde_json::from_str(course.conversation)?;
        if convo.slug != course.slug {
            return Err(Error::msg(format!(
                "conversation file for '{}' declares slug '{}'",
                course.slug, convo.slug
            )));
        }
        let course_id = crate::courses::ensure(conn, course.slug, &convo.title_en, &convo.title_pl, &convo.summary_en)?;
        diesel::update(courses::table.find(course_id))
            .set((courses::ord.eq(index as i64 + 1), courses::created_at.eq(0)))
            .execute(conn)?;
        let words: Vec<crate::courses::NewWord> = content::parse_words(course.words)
            .into_iter()
            .map(|w| crate::courses::NewWord { pl: w.pl.to_string(), en: w.en.to_string(), pos: w.pos.to_string() })
            .collect();
        let sentences: Vec<crate::courses::NewSentence> = content::parse_sentences(course.sentences)
            .into_iter()
            .map(|s| crate::courses::NewSentence { pl: s.pl.to_string(), en: s.en.to_string() })
            .collect();
        let mut lines = Vec::new();
        for line in &convo.lines {
            let gender = convo.speakers.get(&line.speaker).ok_or_else(|| {
                Error::msg(format!(
                    "conversation '{}' has no gender for speaker '{}'",
                    course.slug, line.speaker
                ))
            })?;
            lines.push(crate::courses::NewLine {
                speaker: line.speaker.clone(),
                gender: gender.clone(),
                pl: line.pl.clone(),
                en: line.en.clone(),
            });
        }
        // A course that already has a conversation (an older install) keeps it.
        let has_dialogue = course_lessons::table
            .inner_join(lessons::table)
            .filter(course_lessons::course_id.eq(course_id))
            .filter(lessons::lesson_type.eq("dialogue"))
            .count()
            .get_result::<i64>(conn)?
            > 0;
        crate::courses::cut_into_drills(
            conn,
            course_id,
            &words,
            &sentences,
            if has_dialogue { &[] } else { &lines },
            "starter",
        )?;
    }
    crate::lessons::seed_lessons(conn)?;
    Ok(())
}

/// One kind of folder: every course is a course, every lesson is listed in one.
///
/// Built-in courses and the learner's programmes both become plain courses
/// (programmes after the starter ones, in the order they were made). Lessons
/// no course lists are filed: the starter lessons in "Grammar essentials",
/// listening lessons in "Listening", the rest in "Grammar & culture". Safe to run twice.
pub fn unify_courses(conn: &mut Conn) -> Result<()> {
    conn.transaction::<_, Error, _>(|conn| {
        // Starter courses keep their order; former programmes follow.
        let mut all: Vec<(i64, String, i64, i64)> = courses::table
            .select((courses::id, courses::kind, courses::ord, courses::created_at))
            .load(conn)?;
        all.sort_by_key(|(id, kind, ord, created)| (kind == "programme", *ord, *created, *id));
        for (i, (id, ..)) in all.iter().enumerate() {
            diesel::update(courses::table.find(id))
                .set((courses::ord.eq(i as i64 + 1), courses::kind.eq("course")))
                .execute(conn)?;
        }

        // Lessons no course lists: lessons, listening lessons, anything else.
        let listed: std::collections::HashSet<i64> =
            course_lessons::table.select(course_lessons::lesson_id).load::<i64>(conn)?.into_iter().collect();
        let mut loose: Vec<(i64, Option<String>, Option<i64>, i64)> = lessons::table
            .select((lessons::id, lessons::slug, lessons::song_id, lessons::ord))
            .load::<(i64, Option<String>, Option<i64>, i64)>(conn)?
            .into_iter()
            .filter(|(id, ..)| !listed.contains(id))
            .collect();
        loose.sort_by_key(|(id, _, _, ord)| (*ord, *id));
        let starter = |slug: &Option<String>| slug.as_deref().is_some_and(|s| s.starts_with("lesson/"));
        let groups: [(Vec<i64>, fn(&mut Conn) -> Result<i64>); 3] = [
            (loose.iter().filter(|(_, slug, ..)| starter(slug)).map(|b| b.0).collect(), crate::courses::grammar_course),
            (
                loose.iter().filter(|(_, slug, song, _)| !starter(slug) && song.is_some()).map(|b| b.0).collect(),
                crate::courses::listening_course,
            ),
            (
                loose.iter().filter(|(_, slug, song, _)| !starter(slug) && song.is_none()).map(|b| b.0).collect(),
                crate::courses::grammar_culture_course,
            ),
        ];
        for (ids, course) in groups {
            if !ids.is_empty() {
                let id = course(conn)?;
                for lesson in ids {
                    crate::courses::add_lesson(conn, id, lesson)?;
                }
            }
        }
        diesel::update(lessons::table.filter(lessons::origin.eq_any(["course", "builtin"])))
            .set(lessons::origin.eq("starter"))
            .execute(conn)?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

pub fn get_setting(conn: &mut Conn, key: &str) -> Result<Option<String>> {
    Ok(settings::table.find(key).select(settings::value).first(conn).optional()?)
}

pub fn set_setting(conn: &mut Conn, key: &str, value: &str) -> Result<()> {
    diesel::insert_into(settings::table)
        .values((settings::key.eq(key), settings::value.eq(value)))
        .on_conflict(settings::key)
        .do_update()
        .set(settings::value.eq(value))
        .execute(conn)?;
    Ok(())
}

pub fn delete_setting(conn: &mut Conn, key: &str) -> Result<()> {
    diesel::delete(settings::table.find(key)).execute(conn)?;
    Ok(())
}

pub fn get_setting_i64(conn: &mut Conn, key: &str, fallback: i64) -> i64 {
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
pub fn today(conn: &mut Conn) -> i64 {
    let offset_minutes = get_setting_i64(conn, "tz_offset_minutes", 0);
    (now_secs() + offset_minutes * 60).div_euclid(86_400)
}

/// The id of the course at a position in the course list (1-based).
#[cfg(test)]
pub fn require_course_id(conn: &mut Conn, ord: i64) -> Result<i64> {
    courses::table
        .filter(courses::ord.eq(ord))
        .select(courses::id)
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg(format!("no course with position {ord}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two levels (kind, format) become one type: a drill's format is its
    /// type; study lessons become study, writing or listening.
    #[test]
    fn lesson_kinds_and_formats_flatten_into_types() {
        use diesel::Connection;
        let mut conn = Conn::establish(":memory:").unwrap();
        conn.batch_execute(
            "CREATE TABLE lessons (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, format TEXT NOT NULL DEFAULT '', song_id INTEGER);
             INSERT INTO lessons VALUES (1, 'drill', 'words', NULL), (2, 'drill', 'dialogue', NULL),
                 (3, 'study', '', NULL), (4, 'study', 'writing', NULL), (5, 'study', 'song', 7);",
        )
        .unwrap();
        flatten_lesson_types(&mut conn).unwrap();
        let types: Vec<String> = crate::schema::lessons::table
            .select(crate::schema::lessons::lesson_type)
            .order(crate::schema::lessons::id)
            .load(&mut conn)
            .unwrap();
        assert_eq!(types, ["words", "dialogue", "study", "writing", "listening"]);
        assert!(types.iter().all(|t| crate::lesson_types::get(t).is_ok()));
        // Run again (every start): nothing changes.
        flatten_lesson_types(&mut conn).unwrap();
    }

    /// `crate::schema` is what queries are checked against at compile time;
    /// this checks it against the database the app actually creates. Every
    /// table and view's full column list must be selectable as declared.
    #[test]
    fn an_async_command_waits_for_the_database_without_a_thread() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let db = std::sync::Arc::new(Db::default());
        runtime.block_on(async {
            let late = db.clone();
            let filler = tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                late.fill(SqliteConnection::establish(":memory:").unwrap());
            });
            // One thread only: this works only if waiting does not block it.
            db.opened(std::time::Duration::from_secs(5)).await.unwrap();
            filler.await.unwrap();
            assert!(Db::default().opened(std::time::Duration::from_millis(20)).await.is_err());
        });
    }

    #[test]
    fn a_command_before_the_database_opens_waits_for_it() {
        let db = std::sync::Arc::new(Db::default());
        let waiting = {
            let db = db.clone();
            std::thread::spawn(move || db.lock().batch_execute("SELECT 1").is_ok())
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        db.fill(SqliteConnection::establish(":memory:").unwrap());
        assert!(waiting.join().unwrap());
    }

    #[test]
    fn schema_matches_the_database() {
        let path = std::env::temp_dir()
            .join("learn-polish-tests")
            .join(format!("schema-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = &mut open(&path).unwrap();
        macro_rules! check {
            ($($table:ident),* $(,)?) => {$({
                use crate::schema::$table;
                let query = $table::table.select($table::all_columns);
                let sql = diesel::debug_query::<diesel::sqlite::Sqlite, _>(&query).to_string();
                let sql = sql.split(" -- binds").next().unwrap_or_default();
                conn.batch_execute(sql)
                    .unwrap_or_else(|e| panic!("schema.rs and the database disagree on {}: {e}", stringify!($table)));
            })*};
        }
        check!(
            courses, words, sentences, conversation_lines, reviews, day_log, settings, achievements,
            files, speech_clips, wiki_sources, lessons, course_lessons, course_words, course_sentences, course_lines, checkins,
            checkin_messages, tutor_conversations, tutor_messages, songs, song_tracks, playlists,
            playlist_tracks, ai_costs, memories, word_examples,
        );
    }
}
