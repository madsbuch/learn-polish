# Data rules

How the database is kept, so that it can one day sync between devices (see
Tally's sync engine) and so that agents can't quietly use the wrong row. Every
change to the schema follows these.

## Ids

- **Every row's id is a tagged TypeID**: a short type prefix, an underscore,
  and a UUIDv7 in 26 characters of lowercase base32 — `ls_01jd6x7k2m8q9r4t5v6w7x8y9z`.
  Made on the device that writes the row (`src-tauri/src/ids.rs`), never an
  auto-incrementing integer. They sort like the UUIDv7 inside them: by when
  they were made.
- **The prefix says what it is.** `crs_` course, `ls_` lesson, `wd_` word,
  `sn_` sentence, `dl_` dialogue line, `sg_` song or podcast, `tk_` take,
  `pl_` playlist, `pe_` playlist entry, `fl_` file, `ci_` check-in, `cm_`
  check-in message, `cv_` tutor conversation, `msg_` tutor message, `mem_`
  memory, `src_` Wikipedia source, `xp_` XP entry, `cost_` AI cost, `bench_`
  benchmark run, `dev_` device (sync), `sp_` built-in sound pair
  (`sounds.rs`: derived ids, kept in the code, not a table), `spot_` where a
  thing is in a scene lesson's picture, `pnl_` a story lesson's panel, `att_` an answer in the learning log, `ev_` a
  learning event, `gr_` a grammar tag (`grammar.rs`: derived from its key, kept
  in the code, not a table), `tp_` a topic (`topics.rs`: derived from its key, kept in the
  code, not a table), `brg_` a radio bridge (`radio.rs`). A new table gets a new prefix, added
  to `ids.rs` and here.
- **Each kind has its own type**: `LessonId`, `CourseId`, … in Rust; branded
  strings (`LessonIdSchema`) in `src/lib/schemas.ts`. Only a correctly tagged id
  parses, so a course id where a lesson id belongs fails at once, with a
  message saying which it got — for the tutor too.
- **Built-in content gets derived ids** (`ids::derived`): the same namespace
  and key always give the same id, so two devices that install the demo course
  agree on its rows.
- **Tables keyed by something natural keep it**: settings by key, a day's log
  by day and device (each device adds up its own: docs/SYNC.md), a speech
  clip by its cache key, achievements by code, links (a course's lessons) by
  their pair of ids — a lesson is in exactly one course, so
  `course_lessons(lesson_id)` is unique too (schema 18; a lesson listed in
  several courses before kept the one it was first added to: db.rs,
  `one_course_per_lesson`) — a scene lesson's picture (`scenes`) and a story lesson's story (`stories`)
  by its lesson.
- **The learning log** (`attempts`, `learning_events`, schema 14) is
  append-only and per device: each row is written once, on the device that
  graded the answer or saw the event, in the same transaction as the grade.
  It keeps no foreign keys: an answer stays history after its word or lesson
  goes. Its `details` column is a small serde struct with a version (`v`),
  strict about its fields (`learnlog.rs`). Grammar tags link items to the
  built-in list: `word_grammar(word_id, tag_id)` and
  `sentence_grammar(sentence_id, tag_id)`, keyed by their pair.
- **Context in the log** (schema 15): each answer and event keeps the course
  it was practised from and how it was reached (`course_id`, `source`: one of
  `learnlog::SOURCES`), and each answer the card exactly (`card_key`,
  `direction`, `prompt`, `position`, `checkin_id`; every option in order,
  the tiles, the hint and a verdict in its `details`, version 2). The words
  an answer involved are `attempt_items(attempt_id, item_id, role,
  position)` — role target, in_sentence, in_line, in_story, in_song or
  distractor, with the form and its own result — written with the answer.
  A sentence word by word is `sentence_tokens(sentence_id, position)`, every
  value from the vocabulary in `grammar.rs`; topics link to the built-in
  ontology: `word_topics`, `sentence_topics`, `lesson_topics` (column
  `topic`, never `topic_id`: the name courses had before version 6).
- **The mastery ledger** (`mastery_log`, schema 16, ids `mst_…`): one row per
  graded go at a lesson and one for its mastered stamp — the change to its
  mastery points, the points before and after, the grade, a drill's score,
  days since the last go, extra practice or not, drill or check-in, the run,
  and a reason key (`masterylog::REASONS`) with versioned `details`. Written
  in the same transaction as the change (`lessons::credit_mastery`,
  `stamp_mastered`), by the device that made it, never updated: append-only
  and per device like `attempts`, with no foreign keys. Not backfilled: a lesson's points from before
  it are shown as one starting line.
- **No ids inside stored text** unless they are TypeIDs: JSON in a column
  (a tutor message's actions, a song's origin, the day's list) holds TypeIDs
  only, never positions or integers.
- **Ids are for code and agents, not people.** Screens show an id as the
  title of what it names (a chip that opens it), and the Diagnostics log adds
  the title after it. Nobody types an id.

## Changes to the schema

- **Every change bumps `SCHEMA_VERSION`** (`db.rs`), so the database is backed
  up before the upgrade (`backups/`, the newest two kept).
- **No one-time backfills of the learner's content** at startup
  (PRINCIPLES.md #4): a feature that needs existing content filled in offers a
  button that asks the tutor.
- **Writes that belong together are one transaction**: a lesson's grade and
  schedule, a check-in's verdict, every tutor tool call.
