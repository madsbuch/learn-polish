//! Pairing and syncing over real TCP connections on localhost, every device
//! a real database (the app's schema, demo course, triggers and all) in this
//! process — Tally's session tests, and its reviews' regression tests for
//! restored copies, ported.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use diesel::connection::SimpleConnection;
use diesel::sql_types::Text;

use super::*;
use crate::db::Db;
use crate::ids::{CourseId, LessonId, MemoryId, WordId};
use crate::sync::instance;
use crate::sync::tests::dump;

static DEVICES: AtomicUsize = AtomicUsize::new(0);

/// A device: its database, opened as the app opens it, and where it lives.
struct Dev {
    db: Arc<Db>,
    path: PathBuf,
}

fn device() -> Dev {
    let n = DEVICES.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join("learn-polish-tests")
        .join(format!("session-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("learn-polish.db");
    let conn = db::open(&path).unwrap();
    Dev { db: Arc::new(Db::new(conn)), path }
}

/// Opens a database file without the instance check `db::open` runs: a
/// copy nothing looked at (the worst case).
fn open_unchecked(path: &Path) -> Arc<Db> {
    let mut conn = Conn::establish(&path.to_string_lossy()).unwrap();
    conn.batch_execute("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;").unwrap();
    Arc::new(Db::new(conn))
}

fn exec(db: &Db, sql: &str) {
    db.lock().batch_execute(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
}

fn count(db: &Db, sql: &str) -> i64 {
    #[derive(QueryableByName)]
    struct N {
        #[diesel(sql_type = BigInt)]
        n: i64,
    }
    diesel::sql_query(sql).get_result::<N>(&mut *db.lock()).unwrap_or_else(|e| panic!("{sql}: {e}")).n
}

fn me(db: &Db) -> Identity {
    identity(&mut db.lock()).unwrap()
}

struct Host {
    dev: Dev,
    slot: PairingSlot,
    addr: SocketAddr,
    served: Arc<AtomicUsize>,
    errors: Arc<Mutex<Vec<String>>>,
}

async fn host() -> Host {
    let dev = device();
    let slot: PairingSlot = Arc::new(Mutex::new(None));
    let served = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let (s, e) = (served.clone(), errors.clone());
    let (port, _) = listen(dev.db.clone(), slot.clone(), "127.0.0.1", 0, move |r| match r {
        Ok(_) => {
            s.fetch_add(1, Ordering::SeqCst);
        }
        Err(err) => e.lock().unwrap().push(err.to_string()),
    })
    .await
    .unwrap();
    Host { dev, slot, addr: format!("127.0.0.1:{port}").parse().unwrap(), served, errors }
}

/// Syncs, waiting out a host that's still finishing the previous sync.
async fn sync_now(db: &Arc<Db>, addr: SocketAddr) -> Result<Outcome> {
    for _ in 0..100 {
        if let Some(o) = sync_with(db, addr).await? {
            return Ok(o);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err(Error::msg("host stayed busy"))
}

/// Syncs until a sync ends with both identical (a merged version may need
/// a second one to travel back).
async fn sync_until_complete(db: &Arc<Db>, addr: SocketAddr) -> Outcome {
    let mut last = None;
    for _ in 0..4 {
        let o = sync_now(db, addr).await.unwrap();
        if o.complete {
            return o;
        }
        last = Some(o);
    }
    panic!("never complete: {last:?}");
}

async fn offer_from(h: &Host) -> PairingOffer {
    offer(&h.dev.db, &h.slot, vec![h.addr.to_string()]).await.unwrap()
}

/// Waits for the host's listener (on its own task) to report an error.
async fn host_error(h: &Host, containing: &str) -> bool {
    for _ in 0..100 {
        if h.errors.lock().unwrap().iter().any(|e| e.contains(containing)) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

fn memory(db: &Db, content: &str) {
    exec(db, &format!("INSERT INTO memories (id, content, created_at, updated_at) VALUES ('{}', '{content}', 1, 1)", MemoryId::new()));
}

fn with_some_data(db: &Db) {
    for i in 0..25 {
        memory(db, &format!("likes cats {i}"));
    }
    db::set_setting(&mut db.lock(), "openrouter_api_key", "sk-live").unwrap();
    let picture: Vec<u8> = (0..2_000_000u32).map(|i| u8::try_from(i * 13 % 256).unwrap()).collect();
    crate::files::store(&mut db.lock(), crate::files::COURSE_IMAGE, "c.jpg", "image/jpeg", &picture).unwrap();
}

/// Both devices' data, compared table by table (versions included).
fn assert_same(a: &Db, b: &Db) {
    let (x, y) = (dump(&mut a.lock()), dump(&mut b.lock()));
    for (table, rows) in &x {
        assert_eq!(rows, &y[table], "{table} differs");
    }
}

fn copy_to(db: &Db, to: &Path) {
    let _ = std::fs::remove_file(to);
    diesel::sql_query("VACUUM INTO ?").bind::<Text, _>(to.to_string_lossy().to_string()).execute(&mut *db.lock()).unwrap();
}

#[tokio::test]
async fn a_new_device_joins_with_the_code_and_gets_everything() {
    let h = host().await;
    with_some_data(&h.dev.db);
    let o = offer_from(&h).await;
    let joiner = device();
    let old_id = me(&joiner.db).device_id;

    let outcome = join(&joiner.db, &PairingOffer::decode(&o.encode().unwrap()).unwrap(), false).await.unwrap();
    assert!(outcome.complete, "{:?}", outcome.differences);
    assert!(outcome.received.applied >= 27, "{:?}", outcome.received);
    assert_same(&h.dev.db, &joiner.db);
    // Both know both, by the keys they proved.
    assert_eq!(count(&h.dev.db, "SELECT count(*) AS n FROM vault_devices"), 2);
    assert_eq!(count(&joiner.db, "SELECT count(*) AS n FROM vault_devices"), 2);
    assert_ne!(me(&joiner.db).device_id, old_id, "a joining device takes a new id");
    assert_eq!(db::get_setting(&mut joiner.db.lock(), "openrouter_api_key").unwrap().as_deref(), Some("sk-live"));

    // Afterwards, plain syncs both ways.
    memory(&h.dev.db, "from the host");
    memory(&joiner.db, "from the joiner");
    let outcome = sync_now(&joiner.db, h.addr).await.unwrap();
    assert!(outcome.complete);
    assert_same(&h.dev.db, &joiner.db);
    assert_eq!(count(&h.dev.db, "SELECT count(*) AS n FROM memories WHERE content = 'from the joiner'"), 1);
    assert_eq!(count(&joiner.db, "SELECT count(*) AS n FROM memories WHERE content = 'from the host'"), 1);

    // A sync with nothing new moves nothing.
    let outcome = sync_now(&joiner.db, h.addr).await.unwrap();
    assert!(!outcome.received.changed_anything());
    assert!(outcome.complete);
}

#[tokio::test]
async fn two_fresh_installs_paired_converge_after_each_adds_a_course_and_practises() {
    let h = host().await;
    let joiner = device();
    join(&joiner.db, &offer_from(&h).await, false).await.unwrap();

    for (i, db) in [&h.dev.db, &joiner.db].into_iter().enumerate() {
        let mut c = db.lock();
        // A course of its own, with a lesson and words…
        let (course, lesson) = (CourseId::new(), LessonId::new());
        c.batch_execute(&format!(
            "INSERT INTO courses (id, slug, ord, title_pl, title_en, summary_en, created_at) VALUES ('{course}', '{course}', {}, 'Kurs', 'Course {i}', '', 1);
             INSERT INTO lessons (id, lesson_type, title, complexity, period_days, created_at) VALUES ('{lesson}', 'words', 'Lesson {i}', 'low', 7, 1);
             INSERT INTO course_lessons (course_id, lesson_id, ord) VALUES ('{course}', '{lesson}', 0);
             INSERT INTO words (id, lesson_id, ord, pl, en, pos) VALUES ('{}', '{lesson}', 0, 'kot', 'cat', 'n'), ('{}', '{lesson}', 1, 'pies', 'dog', 'n');",
            100 + i,
            WordId::new(),
            WordId::new(),
        ))
        .unwrap();
        // …and practice: the demo course's first drill, and an answer.
        let demo: CourseId = crate::schema::courses::table
            .filter(crate::schema::courses::slug.eq(crate::demo_course::SLUG))
            .select(crate::schema::courses::id)
            .first(&mut *c)
            .unwrap();
        let drill = crate::lessons::for_course(&mut c, demo).unwrap().into_iter().next().unwrap();
        crate::practice::lesson_finish_in(&mut c, drill.id, 8 + i64::try_from(i).unwrap(), 10).unwrap();
        let today = db::today(&mut c);
        crate::stats::record_answer(&mut c, today, 10, true).unwrap();
    }
    let xp_before = crate::stats::total_xp(&mut h.dev.db.lock()).unwrap() + crate::stats::total_xp(&mut joiner.db.lock()).unwrap();

    let outcome = sync_until_complete(&joiner.db, h.addr).await;
    assert!(outcome.complete);
    let (a, b) = (engine::digest(&mut h.dev.db.lock()).unwrap(), engine::digest(&mut joiner.db.lock()).unwrap());
    assert_eq!(a, b, "the digests are equal");
    assert_same(&h.dev.db, &joiner.db);
    for db in [&h.dev.db, &joiner.db] {
        assert_eq!(count(db, "SELECT count(*) AS n FROM courses WHERE title_en LIKE 'Course %'"), 2);
        // Each device's day is its own row: the XP adds up, nothing is lost.
        assert_eq!(crate::stats::total_xp(&mut db.lock()).unwrap(), xp_before);
    }
}

#[tokio::test]
async fn a_round_finds_the_peer_where_it_was_last_reached() {
    let h = host().await;
    let joiner = device();
    join(&joiner.db, &offer_from(&h).await, false).await.unwrap();
    memory(&h.dev.db, "found in a round");
    let round = crate::sync::service::sync_round(&joiner.db).await;
    assert!(round.errors.is_empty(), "{:?}", round.errors);
    assert_eq!(round.outcomes.len(), 1);
    assert_eq!(count(&joiner.db, "SELECT count(*) AS n FROM memories WHERE content = 'found in a round'"), 1);
}

/// Android's background job: the phone's app is closed (its database opened
/// afresh by the run), and the computer is found where it was last reached.
#[tokio::test]
async fn a_background_run_syncs_with_a_device_that_answers() {
    let h = host().await;
    let phone = device();
    join(&phone.db, &offer_from(&h).await, false).await.unwrap();
    memory(&phone.db, "written before the app closed");
    let summary = crate::sync::background::run_once(&phone.path, Duration::from_secs(30)).await;
    assert!(summary.starts_with("synced with"), "{summary}");
    assert_eq!(count(&h.dev.db, "SELECT count(*) AS n FROM memories WHERE content = 'written before the app closed'"), 1);
}

#[tokio::test]
async fn a_code_works_once() {
    let h = host().await;
    let o = offer_from(&h).await;
    join(&device().db, &o, false).await.unwrap();
    let err = join(&device().db, &o, false).await.unwrap_err().to_string();
    assert!(err.contains("expired or was already used"), "{err}");
    assert_eq!(count(&h.dev.db, "SELECT count(*) AS n FROM vault_devices"), 2);
}

#[tokio::test]
async fn an_expired_code_is_refused() {
    let h = host().await;
    let o = offer_from(&h).await;
    h.slot.lock().unwrap().as_mut().unwrap().expires = Instant::now() - Duration::from_secs(1);
    let joiner = device();
    let err = join(&joiner.db, &o, false).await.unwrap_err().to_string();
    assert!(err.contains("expired"), "{err}");
    // Nothing on the joiner was touched.
    assert_eq!(me(&joiner.db).vault_id, None);
}

#[tokio::test]
async fn a_forged_code_is_refused_before_anything_is_touched() {
    let h = host().await;
    let mut o = offer_from(&h).await;
    // Someone else's key: the device that answers isn't the one in the code.
    o.host_key = wire::generate_keypair().unwrap().1;
    let joiner = device();
    db::set_setting(&mut joiner.db.lock(), "daily_goal", "300").unwrap();
    let before = dump(&mut joiner.db.lock());
    let err = join(&joiner.db, &o, false).await.unwrap_err().to_string();
    assert!(err.contains("isn't the one that showed the code"), "{err}");
    assert_eq!(dump(&mut joiner.db.lock()), before);
    assert_eq!(me(&joiner.db).vault_id, None);

    // A code with the right key but a made-up token gets nowhere either.
    let mut o = offer_from(&h).await;
    o.token = [7; 32];
    let err = join(&joiner.db, &o, false).await.unwrap_err().to_string();
    assert!(err.contains("expired or was already used"), "{err}");
    assert_eq!(dump(&mut joiner.db.lock()), before);
}

#[tokio::test]
async fn a_device_with_data_cannot_join_and_keeps_its_data() {
    let h = host().await;
    let o = offer_from(&h).await;
    let joiner = device();
    memory(&joiner.db, "mine");
    let err = join(&joiner.db, &o, false).await.unwrap_err().to_string();
    assert!(err.contains("already holds data"), "{err}");
    assert_eq!(count(&joiner.db, "SELECT count(*) AS n FROM memories"), 1);
    // And the code is still good for an empty one.
    join(&device().db, &o, false).await.unwrap();
}

#[tokio::test]
async fn a_device_with_data_can_join_by_replacing_it_after_a_copy_is_saved() {
    let h = host().await;
    with_some_data(&h.dev.db);
    let o = offer_from(&h).await;
    let joiner = device();
    memory(&joiner.db, "mine");
    crate::stats::add_xp(&mut joiner.db.lock(), 20_000, 5).unwrap();
    let held = crate::sync::service::held(&mut joiner.db.lock()).unwrap();
    let tables: Vec<&str> = held.iter().map(|h| h.table.as_str()).collect();
    assert_eq!(tables, ["day_log", "memories"]);

    // Saved first…
    let path = joiner.path.clone();
    let backup = crate::sync::service::backup_before_join(&mut joiner.db.lock(), &path).unwrap().unwrap();
    // …then replaced by the vault's.
    let outcome = join(&joiner.db, &o, true).await.unwrap();
    assert!(outcome.complete, "{:?}", outcome.differences);
    assert_same(&h.dev.db, &joiner.db);
    assert_eq!(count(&joiner.db, "SELECT count(*) AS n FROM memories WHERE content = 'mine'"), 0);

    // The copy holds exactly what was there, and is marked as a copy.
    let copy = open_unchecked(Path::new(&backup));
    assert_eq!(count(&copy, "SELECT count(*) AS n FROM memories WHERE content = 'mine'"), 1);
    let marker = db::get_setting(&mut copy.lock(), instance::MARKER).unwrap().unwrap();
    assert!(marker.starts_with("copy:"), "{marker}");
    // An empty device needs no copy.
    assert_eq!(crate::sync::service::backup_before_join(&mut device().db.lock(), &path).unwrap(), None);
}

#[tokio::test]
async fn a_stranger_cannot_sync() {
    let h = host().await;
    with_some_data(&h.dev.db);
    offer_from(&h).await; // the host now has a vault
    let stranger = device();
    // A vault of its own, the same id even: still not on the host's list.
    let vault = me(&h.dev.db).vault_id.unwrap();
    exec(&stranger.db, &format!("UPDATE sync_state SET vault_id = '{vault}'"));
    ensure_vault(&mut stranger.db.lock()).unwrap();
    let before = dump(&mut h.dev.db.lock());
    assert!(sync_now(&stranger.db, h.addr).await.is_err());
    assert!(host_error(&h, "outside the vault").await, "{:?}", h.errors.lock().unwrap());
    assert_eq!(dump(&mut h.dev.db.lock()), before);
    assert_eq!(count(&stranger.db, "SELECT count(*) AS n FROM memories"), 0);
}

#[tokio::test]
async fn a_stranger_is_turned_away_before_it_can_send_much() {
    let h = host().await;
    offer_from(&h).await;
    // A device that isn't in the vault completes a handshake and announces a
    // huge message: it's dropped without the host reading or holding it.
    let (secret, _) = wire::generate_keypair().unwrap();
    let stream = TcpStream::connect(h.addr).await.unwrap();
    let mut ch = Channel::initiate(stream, PURPOSE_SYNC, &secret, None).await.unwrap();
    let _ = ch.send_bytes(&vec![0u8; 8 * 1024 * 1024]).await;
    assert!(host_error(&h, "outside the vault").await, "{:?}", h.errors.lock().unwrap());
}

#[tokio::test]
async fn a_removed_device_is_refused_from_then_on() {
    let h = host().await;
    let joiner = device();
    join(&joiner.db, &offer_from(&h).await, false).await.unwrap();
    let joiner_id = me(&joiner.db).device_id;
    exec(&h.dev.db, &format!("DELETE FROM vault_devices WHERE id = '{joiner_id}'"));
    memory(&h.dev.db, "secret");
    assert!(sync_now(&joiner.db, h.addr).await.is_err());
    assert_eq!(count(&joiner.db, "SELECT count(*) AS n FROM memories WHERE content = 'secret'"), 0);
}

#[tokio::test]
async fn a_revoked_device_is_refused_by_its_key_whatever_id_it_takes() {
    let h = host().await;
    let joiner = device();
    join(&joiner.db, &offer_from(&h).await, false).await.unwrap();
    let joiner_id = me(&joiner.db).device_id;
    crate::sync::service::remove_device(&mut h.dev.db.lock(), joiner_id).unwrap();
    assert_eq!(count(&h.dev.db, "SELECT count(*) AS n FROM vault_revocations"), 1);
    // A new id doesn't help: the key is what's revoked.
    instance::rotate(&mut joiner.db.lock()).unwrap();
    assert!(sync_now(&joiner.db, h.addr).await.is_err());
    assert_eq!(h.served.load(Ordering::SeqCst), 1, "only the join's sync went through");
    // And a device can't remove itself.
    let host_id = me(&h.dev.db).device_id;
    assert!(crate::sync::service::remove_device(&mut h.dev.db.lock(), host_id).is_err());
}

#[tokio::test]
async fn a_device_answering_at_a_spoofed_address_is_not_trusted() {
    // Our vault, and a host of another vault that someone points us at.
    let ours = host().await;
    let joiner = device();
    join(&joiner.db, &offer_from(&ours).await, false).await.unwrap();
    let impostor = host().await;
    offer_from(&impostor).await;
    let err = sync_now(&joiner.db, impostor.addr).await.unwrap_err().to_string();
    assert!(err.contains("outside the vault"), "{err}");
}

#[tokio::test]
async fn a_different_schema_or_build_is_refused() {
    let h = host().await;
    let joiner = device();
    join(&joiner.db, &offer_from(&h).await, false).await.unwrap();
    let host_key = me(&h.dev.db).public;
    let mine = {
        let mut c = joiner.db.lock();
        let me = identity(&mut c).unwrap();
        hello(&mut c, &me).unwrap()
    };
    let theirs = {
        let mut c = h.dev.db.lock();
        let me = identity(&mut c).unwrap();
        hello(&mut c, &me).unwrap()
    };
    assert!(check_peer(&mut joiner.db.lock(), &mine, &theirs, &host_key, None).is_ok());

    let newer = Hello { schema: theirs.schema + 1, ..theirs.clone() };
    let err = check_peer(&mut joiner.db.lock(), &mine, &newer, &host_key, None).unwrap_err().to_string();
    assert!(err.contains("different version of Polski"), "{err}");

    let release = Hello { build: "release".into(), ..theirs.clone() };
    let err = check_peer(&mut joiner.db.lock(), &mine, &release, &host_key, None).unwrap_err().to_string();
    assert!(err.contains("development builds never sync"), "{err}");

    let other_protocol = Hello { protocol: PROTOCOL + 1, ..theirs.clone() };
    assert!(check_peer(&mut joiner.db.lock(), &mine, &other_protocol, &host_key, None).is_err());
    let other_vault = Hello { vault_id: Some("elsewhere".into()), ..theirs };
    assert!(check_peer(&mut joiner.db.lock(), &mine, &other_vault, &host_key, None).is_err());
}

#[test]
fn development_and_release_builds_never_sync() {
    assert!(same_build("dev", "dev"));
    assert!(same_build("release", "release"));
    assert!(!same_build("dev", "release"));
    assert!(!same_build("release", ""));
    // These tests are a debug build.
    assert_eq!(build_kind(), "dev");
}

#[tokio::test]
async fn the_last_sync_is_remembered_per_peer() {
    let h = host().await;
    let joiner = device();
    join(&joiner.db, &offer_from(&h).await, false).await.unwrap();
    let host_id = me(&h.dev.db).device_id.to_string();
    let (complete, at, addr): (i64, Option<String>, Option<String>) = crate::schema::sync_peers::table
        .find(&host_id)
        .select((
            crate::schema::sync_peers::complete,
            crate::schema::sync_peers::last_complete_at,
            crate::schema::sync_peers::last_addr,
        ))
        .first(&mut *joiner.db.lock())
        .unwrap();
    assert_eq!(complete, 1);
    assert!(at.unwrap().ends_with('Z'));
    assert_eq!(addr.unwrap(), h.addr.to_string());
    // The screens see it too.
    let status = crate::sync::service::status(&mut joiner.db.lock(), false, Default::default()).unwrap();
    assert!(status.in_vault);
    assert_eq!(status.devices.len(), 2);
    let host = status.devices.iter().find(|d| !d.is_self).unwrap();
    assert!(host.complete && host.last_sync_at.is_some());
}

#[test]
fn pairing_codes_round_trip_and_garbage_is_rejected() {
    let o = PairingOffer {
        vault_id: "v".into(),
        host_id: "h".into(),
        host_name: "Phone".into(),
        host_key: vec![1; 32],
        token: [9; 32],
        addrs: vec!["192.168.1.20:47812".into()],
    };
    let code = o.encode().unwrap();
    assert!(code.starts_with("polski-pair:1:"), "{code}");
    assert_eq!(PairingOffer::decode(&code).unwrap(), o);
    assert!(PairingOffer::decode("hello").is_err());
    assert!(PairingOffer::decode("polski-pair:1:@@@").is_err());
    assert!(PairingOffer::decode(&code.replace("polski-pair:1:", "dansk-pair:1:")).is_err());
    let mut s = code;
    s.truncate(s.len() - 10);
    assert!(PairingOffer::decode(&s).is_err());
}

#[tokio::test]
async fn failed_connections_are_reported_by_the_listener() {
    let h = host().await;
    // Something that isn't Polski knocks on the port.
    let mut s = TcpStream::connect(h.addr).await.unwrap();
    tokio::io::AsyncWriteExt::write_all(&mut s, b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
    drop(s);
    assert!(host_error(&h, "").await);
    assert_eq!(h.served.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_busy_host_leaves_the_joiner_untouched_and_the_code_usable() {
    let h = host().await;
    let o = offer_from(&h).await;
    let host_id = me(&h.dev.db).device_id;
    let joiner = device();
    db::set_setting(&mut joiner.db.lock(), "daily_goal", "300").unwrap();
    {
        let _held = try_busy(&host_id).unwrap();
        let err = join(&joiner.db, &o, false).await.unwrap_err().to_string();
        assert!(err.contains("busy"), "{err}");
        // Nothing on the joiner changed.
        assert_eq!(me(&joiner.db).vault_id, None);
        assert_eq!(db::get_setting(&mut joiner.db.lock(), "daily_goal").unwrap().as_deref(), Some("300"));
    }
    // The code wasn't used up.
    join(&joiner.db, &o, false).await.unwrap();
}

#[tokio::test]
async fn a_failed_join_leaves_the_device_exactly_as_it_was() {
    let joiner = device();
    let before = me(&joiner.db);
    let o = PairingOffer {
        vault_id: "v".into(),
        host_id: "h".into(),
        host_name: "Phone".into(),
        host_key: vec![1; 32],
        token: [9; 32],
        // Nothing listens here.
        addrs: vec!["127.0.0.1:9".into()],
    };
    assert!(join(&joiner.db, &o, false).await.is_err());
    let after = me(&joiner.db);
    assert_eq!((before.public, before.device_id, before.vault_id), (after.public, after.device_id, after.vault_id));
}

#[tokio::test]
async fn an_idle_sync_skips_the_full_comparison_only_when_it_is_safe() {
    let h = host().await;
    let joiner = device();
    let first = join(&joiner.db, &offer_from(&h).await, false).await.unwrap();
    assert!(first.complete && !first.digest_skipped);

    // Nothing new on either side, after an identical sync: skipped, still complete.
    let idle = sync_now(&joiner.db, h.addr).await.unwrap();
    assert!(idle.digest_skipped && idle.complete);

    // Anything moving brings the comparison back.
    memory(&h.dev.db, "tea");
    let moved = sync_now(&joiner.db, h.addr).await.unwrap();
    assert!(!moved.digest_skipped && moved.complete);
    memory(&joiner.db, "coffee");
    let pushed = sync_now(&joiner.db, h.addr).await.unwrap();
    assert!(!pushed.digest_skipped && pushed.complete);
    assert_same(&h.dev.db, &joiner.db);

    // After a sync that ended different, an idle one still compares.
    let host_id = me(&h.dev.db).device_id;
    exec(&joiner.db, &format!("UPDATE sync_peers SET complete = 0 WHERE device_id = '{host_id}'"));
    let after_incomplete = sync_now(&joiner.db, h.addr).await.unwrap();
    assert!(!after_incomplete.digest_skipped && after_incomplete.complete);
}

// ---------------------------------------------------------------------------
// Restored copies (Tally's review tests)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_restored_copy_is_caught_at_the_start_of_a_sync_and_then_catches_up() {
    let h = host().await;
    let joiner = device();
    join(&joiner.db, &offer_from(&h).await, false).await.unwrap();
    // A copy of the phone's database, taken now…
    let copy = joiner.path.with_file_name("restored.db");
    copy_to(&joiner.db, &copy);
    // …then the phone adds more, and syncs it.
    for i in 0..4 {
        memory(&joiner.db, &format!("lost-{i}"));
    }
    sync_now(&joiner.db, h.addr).await.unwrap();
    // The phone is restored from the copy — and nothing checked the file
    // beside it (the worst case). It adds something.
    let restored = open_unchecked(&copy);
    let old_id = me(&restored).device_id;
    memory(&restored, "after-restore");

    // The first sync notices, takes a new id, and stops.
    let err = sync_now(&restored, h.addr).await.unwrap_err().to_string();
    assert!(err.contains("older copy"), "{err}");
    assert_ne!(me(&restored).device_id, old_id);
    // The next one goes through — the host knows it by its key — and
    // everything comes together.
    sync_until_complete(&restored, h.addr).await;
    assert_eq!(count(&restored, "SELECT count(*) AS n FROM memories WHERE content LIKE 'lost-%'"), 4);
    assert_eq!(count(&h.dev.db, "SELECT count(*) AS n FROM memories WHERE content = 'after-restore'"), 1);
    assert_same(&h.dev.db, &restored);
}

/// The app's data folder copied as a whole — database AND the marker file
/// beside it — as a file-level backup does. Returns the copy's path.
fn folder_copy(db: &Db, path: &Path) -> PathBuf {
    let dir = path.parent().unwrap().join("folder-backup");
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("learn-polish.db");
    copy_to(db, &target);
    std::fs::copy(instance::sidecar(path), instance::sidecar(&target)).unwrap();
    target
}

fn memory_content(db: &Db, id: MemoryId) -> String {
    crate::schema::memories::table
        .find(id)
        .select(crate::schema::memories::content)
        .first(&mut *db.lock())
        .unwrap()
}

#[tokio::test]
async fn an_edit_made_on_a_restored_folder_before_its_first_sync_still_converges() {
    let h = host().await;
    let phone = device();
    join(&phone.db, &offer_from(&h).await, false).await.unwrap();
    let x = MemoryId::new();
    exec(&phone.db, &format!("INSERT INTO memories (id, content, created_at, updated_at) VALUES ('{x}', 'Oats', 1, 1)"));
    sync_now(&phone.db, h.addr).await.unwrap();

    let backup = folder_copy(&phone.db, &phone.path);

    // The phone edits the memory and adds a little more; it all syncs.
    exec(&phone.db, &format!("UPDATE memories SET content = 'Oats with honey' WHERE id = '{x}'"));
    memory(&phone.db, "Tea");
    sync_now(&phone.db, h.addr).await.unwrap();

    // Restored from the folder backup. The marker check runs first, as in the app…
    let restored = open_unchecked(&backup);
    assert_eq!(instance::check(&mut restored.lock(), &backup).unwrap(), instance::Check::Same, "the marker can't tell");
    // …then, before the first sync, the learner corrects the same memory.
    exec(&restored, &format!("UPDATE memories SET content = 'Oats, half portion' WHERE id = '{x}'"));

    // The first sync: Seen catches the restore and rotates. Then sync until quiet.
    let first = sync_now(&restored, h.addr).await;
    assert!(first.as_ref().is_err_and(|e| e.to_string().contains("older copy")), "{first:?}");
    sync_until_complete(&restored, h.addr).await;
    assert_eq!(memory_content(&h.dev.db, x), memory_content(&restored, x));
    assert_same(&h.dev.db, &restored);
}

#[tokio::test]
async fn a_restored_folder_that_writes_a_lot_before_syncing_is_still_caught() {
    let h = host().await;
    let phone = device();
    join(&phone.db, &offer_from(&h).await, false).await.unwrap();
    sync_now(&phone.db, h.addr).await.unwrap();
    let backup = folder_copy(&phone.db, &phone.path);
    for i in 0..3 {
        memory(&phone.db, &format!("before-loss-{i}"));
    }
    sync_now(&phone.db, h.addr).await.unwrap();

    let restored = open_unchecked(&backup);
    assert_eq!(instance::check(&mut restored.lock(), &backup).unwrap(), instance::Check::Same);
    // A day offline: more writes than the phone made after the backup.
    for i in 0..10 {
        memory(&restored, &format!("offline-{i}"));
    }
    for _ in 0..3 {
        let _ = sync_now(&restored, h.addr).await;
    }
    assert_eq!(count(&h.dev.db, "SELECT count(*) AS n FROM memories WHERE content LIKE 'offline-%'"), 10);
    assert_eq!(count(&restored, "SELECT count(*) AS n FROM memories WHERE content LIKE 'before-loss-%'"), 3);
}

#[test]
fn renaming_this_device_renames_its_entry_in_the_vault() {
    let d = device();
    let mut c = d.db.lock();
    let me = ensure_vault(&mut c).unwrap();
    assert_eq!(me.name, "Computer");
    crate::sync::service::set_name(&mut c, "  Desk  ").unwrap();
    let name: String = crate::schema::vault_devices::table
        .find(me.device_id.to_string())
        .select(crate::schema::vault_devices::name)
        .first(&mut *c)
        .unwrap();
    assert_eq!(name, "Desk");
    assert_eq!(identity(&mut c).unwrap().name, "Desk");
    assert!(crate::sync::service::set_name(&mut c, " ").is_err());
}

/// A device's database as the app holds it: listening on `port`, which it
/// tells the devices it syncs with.
#[derive(Clone)]
struct Listening {
    db: Arc<Db>,
    port: u16,
}

impl Store for Listening {
    fn db(&self) -> &Db {
        &self.db
    }

    fn listen_port(&self) -> Option<u16> {
        Some(self.port)
    }
}

fn last_addr(of: &Db, peer: &Db) -> Option<String> {
    let id = me(peer).device_id.to_string();
    crate::schema::sync_peers::table
        .find(&id)
        .select(crate::schema::sync_peers::last_addr)
        .first::<Option<String>>(&mut *of.lock())
        .optional()
        .unwrap()
        .flatten()
}

/// Waits for the host's side of a sync to have recorded `peer`'s address:
/// it finishes on its own task, a moment after the client.
async fn recorded_addr(of: &Db, peer: &Db, expected: &str) -> Option<String> {
    for _ in 0..100 {
        let addr = last_addr(of, peer);
        if addr.as_deref() == Some(expected) {
            return addr;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    last_addr(of, peer)
}

/// The side that was connected to records where the other listens — the
/// address the connection came from, at the port it says — so a computer
/// learns where the phone is from the phone's own syncs, and reaches it
/// there itself without discovery.
#[tokio::test]
async fn the_side_connected_to_records_where_the_other_listens() {
    let h = host().await;
    let phone = device();
    let slot: PairingSlot = Arc::new(Mutex::new(None));
    let (port, _) = listen(phone.db.clone(), slot, "127.0.0.1", 0, |_| {}).await.unwrap();
    let listening = Listening { db: phone.db.clone(), port };
    join(&listening, &offer_from(&h).await, false).await.unwrap();
    let at = format!("127.0.0.1:{port}");
    assert_eq!(recorded_addr(&h.dev.db, &phone.db, &at).await.as_deref(), Some(at.as_str()), "recorded on joining");

    // An idle sync (no comparison) records it too.
    exec(&h.dev.db, "UPDATE sync_peers SET last_addr = '10.9.9.9:1'");
    let mut outcome = None;
    for _ in 0..100 {
        outcome = sync_with(&listening, h.addr).await.unwrap();
        if outcome.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(outcome.unwrap().digest_skipped);
    assert_eq!(recorded_addr(&h.dev.db, &phone.db, &at).await.as_deref(), Some(at.as_str()));

    // A device that doesn't listen (a background run) says no port: what
    // was recorded stays, rather than its connection's throwaway port.
    memory(&phone.db, "from the background");
    sync_now(&phone.db, h.addr).await.unwrap();
    // Once the host has finished with it (it's busy until then).
    sync_now(&phone.db, h.addr).await.unwrap();
    assert_eq!(last_addr(&h.dev.db, &phone.db).as_deref(), Some(at.as_str()));

    // And the host reaches the phone there by itself.
    memory(&h.dev.db, "for the phone");
    let mut reached = false;
    for _ in 0..100 {
        let round = crate::sync::service::sync_round(&h.dev.db).await;
        assert!(round.errors.is_empty(), "{:?}", round.errors);
        if !round.outcomes.is_empty() {
            reached = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(reached);
    assert_eq!(count(&phone.db, "SELECT count(*) AS n FROM memories WHERE content = 'for the phone'"), 1);
}
