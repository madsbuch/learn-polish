//! Correctness of the sync engine on real databases (the app's own schema,
//! demo course, triggers and all), several devices in one process.
//!
//! The heart of it is `random_histories_converge_*`: devices make random
//! edits — the kinds the app makes, across plain, natural-key and per-device
//! tables — and sync in random pairs, with small batches and connections
//! dropped half-way, for many seeds. Afterwards every device must hold
//! exactly the same rows and versions, with no reference left dangling.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::engine::{self, ApplyStats, Batch, Digest, Value};
use crate::db::{self, Conn};
use crate::ids::*;
use crate::schema::{day_log, day_totals, files, memories, sync_rows, sync_state, words};

pub(crate) struct Device {
    pub(crate) conn: Conn,
    pub(crate) id: String,
}

static DEVICES: AtomicUsize = AtomicUsize::new(0);

/// A fresh install: the app's database as it opens the first time.
pub(crate) fn device() -> Device {
    let n = DEVICES.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join("learn-polish-tests").join(format!("sync-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut conn = db::open(&path).unwrap();
    let id = engine::local_state(&mut conn).unwrap().device_id;
    Device { conn, id }
}

fn exec(d: &mut Device, sql: &str) {
    d.conn.batch_execute(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
}

fn two(devices: &mut [Device], i: usize, j: usize) -> (&mut Device, &mut Device) {
    assert_ne!(i, j);
    if i < j {
        let (a, b) = devices.split_at_mut(j);
        (&mut a[i], &mut b[0])
    } else {
        let (a, b) = devices.split_at_mut(i);
        (&mut b[0], &mut a[j])
    }
}

fn pull(into: &mut Device, from: &mut Device, rows: i64, bytes: usize) -> ApplyStats {
    engine::pull_all(&mut into.conn, &mut from.conn, rows, bytes).unwrap()
}

fn sync_pair(a: &mut Device, b: &mut Device) -> (ApplyStats, ApplyStats) {
    let x = pull(a, b, 4, 64 * 1024);
    let y = pull(b, a, 4, 64 * 1024);
    (x, y)
}

/// Pulls at most `batches` batches, as a connection that drops would.
fn pull_some(into: &mut Device, from: &mut Device, batches: usize, rows: i64) {
    for _ in 0..batches {
        let since = engine::pulled_from(&mut into.conn, &from.id).unwrap();
        let batch = engine::export(&mut from.conn, since, &into.id, rows, 1024).unwrap();
        engine::apply(&mut into.conn, &from.id, &batch).unwrap();
        if batch.done {
            break;
        }
    }
}

#[derive(QueryableByName)]
struct Version {
    #[diesel(sql_type = Text)]
    line: String,
}

/// Every synced table's rows, with their versions, as comparable text.
pub(crate) fn dump(conn: &mut Conn) -> BTreeMap<String, Vec<String>> {
    let tables = engine::synced_tables(conn).unwrap();
    let mut out = BTreeMap::new();
    for t in tables.values() {
        let rows = engine::rows_of(conn, t).unwrap();
        out.insert(
            t.name.clone(),
            rows.iter()
                .map(|r| r.iter().map(|(c, v)| format!("{c}={v:?}")).collect::<Vec<_>>().join("|"))
                .collect(),
        );
    }
    let versions = diesel::sql_query(
        "SELECT tbl || ' ' || pk || ' ' || hlc || ' ' || origin || ' ' || deleted AS line FROM sync_rows ORDER BY tbl, pk",
    )
    .load::<Version>(conn)
    .unwrap();
    out.insert("~versions".into(), versions.into_iter().map(|v| v.line).collect());
    let vv = diesel::sql_query("SELECT tbl || ' ' || pk || ' ' || device || ' ' || counter AS line FROM sync_vv ORDER BY tbl, pk, device")
        .load::<Version>(conn)
        .unwrap();
    out.insert("~vv".into(), vv.into_iter().map(|v| v.line).collect());
    out
}

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Text)]
    id: String,
}

fn ids(d: &mut Device, sql: &str) -> Vec<String> {
    diesel::sql_query(sql).load::<Id>(&mut d.conn).unwrap().into_iter().map(|i| i.id).collect()
}

fn pick(rng: &mut StdRng, v: &[String]) -> Option<String> {
    if v.is_empty() {
        None
    } else {
        Some(v[rng.random_range(0..v.len())].clone())
    }
}

fn count(d: &mut Device, sql: &str) -> i64 {
    #[derive(QueryableByName)]
    struct N {
        #[diesel(sql_type = BigInt)]
        n: i64,
    }
    diesel::sql_query(sql).get_result::<N>(&mut d.conn).unwrap_or_else(|e| panic!("{sql}: {e}")).n
}

/// A time for made-up ids: a seeded test makes the same ones every run.
fn at(n: usize) -> u64 {
    1_700_000_000_000 + crate::convert::bytes(n)
}

/// One random edit of the kind the app makes.
fn random_edit(rng: &mut StdRng, d: &mut Device, n: usize) {
    let now = 1_700_000_000 + crate::convert::count(n);
    match rng.random_range(0..22) {
        0 | 1 => {
            let id = CourseId::at(at(n), rng.random());
            exec(d, &format!(
                "INSERT INTO courses (id, slug, ord, title_pl, title_en, summary_en, created_at) VALUES ('{id}', 'course-{id}', {n}, 'Kurs', 'Course {n}', '', {now})"
            ));
        }
        2 => {
            if let Some(id) = pick(rng, &ids(d, "SELECT id FROM courses")) {
                exec(d, &format!("UPDATE courses SET title_en = 'edited {n}' WHERE id = '{id}'"));
            }
        }
        3 => {
            if let Some(id) = pick(rng, &ids(d, "SELECT id FROM courses")) {
                exec(d, &format!("DELETE FROM courses WHERE id = '{id}'"));
            }
        }
        4 => {
            // A lesson in a course, with a few words from a small vocabulary.
            if let Some(course) = pick(rng, &ids(d, "SELECT id FROM courses")) {
                let lesson = LessonId::at(at(n), rng.random());
                exec(d, &format!(
                    "INSERT INTO lessons (id, lesson_type, title, complexity, period_days, created_at) VALUES ('{lesson}', 'words', 'Lesson {n}', 'low', 7, {now});
                     INSERT INTO course_lessons (course_id, lesson_id, ord) VALUES ('{course}', '{lesson}', {n});"
                ));
                for k in 0..rng.random_range(0..3usize) {
                    let word = WordId::at(at(n), rng.random());
                    exec(d, &format!("INSERT OR IGNORE INTO words (id, lesson_id, ord, pl, en, pos) VALUES ('{word}', '{lesson}', {k}, 'w{k}', 'x', 'n')"));
                }
            }
        }
        5 => {
            // The same few words on every device: the unique index will clash.
            if let Some(lesson) = pick(rng, &ids(d, "SELECT id FROM lessons")) {
                let pl = ["kot", "pies", "dom", "las"][rng.random_range(0..4)];
                let word = WordId::at(at(n), rng.random());
                exec(d, &format!("INSERT OR IGNORE INTO words (id, lesson_id, ord, pl, en, pos) VALUES ('{word}', '{lesson}', {n}, '{pl}', 'x', 'n')"));
            }
        }
        6 => {
            if let Some(id) = pick(rng, &ids(d, "SELECT id FROM lessons")) {
                exec(d, &format!("DELETE FROM lessons WHERE id = '{id}'"));
            }
        }
        7 => {
            if let Some(id) = pick(rng, &ids(d, "SELECT id FROM words")) {
                exec(d, &format!("UPDATE words SET en = 'meaning {n}' WHERE id = '{id}'"));
            }
        }
        8 | 9 => {
            // Practice: a review (keyed by kind and item), the day's log.
            if let Some(word) = pick(rng, &ids(d, "SELECT id FROM words")) {
                exec(d, &format!(
                    "INSERT INTO reviews (kind, item_id, due_day, interval, ease, reps, lapses, last_day) VALUES ('word', '{word}', {n}, 1, 2.5, 1, 0, {n})
                     ON CONFLICT (kind, item_id) DO UPDATE SET reps = reps + 1, ease = ease * 1.1, due_day = {n}"
                ));
            }
            let day = 20_000 + crate::convert::count(n % 3);
            crate::stats::record_answer(&mut d.conn, day, rng.random_range(0..12), rng.random_bool(0.7)).unwrap();
        }
        10 => {
            let key = ["daily_goal", "ai_model", "openrouter_api_key"][rng.random_range(0..3)];
            db::set_setting(&mut d.conn, key, &format!("v{n}")).unwrap();
        }
        11 => {
            let code = ["first_lesson", "streak_7", "words_100"][rng.random_range(0..3)];
            exec(d, &format!("INSERT OR IGNORE INTO achievements (code, unlocked_day, unlocked_at) VALUES ('{code}', {n}, {now})"));
        }
        12 => {
            // A lesson listed in a course (ignored where it is in one already:
            // the unique index; two devices listing it apart clash when they
            // sync, and the lower key stays), or taken out of one.
            let course = pick(rng, &ids(d, "SELECT id FROM courses"));
            let lesson = pick(rng, &ids(d, "SELECT id FROM lessons"));
            if let (Some(course), Some(lesson)) = (course, lesson) {
                if rng.random_bool(0.6) {
                    exec(d, &format!("INSERT OR IGNORE INTO course_lessons (course_id, lesson_id, ord) VALUES ('{course}', '{lesson}', {n})"));
                } else {
                    exec(d, &format!("DELETE FROM course_lessons WHERE course_id = '{course}' AND lesson_id = '{lesson}'"));
                }
            }
        }
        13 => {
            // A course picture: a file (blob), referenced with ON DELETE SET NULL.
            if let Some(course) = pick(rng, &ids(d, "SELECT id FROM courses")) {
                let k: u8 = rng.random_range(0..5);
                let bytes: Vec<u8> = (0..3000u32).map(|i| u8::try_from(i % 251).unwrap_or(0).wrapping_mul(k + 1)).collect();
                let file = crate::files::store(&mut d.conn, crate::files::COURSE_IMAGE, "c.jpg", "image/jpeg", &bytes).unwrap();
                exec(d, &format!("UPDATE courses SET image_file_id = '{file}' WHERE id = '{course}'"));
            }
        }
        14 => {
            if let Some(file) = pick(rng, &ids(d, "SELECT id FROM files")) {
                let file = FileId::parse(&file).unwrap();
                crate::files::delete(&mut d.conn, &[file]).unwrap();
            }
        }
        15 => {
            let conversation = ConversationId::at(at(n), rng.random());
            let message = MessageId::at(at(n), rng.random());
            exec(d, &format!(
                "INSERT INTO tutor_conversations (id, title, created_at, updated_at) VALUES ('{conversation}', 'Chat {n}', {now}, {now});
                 INSERT INTO tutor_messages (id, role, content, created_at, conversation_id) VALUES ('{message}', 'user', 'hi {n}', {now}, '{conversation}');"
            ));
        }
        16 => {
            // A conversation continued: a turn of its own, the conversation touched.
            if let Some(conversation) = pick(rng, &ids(d, "SELECT id FROM tutor_conversations")) {
                let message = MessageId::at(at(n), rng.random());
                exec(d, &format!(
                    "INSERT INTO tutor_messages (id, role, content, created_at, conversation_id) VALUES ('{message}', 'assistant', 'turn {n}', {now}, '{conversation}');
                     UPDATE tutor_conversations SET updated_at = {now} WHERE id = '{conversation}';"
                ));
            }
        }
        17 => {
            if let Some(conversation) = pick(rng, &ids(d, "SELECT id FROM tutor_conversations")) {
                exec(d, &format!("DELETE FROM tutor_conversations WHERE id = '{conversation}'"));
            }
        }
        18 => {
            if let Some(word) = pick(rng, &ids(d, "SELECT id FROM words")) {
                exec(d, &format!(
                    "INSERT INTO word_examples (word_id, examples, created_at) VALUES ('{word}', '[{n}]', {now})
                     ON CONFLICT (word_id) DO UPDATE SET examples = excluded.examples"
                ));
            }
        }
        19 => {
            // A speech clip, keyed by its text: the same key on two devices.
            let key = ["clip-a", "clip-b", "clip-c"][rng.random_range(0..3)];
            let file = crate::files::store(&mut d.conn, crate::files::SPEECH, "s.mp3", "audio/mpeg", format!("{key}{n}").as_bytes()).unwrap();
            exec(d, &format!(
                "INSERT INTO speech_clips (cache_key, model, voice, text, file_id, created_at) VALUES ('{key}', 'm', 'v', 't', '{file}', {now})
                 ON CONFLICT (cache_key) DO UPDATE SET file_id = excluded.file_id"
            ));
        }
        20 => {
            // A lesson practised: its schedule moves (a REAL column too).
            if let Some(lesson) = pick(rng, &ids(d, "SELECT id FROM lessons")) {
                exec(d, &format!("UPDATE lessons SET reps = reps + 1, due_day = {n}, ease = ease * 1.05 + 0.1 WHERE id = '{lesson}'"));
            }
        }
        _ => {
            let memory = MemoryId::at(at(n), rng.random());
            exec(d, &format!("INSERT INTO memories (id, content, created_at, updated_at) VALUES ('{memory}', 'likes cats {n}', {now}, {now})"));
        }
    }
}

/// Syncs every pair until a whole round changes nothing anywhere, cleaning up
/// orphans as the app does after a sync. Panics if it doesn't settle.
fn settle(devices: &mut [Device]) {
    for round in 0..12 {
        let mut changed = 0;
        for i in 0..devices.len() {
            for j in 0..devices.len() {
                if i != j {
                    let (a, b) = two(devices, i, j);
                    let s = pull(a, b, 7, 16 * 1024);
                    changed += s.applied + s.conflicts + s.unique_merges;
                }
            }
        }
        for d in devices.iter_mut() {
            changed += engine::remove_orphans(&mut d.conn).unwrap();
        }
        if changed == 0 {
            return;
        }
        assert!(round < 11, "sync didn't settle");
    }
}

fn assert_identical(devices: &mut [Device]) {
    let first = dump(&mut devices[0].conn);
    let first_digest = engine::digest(&mut devices[0].conn).unwrap();
    for d in devices[1..].iter_mut() {
        let other = dump(&mut d.conn);
        for (table, rows) in &first {
            assert_eq!(rows, &other[table], "{table} differs between devices");
        }
        let digest = engine::digest(&mut d.conn).unwrap();
        assert_eq!(engine::digest_differences(&first_digest, &digest), Vec::<String>::new());
    }
    for d in devices.iter_mut() {
        assert_eq!(db::foreign_key_violations(&mut d.conn).unwrap(), 0, "dangling references");
        let applying: i64 = sync_state::table.select(sync_state::applying).first(&mut d.conn).unwrap();
        assert_eq!(applying, 0);
    }
}

fn run_history(seed: u64, devices_n: usize, steps: usize) {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut devices: Vec<Device> = (0..devices_n).map(|_| device()).collect();
    for n in 0..steps {
        let d = rng.random_range(0..devices_n);
        random_edit(&mut rng, &mut devices[d], n);
        if rng.random_bool(0.25) {
            let a = rng.random_range(0..devices_n);
            let b = (a + rng.random_range(1..devices_n)) % devices_n;
            let (x, y) = two(&mut devices, a, b);
            if rng.random_bool(0.3) {
                // A connection that drops part-way.
                let k = rng.random_range(0..3);
                let rows = rng.random_range(1..4);
                pull_some(x, y, k, rows);
            } else {
                sync_pair(x, y);
            }
        }
    }
    settle(&mut devices);
    assert_identical(&mut devices);

    // Quiet once settled: nothing more moves, in either direction.
    for i in 0..devices_n {
        for j in 0..devices_n {
            if i != j {
                let (a, b) = two(&mut devices, i, j);
                let s = pull(a, b, 50, 1 << 20);
                assert_eq!(s.applied + s.conflicts, 0, "seed {seed}: still moving after settling");
            }
        }
    }
}

#[test]
fn random_histories_converge_between_two_devices() {
    for seed in 0..12 {
        run_history(seed, 2, 120);
    }
}

#[test]
fn random_histories_converge_between_three_devices() {
    for seed in 100..108 {
        run_history(seed, 3, 160);
    }
}

// ---------------------------------------------------------------------------
// Specific cases, spelled out
// ---------------------------------------------------------------------------

fn memory(d: &mut Device, id: MemoryId, content: &str) {
    exec(d, &format!("INSERT INTO memories (id, content, created_at, updated_at) VALUES ('{id}', '{content}', 1, 1)"));
}

#[test]
fn two_fresh_installs_converge_without_duplicates() {
    let mut devices = [device(), device()];
    let courses = |d: &mut Device| count(d, "SELECT count(*) AS n FROM courses");
    let before: Vec<i64> = devices.iter_mut().map(courses).collect();
    assert_eq!(before, [1, 1], "each has the demo course");
    settle(&mut devices);
    assert_identical(&mut devices);
    for d in devices.iter_mut() {
        assert_eq!(courses(d), 1);
        let types = crate::convert::count(crate::lesson_types::REGISTRY.len());
        assert_eq!(count(d, "SELECT count(*) AS n FROM lessons"), types);
        assert_eq!(count(d, "SELECT count(*) AS n FROM words"), 31);
        assert_eq!(count(d, "SELECT count(*) AS n FROM sentences"), 10);
        assert_eq!(count(d, "SELECT count(*) AS n FROM conversation_lines"), 6);
        assert_eq!(count(d, "SELECT count(*) AS n FROM course_lessons"), types);
        assert_eq!(count(d, "SELECT count(*) AS n FROM story_panels"), 4);
        assert_eq!(count(d, "SELECT count(*) AS n FROM songs"), 1);
        // Each kept its own seeding marks: they are about the install.
        assert_eq!(db::get_setting(&mut d.conn, crate::demo_course::SEEDED).unwrap().as_deref(), Some("1"));
        // And neither counts the demo as the learner's data.
        assert_eq!(engine::held_data(&mut d.conn).unwrap(), Vec::<(String, i64)>::new());
    }
}

#[test]
fn deletes_reach_the_other_device_and_stay_deleted() {
    let (mut a, mut b) = (device(), device());
    let id = MemoryId::new();
    memory(&mut a, id, "likes cats");
    sync_pair(&mut a, &mut b);
    exec(&mut b, &format!("DELETE FROM memories WHERE id = '{id}'"));
    sync_pair(&mut a, &mut b);
    assert_eq!(count(&mut a, "SELECT count(*) AS n FROM memories"), 0);
    // A stale copy of the old version arriving later can't bring it back.
    let c = device();
    let mut devices = [a, b, c];
    settle(&mut devices);
    assert_identical(&mut devices);
    assert_eq!(count(&mut devices[2], "SELECT count(*) AS n FROM memories"), 0);
}

#[test]
fn concurrent_edits_to_a_row_settle_on_the_later_one_everywhere() {
    let (mut a, mut b) = (device(), device());
    let id = MemoryId::new();
    memory(&mut a, id, "likes cats");
    sync_pair(&mut a, &mut b);
    exec(&mut a, &format!("UPDATE memories SET content = 'first' WHERE id = '{id}'"));
    std::thread::sleep(std::time::Duration::from_millis(5));
    exec(&mut b, &format!("UPDATE memories SET content = 'second' WHERE id = '{id}'"));
    let mut devices = [a, b];
    settle(&mut devices);
    assert_identical(&mut devices);
    let content: String = memories::table.select(memories::content).first(&mut devices[0].conn).unwrap();
    assert_eq!(content, "second");
}

#[test]
fn the_same_word_added_on_two_devices_ends_up_as_one() {
    let (mut a, mut b) = (device(), device());
    let lesson: LessonId = words::table.select(words::lesson_id).first(&mut a.conn).unwrap();
    let (low, high) = (WordId::at(1, 0), WordId::at(2, 0));
    exec(&mut a, &format!("INSERT INTO words (id, lesson_id, ord, pl, en, pos) VALUES ('{high}', '{lesson}', 99, 'pies', 'dog', 'n')"));
    exec(&mut b, &format!("INSERT INTO words (id, lesson_id, ord, pl, en, pos) VALUES ('{low}', '{lesson}', 99, 'pies', 'a dog', 'n')"));
    // A's word has history, which goes with it.
    exec(&mut a, &format!("INSERT INTO reviews VALUES ('word', '{high}', 1, 1, 2.5, 1, 0, 1)"));
    let mut devices = [a, b];
    settle(&mut devices);
    assert_identical(&mut devices);
    let kept: Vec<WordId> = words::table.filter(words::pl.eq("pies")).select(words::id).load(&mut devices[0].conn).unwrap();
    assert_eq!(kept, vec![low], "the lower id keeps its place");
    assert_eq!(count(&mut devices[0], "SELECT count(*) AS n FROM reviews"), 0);
}

#[test]
fn files_and_numbers_arrive_exactly() {
    let (mut a, mut b) = (device(), device());
    let bytes: Vec<u8> = (0..300_000u32).map(|i| u8::try_from((i * 7) % 256).unwrap_or(0)).collect();
    let file = crate::files::store(&mut a.conn, crate::files::WORD_IMAGE, "w.jpg", "image/jpeg", &bytes).unwrap();
    // Values that only survive if nothing goes through text: a real that
    // needs all 17 digits, an integer in a REAL column, a blob of zeros,
    // and text that looks like a number.
    let ease = 0.1 + 0.2;
    let lesson: LessonId = words::table.select(words::lesson_id).first(&mut a.conn).unwrap();
    diesel::sql_query("UPDATE lessons SET ease = ?, summary = '007' WHERE id = ?")
        .bind::<diesel::sql_types::Double, _>(ease)
        .bind::<Text, _>(lesson.to_string())
        .execute(&mut a.conn)
        .unwrap();
    exec(&mut a, &format!("UPDATE words SET image_file_id = '{file}' WHERE lesson_id = '{lesson}' AND ord = 0"));
    let zeros = crate::files::store(&mut a.conn, crate::files::WORD_IMAGE, "z.jpg", "image/jpeg", &[0, 0, 0]).unwrap();
    exec(&mut a, &format!("UPDATE words SET image_file_id = '{zeros}' WHERE lesson_id = '{lesson}' AND ord = 1"));
    exec(&mut a, "INSERT INTO reviews VALUES ('word', 'x', 1, 1, 3, 1, 0, 1)");
    // Small batches: the big rows still go, one per batch.
    pull(&mut b, &mut a, 50, 1024);
    assert_eq!(crate::files::read(&mut b.conn, file).unwrap().1, bytes);
    assert_eq!(crate::files::read(&mut b.conn, zeros).unwrap().1, vec![0, 0, 0]);
    let got: (f64, String) = crate::schema::lessons::table
        .find(lesson)
        .select((crate::schema::lessons::ease, crate::schema::lessons::summary))
        .first(&mut b.conn)
        .unwrap();
    assert_eq!(got.0.to_bits(), ease.to_bits());
    assert_eq!(got.1, "007");
    assert_eq!(count(&mut b, "SELECT count(*) AS n FROM reviews WHERE typeof(ease) = 'real' AND ease = 3"), 1);
    let mut devices = [a, b];
    settle(&mut devices);
    assert_identical(&mut devices);
}

#[test]
fn an_interrupted_pull_resumes_where_it_stopped() {
    let mut devices = [device(), device()];
    settle(&mut devices);
    let [a, b] = &mut devices;
    for i in 0..40 {
        memory(a, MemoryId::at(at(i), 0), "fact");
    }
    pull_some(b, a, 2, 5);
    let half = count(b, "SELECT count(*) AS n FROM memories");
    assert!(half > 0 && half < 40, "{half}");
    let s = pull(b, a, 5, 1 << 20);
    // Exactly the rest: nothing pulled before the drop came again.
    assert_eq!(s.applied, crate::convert::index(40 - half));
    assert_eq!(count(b, "SELECT count(*) AS n FROM memories"), 40);
}

#[test]
fn applying_the_same_batch_twice_changes_nothing() {
    let (mut a, mut b) = (device(), device());
    memory(&mut a, MemoryId::new(), "fact");
    let batch = engine::export(&mut a.conn, 0, &b.id, 1000, 1 << 20).unwrap();
    engine::apply(&mut b.conn, &a.id, &batch).unwrap();
    let before = dump(&mut b.conn);
    let again = engine::apply(&mut b.conn, &a.id, &batch).unwrap();
    assert_eq!(again.applied + again.conflicts, 0);
    assert_eq!(dump(&mut b.conn), before);
}

/// A big batch is taken in parts, each its own transaction, so the lock is
/// let go between them; a pull cut off between parts records nothing as
/// pulled, and the parts already in are found equal when it comes again.
#[test]
fn a_batch_taken_in_parts_ends_the_same_and_resumes() {
    let (mut a, mut b) = (device(), device());
    for i in 0..30 {
        memory(&mut a, MemoryId::at(at(i), 0), "fact");
    }
    let batch = engine::export(&mut a.conn, 0, &b.id, 1000, 1 << 20).unwrap();
    let total = batch.changes.len();
    let parts = engine::parts(batch.changes.clone(), 4, 1 << 20);
    assert!(parts.len() > 1 && parts.iter().all(|p| !p.is_empty() && p.len() <= 4));
    assert_eq!(parts.iter().map(Vec::len).sum::<usize>(), total);
    // Only the first part, then the connection drops.
    engine::apply_part(&mut b.conn, &a.id, &parts[0], None).unwrap();
    assert_eq!(engine::pulled_from(&mut b.conn, &a.id).unwrap(), 0, "nothing recorded as pulled yet");
    // Again from the start, in parts: the first part is skipped as known.
    let last = parts.len() - 1;
    for (n, part) in parts.iter().enumerate() {
        let stats = engine::apply_part(&mut b.conn, &a.id, part, (n == last).then_some(batch.upto)).unwrap();
        if n == 0 {
            assert_eq!(stats.applied + stats.conflicts, 0, "the first part is in already");
        }
    }
    assert_eq!(engine::pulled_from(&mut b.conn, &a.id).unwrap(), batch.upto);
    assert_eq!(count(&mut b, "SELECT count(*) AS n FROM memories"), 30);
    // A part is kept small by size too, but always holds one change.
    assert_eq!(engine::parts(batch.changes, 100, 1).len(), total);
    // The digest a table at a time is the whole digest.
    let whole = engine::digest(&mut b.conn).unwrap();
    for table in engine::digest_tables(&mut b.conn).unwrap() {
        assert_eq!(engine::table_digest(&mut b.conn, &table).unwrap().as_ref(), whole.get(&table));
    }
}

#[test]
fn a_bad_batch_changes_nothing_at_all() {
    let (mut a, mut b) = (device(), device());
    memory(&mut a, MemoryId::new(), "fact");
    let mut batch = engine::export(&mut a.conn, 0, &b.id, 1000, 1 << 20).unwrap();
    let before = dump(&mut b.conn);
    // A change for a table that isn't synced, at the end of an otherwise good batch.
    let mut evil = batch.changes[0].clone();
    evil.tbl = "sync_state".into();
    batch.changes.push(evil);
    assert!(engine::apply(&mut b.conn, &a.id, &batch).is_err());
    assert_eq!(dump(&mut b.conn), before);
    assert_eq!(engine::pulled_from(&mut b.conn, &a.id).unwrap(), 0);
    let applying: i64 = sync_state::table.select(sync_state::applying).first(&mut b.conn).unwrap();
    assert_eq!(applying, 0);
    assert_eq!(count(&mut b, "SELECT foreign_keys AS n FROM pragma_foreign_keys"), 1, "foreign keys are back on");
}

#[test]
fn a_change_with_unknown_columns_is_refused() {
    let (mut a, mut b) = (device(), device());
    memory(&mut a, MemoryId::new(), "fact");
    let mut batch: Batch = engine::export(&mut a.conn, 0, &b.id, 1000, 1 << 20).unwrap();
    let c = batch.changes.iter_mut().find(|c| c.tbl == "memories").unwrap();
    c.data.as_mut().unwrap().push(("evil".into(), Value::Text("x".into())));
    assert!(engine::apply(&mut b.conn, &a.id, &batch).is_err());
    // A missing one, or a key that isn't the row's, likewise.
    let mut batch: Batch = engine::export(&mut a.conn, 0, &b.id, 1000, 1 << 20).unwrap();
    let c = batch.changes.iter_mut().find(|c| c.tbl == "memories").unwrap();
    c.data.as_mut().unwrap().retain(|(column, _)| column != "content");
    assert!(engine::apply(&mut b.conn, &a.id, &batch).is_err());
    let mut batch: Batch = engine::export(&mut a.conn, 0, &b.id, 1000, 1 << 20).unwrap();
    let c = batch.changes.iter_mut().find(|c| c.tbl == "memories").unwrap();
    c.pk = MemoryId::new().to_string();
    assert!(engine::apply(&mut b.conn, &a.id, &batch).is_err());
}

#[test]
fn a_day_practised_on_two_devices_adds_up() {
    let (mut a, mut b) = (device(), device());
    crate::stats::record_answer(&mut a.conn, 20_000, 10, true).unwrap();
    crate::stats::record_answer(&mut b.conn, 20_000, 7, false).unwrap();
    crate::stats::add_xp(&mut b.conn, 20_000, 30).unwrap();
    let mut devices = [a, b];
    settle(&mut devices);
    assert_identical(&mut devices);
    for d in devices.iter_mut() {
        assert_eq!(day_log::table.count().get_result::<i64>(&mut d.conn).unwrap(), 2, "a row per device");
        let (xp, answered, correct): (i64, i64, i64) = day_totals::table
            .find(20_000)
            .select((day_totals::xp, day_totals::answered, day_totals::correct))
            .first(&mut d.conn)
            .unwrap();
        assert_eq!((xp, answered, correct), (47, 2, 1));
        assert_eq!(crate::stats::total_xp(&mut d.conn).unwrap(), 47);
    }
    // More practice later on one device adds to its own row only.
    crate::stats::record_answer(&mut devices[0].conn, 20_000, 3, true).unwrap();
    settle(&mut devices);
    assert_eq!(crate::stats::day_xp(&mut devices[1].conn, 20_000).unwrap(), 50);
}

#[test]
fn children_of_a_parent_deleted_elsewhere_go_too() {
    let (mut a, mut b) = (device(), device());
    let lesson: LessonId = words::table.select(words::lesson_id).first(&mut a.conn).unwrap();
    sync_pair(&mut a, &mut b);
    // A deletes the lesson while B, not knowing, adds a word to it.
    exec(&mut a, &format!("DELETE FROM lessons WHERE id = '{lesson}'"));
    let word = WordId::new();
    exec(&mut b, &format!("INSERT INTO words (id, lesson_id, ord, pl, en, pos) VALUES ('{word}', '{lesson}', 50, 'nowe', 'new', 'n')"));
    exec(&mut b, &format!("INSERT INTO word_examples (word_id, examples, created_at) VALUES ('{word}', '[]', 1)"));
    let (_, into_b) = sync_pair(&mut a, &mut b);
    assert!(into_b.applied > 0);
    // The word (its examples go with it, by the foreign key).
    assert_eq!(engine::remove_orphans(&mut b.conn).unwrap(), 1);
    assert_eq!(count(&mut b, "SELECT count(*) AS n FROM word_examples"), 0);
    let mut devices = [a, b];
    settle(&mut devices);
    assert_identical(&mut devices);
    for d in devices.iter_mut() {
        assert_eq!(words::table.filter(words::lesson_id.eq(lesson)).count().get_result::<i64>(&mut d.conn).unwrap(), 0);
        assert_eq!(count(d, "SELECT count(*) AS n FROM word_examples"), 0);
    }
}

#[test]
fn a_parent_that_has_not_arrived_yet_is_not_a_deletion() {
    let (mut a, mut b) = (device(), device());
    let lesson = LessonId::new();
    exec(&mut a, &format!("INSERT INTO lessons (id, title, complexity, period_days, created_at) VALUES ('{lesson}', 'New', 'low', 7, 1)"));
    let word = WordId::new();
    exec(&mut a, &format!("INSERT INTO words (id, lesson_id, ord, pl, en, pos) VALUES ('{word}', '{lesson}', 1, 'kot', 'cat', 'n')"));
    // The lesson changes after its word: it comes later in A's order.
    exec(&mut a, &format!("UPDATE lessons SET title = 'Renamed' WHERE id = '{lesson}'"));
    let batch = engine::export(&mut a.conn, 0, &b.id, 1000, 1 << 20).unwrap();
    let cut: Vec<_> = batch.changes.iter().take_while(|c| !(c.tbl == "lessons" && c.pk == lesson.to_string())).cloned().collect();
    assert!(cut.iter().any(|c| c.tbl == "words" && c.pk == word.to_string()));
    let partial = Batch { upto: 0, done: false, changes: cut };
    engine::apply(&mut b.conn, &a.id, &partial).unwrap();
    assert_eq!(engine::remove_orphans(&mut b.conn).unwrap(), 0);
    assert_eq!(words::table.find(word).count().get_result::<i64>(&mut b.conn).unwrap(), 1);
}

#[test]
fn written_here_is_true_only_on_the_device_that_wrote_the_row() {
    let (mut a, mut b) = (device(), device());
    let id = MemoryId::new();
    memory(&mut a, id, "likes cats");
    sync_pair(&mut a, &mut b);
    assert!(super::written_here(&mut a.conn, "memories", &id.to_string()));
    assert!(!super::written_here(&mut b.conn, "memories", &id.to_string()));
    // Until B changes it: then B is.
    exec(&mut b, &format!("UPDATE memories SET content = 'likes dogs' WHERE id = '{id}'"));
    assert!(super::written_here(&mut b.conn, "memories", &id.to_string()));
    sync_pair(&mut a, &mut b);
    assert!(!super::written_here(&mut a.conn, "memories", &id.to_string()));
}

#[test]
fn a_reply_being_written_elsewhere_is_left_alone_at_startup() {
    let (mut a, mut b) = (device(), device());
    let conversation = ConversationId::new();
    exec(&mut a, &format!(
        "INSERT INTO tutor_conversations (id, title, created_at, updated_at, job_started_at) VALUES ('{conversation}', 'Chat', 1, 1, 5)"
    ));
    sync_pair(&mut a, &mut b);
    crate::tutor::recover_interrupted(&mut b.conn).unwrap();
    assert_eq!(count(&mut b, "SELECT count(*) AS n FROM tutor_conversations WHERE job_started_at IS NOT NULL"), 1);
    crate::tutor::recover_interrupted(&mut a.conn).unwrap();
    assert_eq!(count(&mut a, "SELECT count(*) AS n FROM tutor_conversations WHERE job_started_at IS NOT NULL"), 0);
}

#[test]
fn a_file_from_another_device_is_not_collected_as_an_orphan() {
    let (mut a, mut b) = (device(), device());
    let file = crate::files::store(&mut a.conn, crate::files::COURSE_IMAGE, "c.jpg", "image/jpeg", b"abc").unwrap();
    // Pulled before the row that uses it, as a cut-off sync would.
    pull(&mut b, &mut a, 1000, 1 << 20);
    assert_eq!(crate::files::collect_orphans(&mut b.conn).unwrap(), 0);
    assert_eq!(files::table.find(file).count().get_result::<i64>(&mut b.conn).unwrap(), 1);
    // On the device that stored it, an orphan is an orphan.
    assert_eq!(crate::files::collect_orphans(&mut a.conn).unwrap(), 1);
}

#[test]
fn joining_needs_an_empty_device_and_then_receives_everything() {
    let (mut a, mut b) = (device(), device());
    exec(&mut a, "UPDATE sync_state SET vault_id = 'v'");
    for i in 0..30 {
        memory(&mut a, MemoryId::at(at(i), 1), "fact");
    }
    db::set_setting(&mut a.conn, "openrouter_api_key", "sk-123").unwrap();

    // A device with data of its own is refused, and keeps it.
    crate::stats::add_xp(&mut b.conn, 20_000, 5).unwrap();
    let held = engine::held_data(&mut b.conn).unwrap();
    assert_eq!(held, vec![("day_log".to_string(), 1)]);
    assert!(engine::reset_for_join(&mut b.conn, "v", DeviceId::new(), None, false).is_err());
    assert_eq!(crate::stats::total_xp(&mut b.conn).unwrap(), 5);

    // An empty one (settings and the demo course don't count) joins.
    let mut c = device();
    db::set_setting(&mut c.conn, "daily_goal", "300").unwrap();
    db::set_setting(&mut c.conn, "theme", "dark").unwrap();
    let old_id = c.id.clone();
    let new_id = DeviceId::new();
    engine::reset_for_join(&mut c.conn, "v", new_id, Some((b"secret", b"public")), false).unwrap();
    assert_ne!(new_id.to_string(), old_id);
    assert_eq!(count(&mut c, "SELECT count(*) AS n FROM courses"), 0);
    assert_eq!(db::get_setting(&mut c.conn, "theme").unwrap().as_deref(), Some("dark"), "device settings stay");
    let mut c = Device { id: new_id.to_string(), ..c };
    pull(&mut c, &mut a, 10, 1 << 20);
    let mut devices = [a, c];
    settle(&mut devices);
    assert_identical(&mut devices);
    assert_eq!(db::get_setting(&mut devices[1].conn, "openrouter_api_key").unwrap().as_deref(), Some("sk-123"));
    assert_eq!(count(&mut devices[1], "SELECT count(*) AS n FROM memories"), 30);
}

/// Tally's: a write made on a device right after (and, here, in the middle
/// of) a sync is recorded as an ordinary local change — not taken for part of
/// what the sync applied — and reaches the other device.
#[test]
fn local_writes_made_during_a_sync_are_still_recorded() {
    let (mut a, mut b) = (device(), device());
    memory(&mut a, MemoryId::new(), "oats");
    sync_pair(&mut a, &mut b);
    memory(&mut b, MemoryId::new(), "eggs");
    let mut devices = [a, b];
    settle(&mut devices);
    assert_identical(&mut devices);
    assert_eq!(count(&mut devices[0], "SELECT count(*) AS n FROM memories"), 2);

    // Between two batches of a pull: the learner practises while a sync is
    // half-way through.
    let [mut a, mut b] = devices;
    for i in 0..20 {
        memory(&mut a, MemoryId::new(), &format!("fact {i}"));
    }
    pull_some(&mut b, &mut a, 1, 4);
    memory(&mut b, MemoryId::new(), "written mid-sync");
    let applying: i64 = sync_state::table.select(sync_state::applying).first(&mut b.conn).unwrap();
    assert_eq!(applying, 0, "no apply is left open between batches");
    let mut devices = [a, b];
    settle(&mut devices);
    assert_identical(&mut devices);
    assert_eq!(count(&mut devices[0], "SELECT count(*) AS n FROM memories WHERE content = 'written mid-sync'"), 1);
    assert_eq!(count(&mut devices[0], "SELECT count(*) AS n FROM memories"), 23);
}

#[test]
fn the_digest_notices_any_difference() {
    let (mut a, b) = (device(), device());
    let id = MemoryId::new();
    memory(&mut a, id, "fact");
    let mut devices = [a, b];
    settle(&mut devices);
    let same: Digest = engine::digest(&mut devices[0].conn).unwrap();
    assert!(engine::digest_differences(&same, &engine::digest(&mut devices[1].conn).unwrap()).is_empty());
    // Changed behind sync's back (no trigger): the digest still sees it.
    exec(&mut devices[1], "UPDATE sync_state SET applying = 1");
    exec(&mut devices[1], "UPDATE memories SET content = 'other'");
    exec(&mut devices[1], "UPDATE sync_state SET applying = 0");
    let diff = engine::digest_differences(&same, &engine::digest(&mut devices[1].conn).unwrap());
    assert_eq!(diff, vec!["memories".to_string()]);
}

#[test]
fn a_revocation_cannot_be_deleted_by_a_peer() {
    let (mut a, mut b) = (device(), device());
    exec(&mut a, "INSERT INTO vault_revocations (public_key, device_id, revoked_at) VALUES ('cc', 'lost', 'now')");
    sync_pair(&mut a, &mut b);
    // A modified client claims a newer deletion of it.
    let mut batch = engine::export(&mut a.conn, 0, &b.id, 1000, 1 << 20).unwrap();
    let c = batch.changes.iter_mut().find(|c| c.tbl == "vault_revocations").unwrap();
    c.deleted = true;
    c.data = None;
    c.hlc = "9999999999999.000000".into();
    for v in c.vv.values_mut() {
        *v += 1_000_000_000_000_000;
    }
    engine::apply(&mut b.conn, &a.id, &batch).unwrap();
    assert_eq!(count(&mut b, "SELECT count(*) AS n FROM vault_revocations"), 1);
}

// ---------------------------------------------------------------------------
// The clock
// ---------------------------------------------------------------------------

fn wall_ms() -> i64 {
    i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()).unwrap()
}

#[test]
fn a_peer_with_a_clock_years_ahead_cannot_drag_ours_along() {
    let (mut a, mut b) = (device(), device());
    memory(&mut a, MemoryId::new(), "fact");
    let mut batch = engine::export(&mut a.conn, 0, &b.id, 1000, 1 << 20).unwrap();
    for c in &mut batch.changes {
        c.hlc = "9999999999999.000000".into();
    }
    engine::apply(&mut b.conn, &a.id, &batch).unwrap();
    let ms: i64 = sync_state::table.select(sync_state::hlc_ms).first(&mut b.conn).unwrap();
    assert!(ms <= wall_ms() + engine::MAX_CLOCK_LEAD_MS + 1000, "clock dragged to {ms}");
}

#[test]
fn the_clock_rolls_into_the_next_millisecond_rather_than_overflow() {
    let mut a = device();
    let ahead = wall_ms() + 3_600_000;
    exec(&mut a, &format!("UPDATE sync_state SET hlc_ms = {ahead}, hlc_counter = 999999"));
    let id = MemoryId::new();
    memory(&mut a, id, "fact");
    let hlc: String = sync_rows::table.find(("memories", id.to_string())).select(sync_rows::hlc).first(&mut a.conn).unwrap();
    assert_eq!(hlc, format!("{:013}.000000", ahead + 1));
}

#[test]
fn malformed_clock_values_are_refused() {
    let (mut a, mut b) = (device(), device());
    memory(&mut a, MemoryId::new(), "fact");
    for bad in ["1.2", "99999999999999.000000", "0000000000001.0000001", "000000000000a.000000"] {
        let mut batch = engine::export(&mut a.conn, 0, &b.id, 1000, 1 << 20).unwrap();
        batch.changes[0].hlc = bad.into();
        assert!(engine::apply(&mut b.conn, &a.id, &batch).is_err(), "{bad} accepted");
    }
}

#[test]
fn an_update_that_changes_nothing_is_not_a_new_version() {
    let mut a = device();
    let id = MemoryId::new();
    memory(&mut a, id, "fact");
    let version = |a: &mut Device| -> (String, i64) {
        sync_rows::table.find(("memories", id.to_string())).select((sync_rows::hlc, sync_rows::seq)).first(&mut a.conn).unwrap()
    };
    let before = version(&mut a);
    exec(&mut a, &format!("UPDATE memories SET content = 'fact', category = category WHERE id = '{id}'"));
    assert_eq!(version(&mut a), before);
    exec(&mut a, &format!("UPDATE memories SET content = 'other' WHERE id = '{id}'"));
    assert!(version(&mut a).1 > before.1);
}

#[test]
fn a_changed_key_is_a_new_row_and_a_deletion_of_the_old() {
    let mut a = device();
    let (old, new) = (MemoryId::new(), MemoryId::new());
    memory(&mut a, old, "fact");
    exec(&mut a, &format!("UPDATE memories SET id = '{new}' WHERE id = '{old}'"));
    let deleted = |a: &mut Device, id: MemoryId| -> i64 {
        sync_rows::table.find(("memories", id.to_string())).select(sync_rows::deleted).first(&mut a.conn).unwrap()
    };
    assert_eq!(deleted(&mut a, old), 1);
    assert_eq!(deleted(&mut a, new), 0);
}

// ---------------------------------------------------------------------------
// The triggers
// ---------------------------------------------------------------------------

#[test]
fn every_synced_table_has_triggers_listing_all_its_columns() {
    let mut a = device();
    let tables = super::triggers::synced_table_names(&mut a.conn).unwrap();
    for local in super::triggers::LOCAL_TABLES {
        assert!(!tables.iter().any(|t| t == local), "{local} is local");
    }
    for expected in ["courses", "lessons", "words", "day_log", "settings", "files", "reviews", "vault_devices", "vault_revocations"] {
        assert!(tables.iter().any(|t| t == expected), "{expected} syncs");
    }
    let synced = engine::synced_tables(&mut a.conn).unwrap();
    for table in &tables {
        assert!(synced.contains_key(table), "{table} has no sync triggers");
        let shape = super::triggers::shape(&mut a.conn, table).unwrap();
        let triggers = super::triggers::existing_triggers(&mut a.conn, table).unwrap();
        assert_eq!(triggers.len(), 4, "{table}: {:?}", triggers.keys());
        let update = &triggers[&format!("sync_{table}_au")];
        for column in &shape.columns {
            assert!(update.contains(&format!("OLD.\"{column}\" IS NOT NEW.\"{column}\"")), "{table}.{column} is not watched");
        }
    }
    // A column added later: the next open re-makes the table's triggers.
    exec(&mut a, "ALTER TABLE memories ADD COLUMN mood TEXT");
    super::init(&mut a.conn).unwrap();
    let update = &super::triggers::existing_triggers(&mut a.conn, "memories").unwrap()["sync_memories_au"];
    assert!(update.contains("OLD.\"mood\" IS NOT NEW.\"mood\""));
}

#[test]
fn natural_keys_are_their_columns_joined() {
    let mut a = device();
    crate::stats::add_xp(&mut a.conn, 20_000, 5).unwrap();
    let key = format!("20000\u{1f}{}", a.id);
    assert_eq!(sync_rows::table.find(("day_log", &key)).count().get_result::<i64>(&mut a.conn).unwrap(), 1);
    let me = crate::sync::device_id(&mut a.conn).unwrap();
    assert!(day_log::table.find((20_000, me)).count().get_result::<i64>(&mut a.conn).unwrap() == 1);
}
