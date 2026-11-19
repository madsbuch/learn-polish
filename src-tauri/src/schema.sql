-- The schema, version 8 (db.rs SCHEMA_VERSION): every id is a tagged
-- TypeID as text (DATA.md), made by the app (src-tauri/src/ids.rs). A new
-- database is made from this; an older one is converted to it first
-- (upgrade_v7.rs, then upgrade_v8.rs). Every statement is "IF NOT EXISTS":
-- it runs at every start.
--
-- Every table syncs between the learner's devices (docs/SYNC.md) except the
-- ones marked local below; the sync triggers on the others are made from
-- this schema in Rust (sync/triggers.rs), not written here.

CREATE TABLE IF NOT EXISTS courses (
    id         TEXT PRIMARY KEY NOT NULL,
    slug       TEXT    NOT NULL UNIQUE,
    ord        INTEGER NOT NULL,
    kind       TEXT    NOT NULL DEFAULT 'course',
    title_pl   TEXT    NOT NULL,
    title_en   TEXT    NOT NULL,
    summary_en TEXT    NOT NULL,
    created_at INTEGER NOT NULL DEFAULT 0,
    -- A picture of what the course is about (pictures.rs).
    image_file_id TEXT REFERENCES files(id) ON DELETE SET NULL,
    -- When it was first completed and first mastered (courses::milestones),
    -- so each is celebrated once.
    completed_at INTEGER,
    mastered_at  INTEGER,
    -- When the learner closed it as mastered: it is kept as a trophy and its
    -- lessons leave the check-ins (courses::close).
    closed_at    INTEGER
, level TEXT, level_checked_at INTEGER);

CREATE TABLE IF NOT EXISTS words (
    id       TEXT PRIMARY KEY NOT NULL,
    lesson_id TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL,
    pos      TEXT    NOT NULL,
    -- A picture of it (picture lessons, or added to a word drill).
    image_file_id TEXT REFERENCES files(id) ON DELETE SET NULL,
    UNIQUE (lesson_id, pl)
);

CREATE TABLE IF NOT EXISTS sentences (
    id       TEXT PRIMARY KEY NOT NULL,
    lesson_id TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL,
    -- Its words in another form than their dictionary one, from the tutor
    -- (a JSON list of {form, base, wrong}: tiles.rs). '[]' for sentences
    -- made without (schema 9): they are built by plain ordering.
    endings  TEXT    NOT NULL DEFAULT '[]',
    UNIQUE (lesson_id, pl)
);

CREATE TABLE IF NOT EXISTS conversation_lines (
    id       TEXT PRIMARY KEY NOT NULL,
    lesson_id TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    speaker  TEXT    NOT NULL,
    gender   TEXT    NOT NULL DEFAULT 'f',
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS reviews (
    kind      TEXT    NOT NULL,          -- 'word' | 'sentence'
    item_id   TEXT NOT NULL,
    due_day   INTEGER NOT NULL,
    interval  INTEGER NOT NULL,
    ease      REAL    NOT NULL,
    reps      INTEGER NOT NULL,
    lapses    INTEGER NOT NULL,
    last_day  INTEGER NOT NULL,
    PRIMARY KEY (kind, item_id)
);

CREATE TABLE IF NOT EXISTS xp_log (
    id        TEXT PRIMARY KEY NOT NULL,
    day       INTEGER NOT NULL,
    at        INTEGER NOT NULL,
    run       TEXT    NOT NULL DEFAULT '',
    lesson_id TEXT,
    label     TEXT    NOT NULL,
    xp        INTEGER NOT NULL,
    note      TEXT    NOT NULL DEFAULT ''
);

-- A day's numbers, one row per device that practised that day: two devices
-- adding to one row would each overwrite the other's totals when they sync.
-- Every read sums the devices' rows (the day_totals view).
CREATE TABLE IF NOT EXISTS day_log (
    day       INTEGER NOT NULL,
    -- The device that practised (sync_state.device_id).
    device    TEXT    NOT NULL,
    xp        INTEGER NOT NULL DEFAULT 0,
    answered  INTEGER NOT NULL DEFAULT 0,
    correct   INTEGER NOT NULL DEFAULT 0,
    course_id  TEXT,                    -- unused: the old daily session's course
    -- The day is done: the daily goal reached, or nothing left due.
    completed INTEGER NOT NULL DEFAULT 0,
    -- Missed, but a streak freeze kept the run going.
    frozen    INTEGER NOT NULL DEFAULT 0,
    -- Minutes of scheduled check-ins done (by the lessons' size), and how
    -- many of them were done before their day — the daily budget learns
    -- from the first, the "ahead" balance shows the second.
    minutes   INTEGER NOT NULL DEFAULT 0,
    early_minutes INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, device)
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

CREATE TABLE IF NOT EXISTS files (
    id         TEXT PRIMARY KEY NOT NULL,
    kind       TEXT    NOT NULL,          -- speech | song_audio | song_image
    name       TEXT    NOT NULL,          -- e.g. "song-3-0.mp3"
    mime       TEXT    NOT NULL,
    size       INTEGER NOT NULL,          -- bytes
    sha256     TEXT    NOT NULL,          -- hex
    bytes      BLOB    NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS speech_clips (
    cache_key  TEXT PRIMARY KEY,
    model      TEXT    NOT NULL,
    voice      TEXT    NOT NULL,
    text       TEXT    NOT NULL,
    file_id    TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS lessons (
    id            TEXT PRIMARY KEY NOT NULL,
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
    song_id       TEXT REFERENCES songs(id) ON DELETE CASCADE, -- listening lessons
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
, level TEXT, mastered_at INTEGER);

CREATE TABLE IF NOT EXISTS course_lessons (
    course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    lesson_id TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    PRIMARY KEY (course_id, lesson_id)
);

CREATE TABLE IF NOT EXISTS checkins (
    id          TEXT PRIMARY KEY NOT NULL,
    lesson_id    TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    started_at  INTEGER NOT NULL,
    finished_at INTEGER,
    result      TEXT,
    feedback    TEXT
);

CREATE TABLE IF NOT EXISTS checkin_messages (
    id         TEXT PRIMARY KEY NOT NULL,
    checkin_id TEXT NOT NULL REFERENCES checkins(id) ON DELETE CASCADE,
    role       TEXT    NOT NULL,
    content    TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS tutor_conversations (
    id         TEXT PRIMARY KEY NOT NULL,
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

CREATE TABLE IF NOT EXISTS tutor_messages (
    id         TEXT PRIMARY KEY NOT NULL,
    role       TEXT    NOT NULL,
    content    TEXT    NOT NULL,
    actions    TEXT,
    created_at INTEGER NOT NULL,
    conversation_id TEXT REFERENCES tutor_conversations(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS songs (
    id           TEXT PRIMARY KEY NOT NULL,
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
    course_id     TEXT REFERENCES courses(id) ON DELETE SET NULL,
    -- JSON: what was asked for and with what (brief, requested style,
    -- length, songwriter model, revisions).
    origin       TEXT    NOT NULL DEFAULT '{}'
, kind TEXT NOT NULL DEFAULT 'song', podcast TEXT NOT NULL DEFAULT '{}');

CREATE TABLE IF NOT EXISTS song_tracks (
    id             TEXT PRIMARY KEY NOT NULL,
    song_id        TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
    ord            INTEGER NOT NULL,
    remote_id      TEXT    NOT NULL,
    duration       REAL    NOT NULL DEFAULT 0,
    play_count     INTEGER NOT NULL DEFAULT 0,
    last_played_at INTEGER,
    aligned_words  TEXT,                -- Suno's word timings (JSON)
    audio_file_id  TEXT REFERENCES files(id),
    image_file_id  TEXT REFERENCES files(id) ON DELETE SET NULL, timed_lines TEXT,
    UNIQUE (song_id, remote_id)
);

CREATE TABLE IF NOT EXISTS playlists (
    id         TEXT PRIMARY KEY NOT NULL,
    name       TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS playlist_tracks (
    id          TEXT PRIMARY KEY NOT NULL,
    playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id    TEXT NOT NULL REFERENCES song_tracks(id) ON DELETE CASCADE,
    ord         INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_costs (
    id         TEXT PRIMARY KEY NOT NULL,
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

CREATE TABLE IF NOT EXISTS bench_runs (
    id         TEXT PRIMARY KEY NOT NULL,
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

CREATE TABLE IF NOT EXISTS wiki_sources (
    id            TEXT PRIMARY KEY NOT NULL,
    -- Whose source it is: a study lesson's or a podcast episode's.
    lesson_id     TEXT REFERENCES lessons(id) ON DELETE CASCADE,
    song_id       TEXT REFERENCES songs(id) ON DELETE CASCADE,
    ord           INTEGER NOT NULL DEFAULT 0,
    lang          TEXT    NOT NULL,     -- the article's Wikipedia: en, pl, …
    title         TEXT    NOT NULL,
    url           TEXT    NOT NULL,
    pageid        INTEGER NOT NULL,
    revid         INTEGER NOT NULL,
    summary       TEXT    NOT NULL,     -- the lead section
    content       TEXT    NOT NULL,     -- the whole article, as markdown
    image_file_id TEXT REFERENCES files(id) ON DELETE SET NULL,
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

CREATE TABLE IF NOT EXISTS memories (
    id         TEXT PRIMARY KEY NOT NULL,
    content    TEXT NOT NULL,
    category   TEXT NOT NULL DEFAULT 'context',
    source     TEXT NOT NULL DEFAULT 'tutor',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS word_examples (
    word_id    TEXT PRIMARY KEY NOT NULL REFERENCES words(id) ON DELETE CASCADE,
    examples   TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS words_lesson ON words(lesson_id, ord);

CREATE INDEX IF NOT EXISTS sentences_lesson ON sentences(lesson_id, ord);

CREATE INDEX IF NOT EXISTS conversation_lesson ON conversation_lines(lesson_id, ord);

CREATE INDEX IF NOT EXISTS reviews_due ON reviews(kind, due_day);

CREATE INDEX IF NOT EXISTS xp_log_day ON xp_log(day);

CREATE INDEX IF NOT EXISTS files_kind ON files(kind);

CREATE INDEX IF NOT EXISTS files_meta ON files(id, kind, name, mime, size);

CREATE INDEX IF NOT EXISTS lessons_due ON lessons(due_day);

CREATE INDEX IF NOT EXISTS course_lessons_lesson ON course_lessons(lesson_id);

CREATE INDEX IF NOT EXISTS playlist_tracks_order ON playlist_tracks(playlist_id, ord);

CREATE INDEX IF NOT EXISTS ai_costs_day ON ai_costs(day, feature);

CREATE UNIQUE INDEX IF NOT EXISTS ai_costs_ref ON ai_costs(ref) WHERE ref IS NOT NULL;

-- A day's numbers over every device that practised it (day_log).
CREATE VIEW IF NOT EXISTS day_totals AS
    SELECT day, sum(xp) AS xp, sum(answered) AS answered, sum(correct) AS correct,
           max(completed) AS completed, max(frozen) AS frozen,
           sum(minutes) AS minutes, sum(early_minutes) AS early_minutes
    FROM day_log GROUP BY day;

CREATE VIEW IF NOT EXISTS course_words AS
    SELECT c.course_id, w.id, w.lesson_id, c.ord * 1000 + w.ord AS ord, w.pl, w.en, w.pos
    FROM words w JOIN course_lessons c ON c.lesson_id = w.lesson_id;

CREATE VIEW IF NOT EXISTS course_sentences AS
    SELECT c.course_id, s.id, s.lesson_id, c.ord * 1000 + s.ord AS ord, s.pl, s.en
    FROM sentences s JOIN course_lessons c ON c.lesson_id = s.lesson_id;

CREATE VIEW IF NOT EXISTS course_lines AS
    SELECT c.course_id, l.id, l.lesson_id, c.ord * 1000 + l.ord AS ord, l.speaker, l.gender, l.pl, l.en
    FROM conversation_lines l JOIN course_lessons c ON c.lesson_id = l.lesson_id;

CREATE TRIGGER IF NOT EXISTS words_forget AFTER DELETE ON words BEGIN
    DELETE FROM reviews WHERE kind = 'word' AND item_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS sentences_forget AFTER DELETE ON sentences BEGIN
    DELETE FROM reviews WHERE kind = 'sentence' AND item_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS words_take_their_reviews AFTER DELETE ON words
BEGIN DELETE FROM reviews WHERE kind = 'word' AND item_id = OLD.id; END;

CREATE TRIGGER IF NOT EXISTS sentences_take_their_reviews AFTER DELETE ON sentences
BEGIN DELETE FROM reviews WHERE kind = 'sentence' AND item_id = OLD.id; END;


-- ---------------------------------------------------------------------------
-- Local: about this device only, never synced
-- ---------------------------------------------------------------------------

-- Settings about the device rather than the learner (theme, reminders, time
-- zone, playback): db.rs DEVICE_SETTINGS routes their keys here.
CREATE TABLE IF NOT EXISTS device_settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

-- Where this device wrote a file's bytes out, for the players that need a
-- real file (files.rs). Only a cache: written from the bytes when missing.
CREATE TABLE IF NOT EXISTS file_paths (
    file_id TEXT PRIMARY KEY NOT NULL,
    path    TEXT NOT NULL
);

-- ---------------------------------------------------------------------------
-- Sync (docs/SYNC.md, sync/engine.rs): local bookkeeping
-- ---------------------------------------------------------------------------

-- This device: its id, its hybrid logical clock (milliseconds and a counter
-- within one), the last local sequence number given out, and whether a
-- sync is writing right now (`applying`: the triggers then stand aside).
CREATE TABLE IF NOT EXISTS sync_state (
    id          INTEGER PRIMARY KEY NOT NULL,
    device_id   TEXT    NOT NULL,
    hlc_ms      INTEGER NOT NULL DEFAULT 0,
    hlc_counter INTEGER NOT NULL DEFAULT 0,
    seq         INTEGER NOT NULL DEFAULT 0,
    applying    INTEGER NOT NULL DEFAULT 0,
    vault_id    TEXT,
    secret_key  BLOB,
    public_key  BLOB
);

-- Every synced row's current version (tombstones kept): its clock value,
-- the device that wrote it, and the local sequence number it was recorded
-- at, which is what peers pull by. `pk` is the row's key as text, its
-- columns joined by U+001F when there are several.
CREATE TABLE IF NOT EXISTS sync_rows (
    tbl       TEXT    NOT NULL,
    pk        TEXT    NOT NULL,
    hlc       TEXT    NOT NULL,
    origin    TEXT    NOT NULL,
    deleted   INTEGER NOT NULL DEFAULT 0,
    seq       INTEGER NOT NULL,
    -- The peer this version came from, so it is not sent back to it.
    from_peer TEXT,
    PRIMARY KEY (tbl, pk)
);

CREATE INDEX IF NOT EXISTS sync_rows_seq ON sync_rows(seq);

-- Each row's version vector: per device, the clock value (as one integer)
-- of that device's latest change to it.
CREATE TABLE IF NOT EXISTS sync_vv (
    tbl     TEXT    NOT NULL,
    pk      TEXT    NOT NULL,
    device  TEXT    NOT NULL,
    counter INTEGER NOT NULL,
    PRIMARY KEY (tbl, pk, device)
);

-- What this device has pulled from each peer, and how the last sync went.
CREATE TABLE IF NOT EXISTS sync_peers (
    device_id        TEXT    PRIMARY KEY NOT NULL,
    last_pulled_seq  INTEGER NOT NULL DEFAULT 0,
    last_addr        TEXT,
    last_sync_at     TEXT,
    last_complete_at TEXT,
    complete         INTEGER NOT NULL DEFAULT 0,
    last_epoch       TEXT
);

-- Marks in this device's history that peers remember: a copy of this
-- database restored from before one never had it, and the first peer to
-- meet that copy tells it so (it then takes a new device id).
CREATE TABLE IF NOT EXISTS sync_epochs (
    epoch      TEXT    PRIMARY KEY NOT NULL,
    seq        INTEGER NOT NULL,
    created_at TEXT    NOT NULL
);

-- ---------------------------------------------------------------------------
-- Sync: the vault, synced like everything else
-- ---------------------------------------------------------------------------

-- The devices that hold the learner's data, each with its public key.
CREATE TABLE IF NOT EXISTS vault_devices (
    id         TEXT PRIMARY KEY NOT NULL,
    name       TEXT NOT NULL,
    public_key TEXT NOT NULL,
    platform   TEXT NOT NULL,
    added_at   TEXT NOT NULL
);

-- Keys of devices removed from the vault: never accepted again.
CREATE TABLE IF NOT EXISTS vault_revocations (
    public_key TEXT PRIMARY KEY NOT NULL,
    device_id  TEXT NOT NULL,
    revoked_at TEXT NOT NULL
);
