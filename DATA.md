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
  benchmark run, `dev_` device (sync). A new table gets a new prefix, added
  to `ids.rs` and here.
- **Each kind has its own type**: `LessonId`, `CourseId`, … in Rust; branded
  strings (`LessonIdSchema`) in `src/lib/types.ts`. Only a correctly tagged id
  parses, so a course id where a lesson id belongs fails at once, with a
  message saying which it got — for the tutor too.
- **Built-in content gets derived ids** (`ids::derived`): the same namespace
  and key always give the same id, so two devices that install the demo course
  agree on its rows.
- **Tables keyed by something natural keep it**: settings by key, a day's log
  by day and device (each device adds up its own: docs/SYNC.md), a speech
  clip by its cache key, achievements by code, links (a course's lessons) by
  their pair of ids.
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
