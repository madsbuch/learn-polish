-- The database schema of version 5 (before blocks became lessons and topics
-- courses), for the upgrade test.

-- A topic is a course: a folder of blocks (see course_blocks). `kind` is
-- always 'course' now (it once told built-in topics from programmes).
CREATE TABLE IF NOT EXISTS topics (
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
    mastered_at  INTEGER
);

-- Words, sentences and dialogue lines belong to a block — the drill that
-- practises them — and go when it goes. A course reaches them through its
-- blocks. A headword is unique within a block only: two courses may well
-- teach the same word, each with its own history.
CREATE TABLE IF NOT EXISTS words (
    id       INTEGER PRIMARY KEY,
    block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL,
    pos      TEXT    NOT NULL,
    UNIQUE (block_id, pl)
);
CREATE INDEX IF NOT EXISTS words_block ON words(block_id, ord);

CREATE TABLE IF NOT EXISTS sentences (
    id       INTEGER PRIMARY KEY,
    block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL,
    UNIQUE (block_id, pl)
);
CREATE INDEX IF NOT EXISTS sentences_block ON sentences(block_id, ord);

CREATE TABLE IF NOT EXISTS conversation_lines (
    id       INTEGER PRIMARY KEY,
    block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    speaker  TEXT    NOT NULL,
    gender   TEXT    NOT NULL DEFAULT 'f',
    pl       TEXT    NOT NULL,
    en       TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS conversation_block ON conversation_lines(block_id, ord);

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
    topic_id  INTEGER,                    -- unused: the old daily session's topic
    -- The day is done: the daily goal reached, or nothing left due.
    completed INTEGER NOT NULL DEFAULT 0,
    -- Missed, but a streak freeze kept the run going.
    frozen    INTEGER NOT NULL DEFAULT 0,
    -- Minutes of scheduled check-ins done (by the blocks' size), and how
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

-- Blocks: the unit of learning (see blocks.rs). A drill practises its own
-- words, sentences or dialogue; a study block carries a lesson and is checked
-- by an examiner. Each has its own check-in schedule, and is listed in one
-- or more courses.
CREATE TABLE IF NOT EXISTS blocks (
    id            INTEGER PRIMARY KEY,
    slug          TEXT    UNIQUE,                -- starter lessons: lesson/<slug>
    kind          TEXT    NOT NULL,              -- drill | study
    format        TEXT    NOT NULL DEFAULT '',   -- drills: words | memory | typing | sentences | gaps | dialogue; 'song' for listening
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
    created_at    INTEGER NOT NULL,
    song_id       INTEGER REFERENCES songs(id) ON DELETE CASCADE, -- listening blocks
    -- What the last fact check against the block's sources changed, and when.
    fact_note       TEXT,
    fact_checked_at INTEGER,
    -- How fast its gaps grow: up when check-ins go easily, down when they
    -- don't (see plan.rs).
    ease          REAL NOT NULL DEFAULT 2.5,
    -- Writing blocks: how much help the learner gets — 0 guided (word bank,
    -- hints, simple questions), 1 supported (hints), 2 free. Moves with
    -- their results.
    support_level INTEGER NOT NULL DEFAULT 0,
    -- Writing blocks: the word bank, made once (JSON).
    scaffold      TEXT,
    -- Beyond the course's level: not needed to complete the course, only to
    -- master it.
    challenge     INTEGER NOT NULL DEFAULT 0,
    -- Passed at least once.
    passed        INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS blocks_due ON blocks(due_day);

-- Which courses list a block, in what order. A block may be in several; it is
-- deleted when it is taken out of the last one (courses.rs).
CREATE TABLE IF NOT EXISTS course_blocks (
    topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
    block_id INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
    ord      INTEGER NOT NULL,
    PRIMARY KEY (topic_id, block_id)
);
CREATE INDEX IF NOT EXISTS course_blocks_block ON course_blocks(block_id);

-- A course's words, sentences and dialogue: whatever its blocks own, in the
-- course's block order. A block in two courses shows in both.
CREATE VIEW IF NOT EXISTS course_words AS
    SELECT c.topic_id, w.id, w.block_id, c.ord * 1000 + w.ord AS ord, w.pl, w.en, w.pos
    FROM words w JOIN course_blocks c ON c.block_id = w.block_id;
CREATE VIEW IF NOT EXISTS course_sentences AS
    SELECT c.topic_id, s.id, s.block_id, c.ord * 1000 + s.ord AS ord, s.pl, s.en
    FROM sentences s JOIN course_blocks c ON c.block_id = s.block_id;
CREATE VIEW IF NOT EXISTS course_lines AS
    SELECT c.topic_id, l.id, l.block_id, c.ord * 1000 + l.ord AS ord, l.speaker, l.gender, l.pl, l.en
    FROM conversation_lines l JOIN course_blocks c ON c.block_id = l.block_id;

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
-- did in the app during that turn (courses created, blocks made, …), so
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
    -- The course whose words it uses, where its listening block goes.
    topic_id     INTEGER REFERENCES topics(id) ON DELETE SET NULL,
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

-- Wikipedia articles a study block is built on: its sources. The text is
-- kept (offline reading, fact checks), with the revision it was read at;
-- a Polish version alongside when there is one. They go with their block.
CREATE TABLE IF NOT EXISTS wiki_sources (
    id            INTEGER PRIMARY KEY,
    block_id      INTEGER NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
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
    fetched_at    INTEGER NOT NULL,
    checked_at    INTEGER NOT NULL,     -- last compared with Wikipedia
    UNIQUE (block_id, lang, pageid)
);

-- A word or sentence takes its practice history with it, however it goes
-- (with its block, its course, or edited out), so the counts on the
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
