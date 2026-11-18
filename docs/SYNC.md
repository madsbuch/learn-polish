# Sync between devices

Polski keeps one learner's data on several devices (a phone and a computer)
in step, directly between them on the same network. No server. The design is
Tally's sync engine (`madsbuch/tally`, `src-tauri/src/sync/`), ported from
sqlx to Diesel and fitted to this schema. Where this file and Tally differ,
this file says why.

## What the learner sees

- **Settings → Devices.** A device shows a QR code ("Add a device"); the other
  scans it. From then on the two are one *vault*: they sync whenever both are
  open on the same Wi-Fi, a few seconds after a change, and the phone also
  every 30 minutes in the background (Android WorkManager, unmetered network
  only). A **Sync now** button, and a list of the vault's devices with when
  each last synced; a device can be removed (its key is revoked).
- **Joining** wipes the joining device's synced data and takes the vault's.
  If it holds data of its own, it says so (how many courses, lessons, words)
  and asks first; a backup is taken (`backups/`, as before an upgrade).
- Everything syncs: courses, lessons, words, reviews, XP, songs and their
  files, the tutor's conversations, memories, settings that are about the
  learner (models, API keys, the daily five). Settings that are about the
  device (theme, reminders, time zone, playback) stay where they are.

## The model (as Tally)

- **Whole rows, not operations.** Triggers on every synced table record each
  insert, update and delete in `sync_rows` (the row's current version: a
  hybrid-logical-clock time, the device that wrote it, a tombstone flag, a
  local sequence number) and `sync_vv` (its version vector: per device, the
  clock value of that device's latest change to it).
- **Pull.** A device asks a peer for everything it recorded after the last
  sequence number it pulled; batches of 500 rows / 4 MB, each applied in one
  transaction together with the new bookmark, so an interrupted sync resumes.
- **Conflicts.** An incoming version that has seen everything the local one
  has replaces it; one already seen is skipped; two made without seeing each
  other: the later clock wins (ties by device id), and the merged vector is
  kept. Every device reaches the same result in any order.
- **Deletes** are tombstones, kept. Children whose parent was deleted
  elsewhere (`ON DELETE CASCADE`) are removed after a sync, as local deletes.
- **Unique columns** (a word's `(lesson_id, da/pl)`): a clash keeps the lower
  id; the other row is deleted everywhere. So a unique column must never be
  made from something two devices can make alike by chance (a time, a
  position): a learner's course's slug is its id.
- **Same schema only.** Peers sync only when their `SCHEMA_VERSION` is equal,
  and debug and release builds never sync with each other.

## Fitted to this schema

1. **Keys.** Tally needs one TEXT primary key per table. Here most tables
   have a TypeID, but some are keyed naturally (DATA.md): `settings(key)`,
   `day_log(day, device)`, `reviews(kind, item_id)`, `achievements(code)`,
   `speech_clips(cache_key)`, `course_lessons(course_id, lesson_id)`,
   `word_examples(word_id)`. The engine takes a table's primary-key columns,
   one or more, of any type: the sync key is their values as text, joined by
   U+001F when there are several (the triggers build the same string). Rows
   are found and upserted by those columns.
2. **Counters don't conflict.** `day_log` adds up a day's XP, answers and
   minutes; two devices practising the same day would each overwrite the
   other's totals. So a day's log is one row per device —
   `day_log(day, device)` — and every read sums the devices' rows. (Schema 8
   gives existing rows this device's id.)
3. **Device settings.** `device_settings(key, value)` is local; `db::get_setting`
   and `set_setting` route the keys in `DEVICE_SETTINGS` there: `theme`,
   `tz_offset_minutes`, `reminders_enabled`, `reminder_hour`,
   `reminder_minute`, `tts_prefetch`, `player_repeat`, the marks that say
   built-in content was seeded here (`starter_seeded`,
   `demo_first_steps_seeded_v1`, `bundled_courses_retired_v1`: synced, a
   device joining a vault could seed the demo again into it), and sync's own. Schema
   8 moves those keys over. Everything else in `settings` syncs (API keys
   included: they only ever go to paired devices, encrypted).
4. **Device-local columns move out.** A synced row must mean the same on every
   device. `files.path` (where this device wrote the bytes out) becomes
   `file_paths(file_id, path)`, local. The same for any other column that
   names something on this device only.
5. **Work is done by the device that started it.** A song being made (Suno
   task), a podcast's audio, a picture, a tutor reply being written: only the
   device whose sync record of the row has itself as origin resumes or polls
   it (`sync::written_here`). Otherwise both devices would download the same
   song and make two copies.
6. **The day's five** (`daily_ids`, `daily_day`) sync: both devices show the
   same lessons; if both chose a list while apart, the later wins.
7. **No backfill of the learner's content.** Schema 8 records the rows already
   there as this device's first version of them (bookkeeping, not content).

## Transport (as Tally)

- **Discovery:** UDP broadcast on port 47821, `POLSKI/1 Q <tag> <me>` answered
  by `POLSKI/1 R <tag> <id> <port>`; the tag is the first 8 bytes of
  `sha256("polski-vault:" + vault_id)`. (Dansk: `DANSK/1`, `dansk-vault:`, and
  its own ports, so the two apps never meet.)
- **Connection:** TCP (preferred port 47822), Noise `XX_25519_ChaChaPoly_BLAKE2s`
  for sync and `XXpsk3` with the QR's one-time token for pairing; messages in
  MessagePack, chunked, at most 256 MiB; a peer's static key must be in
  `vault_devices` and not in `vault_revocations`.
- **Pairing:** the QR holds `polski-pair:1:` + the vault, the host's id, name,
  key and addresses, and a token valid for 10 minutes, used once.
- **Protocol:** `Hello` → `Seen` → the client pulls → `YourTurn` → the server
  pulls → `Finished` → digests compared (skipped when nothing changed).

## Code

- `src-tauri/src/sync/`: `engine.rs` (export, apply, digest, join), `triggers.rs`
  (trigger SQL generated from the schema at upgrade time: no separate
  script), `wire.rs`, `discovery.rs`, `session.rs`, `service.rs` (the loop and
  the commands), `background.rs` (Android), tests beside each.
- `src-tauri/plugins/sync-background`: the WorkManager job (Kotlin → JNI).
- `src/screens/Devices.tsx`, `src/lib/sync.ts`.
- Every schema change from now on re-creates the triggers of the tables it
  touches; a test fails if a synced table's triggers don't list its columns.
