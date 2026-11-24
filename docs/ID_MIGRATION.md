# The id migration (schema version 7)

Every auto-incrementing integer id becomes a tagged TypeID (`DATA.md`). This
is the plan, and the record of how it was done. Polski first; Dansk once the
learner has used it.

## What changes

| Table | Prefix | Its id's time (keeps the order) | Refers to |
|---|---|---|---|
| courses | `crs_` | created_at | image_file_id → files |
| lessons | `ls_` | created_at | song_id → songs |
| words | `wd_` | its lesson's | lesson_id, image_file_id |
| sentences | `sn_` | its lesson's | lesson_id |
| conversation_lines | `dl_` | its lesson's | lesson_id |
| songs | `sg_` | created_at | course_id |
| song_tracks | `tk_` | its song's | song_id, audio_file_id, image_file_id |
| playlists | `pl_` | created_at | — |
| playlist_tracks | `pe_` | its playlist's | playlist_id, track_id |
| files | `fl_` | created_at | — |
| checkins | `ci_` | started_at | lesson_id |
| checkin_messages | `cm_` | created_at | checkin_id |
| tutor_conversations | `cv_` | created_at | — |
| tutor_messages | `msg_` | created_at | conversation_id |
| memories | `mem_` | created_at | — |
| wiki_sources | `src_` | its lesson's or song's | lesson_id, song_id, image_file_id |
| xp_log | `xp_` | at | lesson_id (no foreign key) |
| ai_costs | `cost_` | created_at | — |
| bench_runs | `bench_` | created_at | — |

Kept as they are (natural keys): settings, day_log, achievements,
speech_clips (file_id → files changes), course_lessons (both ids change),
reviews (item_id changes: a word's or sentence's), word_examples (word_id
changes). The views (course_words, course_sentences, course_lines) and the
triggers that forget reviews are made again over the new tables.

## How a database is upgraded

1. **Backed up** first (`backups/`, as every upgrade).
2. **One transaction**, foreign keys off while tables are rebuilt:
   - For each table, a mapping `old integer → new TypeID`. The UUIDv7 inside
     takes the row's own time (above), ordered by the old id within the same
     millisecond, so the new ids sort the way the old ones did.
   - **Built-in content gets derived ids**: the demo course from its slug, its
     lessons from the course and their place, their words and sentences from
     the lesson and their Polish. Two devices that have the demo agree on it.
   - Each table is rebuilt with text ids: `CREATE` the new table, `INSERT …
     SELECT` through the mappings, drop the old, rename the new.
   - **Ids inside stored text are rewritten**: the day's list (`daily_ids`), a
     tutor message's actions (`courseId`, `lessonId`, `songId`), a song's
     origin (`based_on`), and in tutor messages, `*_id` keys of known kinds.
   - Views and triggers are made again; `PRAGMA foreign_key_check` must find
     nothing, or the upgrade is undone.
3. `user_version = 7`.

A new install creates the version 7 tables directly.

## In the code

- **Rust**: `src-tauri/src/ids.rs` — one type per prefix (`LessonId`, …):
  `new()`, `derived(namespace, key)`, parsing that names the kind expected,
  serde as a string, Diesel as `Text`. Every `i64` id becomes its type.
- **Screens**: ids are strings; `src/lib/schemas.ts` has a branded schema per
  kind (`LessonIdSchema`), so mixing kinds is a type error. Draft keys and
  other local storage keyed by an id follow.
- **Tutor tools**: every id parameter is a string with its pattern
  (`^ls_[0-9a-z]{26}$`) in the schema. A wrong kind fails with "lesson_id
  must be a lesson (ls_…), got a course (crs_…)"; an unknown id with "No
  lesson ls_… exists". Old conversations hold integer ids in their history;
  a call that copies one fails the same way, and the tutor looks again.

## Titles instead of ids

- **`ids_resolve`**: takes ids, returns each one's kind and title (the prefix
  says which table; one query each). Missing ids come back as "deleted".
- **Chips**: in the tutor's messages (and tool-call labels), any TypeID is
  shown as a chip with the title, which opens it. Cached per id.
- **Diagnostics log**: a logged message that holds TypeIDs gets each one's
  title after it — `ls_01jd… (Pierwsze spotkanie)` — so a pasted log is both
  exact and readable.

## Tests

- `ids.rs`: TypeIDs round-trip, sort by time, derived ids are stable, a wrong
  prefix is refused.
- An upgrade of a version 6 database with every table filled (the existing
  fixtures): every row and link survives under its new id, `foreign_key_check`
  is clean, stored JSON points at new ids, the demo course has derived ids.
- The shape check (`bun run check:shapes`) with the new string ids.
- Every existing test, on new ids.

## Afterwards

The sync engine (ported from Tally) keys its change records on these ids.
