//! A sync run with the app closed (as Tally's): Android's background job
//! (the sync-background plugin's WorkManager worker, every half hour on
//! unmetered Wi-Fi) calls [`run_blocking`] through JNI. There is no app to
//! borrow a connection from, so it opens the database itself — lightly, not
//! the app's whole start — syncs once with whichever of the vault's devices
//! answer, and returns. It emits no event: the app, when it comes back to
//! the front, reloads what it shows (src/lib/sync.ts).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;

use super::instance;
use crate::db::{self, Db};
use crate::error::Result;

/// WorkManager gives a worker ten minutes; this leaves room to close cleanly.
pub const BUDGET: Duration = Duration::from_secs(8 * 60);

// `SyncNative.syncNow(dbPath)` in Kotlin: an exported JNI symbol, which needs
// an attribute this crate forbids itself; the plugin's macro writes it.
tauri_plugin_sync_background::jni_entry!(crate::sync::background::run_blocking);

/// One background run for the database at `db_path`, on a runtime of its
/// own. Returns a line for Android's log.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn run_blocking(db_path: &str) -> String {
    match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(runtime) => runtime.block_on(run_once(Path::new(db_path), BUDGET)),
        Err(e) => format!("couldn't start: {e}"),
    }
}

/// How long a statement waits for the database while the app (in this same
/// process, when it's open) is writing, before giving up. Longer than the
/// app's own: the background has nobody waiting on it.
const BUSY_TIMEOUT_MS: u32 = 15_000;

/// Opens the database for a background run (a sync, or a course kept
/// offline finishing its download: offline/background.rs): a plain connection with the
/// app's settings, and nothing else. Not `db::open`, which is the app's
/// start — upgrade, courses, clean-ups, an integrity check, timings in the
/// log — far too much to do every half hour, and all of it writes.
pub(crate) fn open_light(path: &Path) -> Result<db::Conn> {
    let mut conn = db::Conn::establish(&path.to_string_lossy())?;
    conn.batch_execute(&format!(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = {BUSY_TIMEOUT_MS};
         PRAGMA journal_size_limit = 67108864;"
    ))?;
    Ok(conn)
}

#[derive(QueryableByName)]
struct Found {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    n: i64,
}

/// Whether there's anything for a background run to do here: sync's tables
/// exist and this device is in a vault with another device in it. Only
/// reads.
fn worth_a_run(conn: &mut db::Conn) -> Result<bool> {
    let tables = diesel::sql_query("SELECT count(*) AS n FROM sqlite_master WHERE type = 'table' AND name = 'sync_state'")
        .get_result::<Found>(conn)?
        .n;
    Ok(tables > 0 && super::service::paired(conn)?)
}

/// One round against the database at `db_path`, given at most `budget`.
///
/// The app may be open in this same process (Android runs the job in the
/// app's process when it's alive), on a connection of its own. They don't
/// fight: both wait for each other's writes (`busy_timeout`) rather than
/// fail, sync writes only one batch per transaction and never holds one
/// across the network (`sync::with`), and only one sync runs per device at
/// a time (`session::try_busy`) — the app's own round, or an incoming one,
/// is turned away as "busy" while this one runs, and the reverse.
pub async fn run_once(db_path: &Path, budget: Duration) -> String {
    if let Some(dir) = db_path.parent() {
        // The app's own log (Settings → Diagnostics), when the app isn't
        // running in this process to have set it already.
        crate::diagnostics::init(dir.to_path_buf());
    }
    // Only a database this build's app has already upgraded: an upgrade
    // takes a backup and can take a while, and is the app's to do, with the
    // learner looking. Peers only sync at the same schema anyway.
    match db::is_current(db_path) {
        Ok(true) => {}
        Ok(false) => return "the app hasn't opened its database since it was updated; skipped".to_string(),
        Err(e) => return format!("couldn't read the database: {e}"),
    }
    let path = db_path.to_path_buf();
    let opened = tokio::task::spawn_blocking(move || -> Result<Option<db::Conn>> {
        let mut conn = open_light(&path)?;
        // Not in a vault, or alone in it: nothing to sync with, and nothing
        // is written — not even the instance check's marker.
        if !worth_a_run(&mut conn)? {
            return Ok(None);
        }
        // Before anything is written: a copy of the database from elsewhere
        // takes a new device id (instance.rs), as when the app opens it.
        if let instance::Check::Rotated { old_id, new_id } = instance::check(&mut conn, &path)? {
            crate::diagnostics::note("sync", format!("This database was copied from elsewhere: the device {old_id} is now {new_id}."));
        }
        Ok(Some(conn))
    })
    .await;
    let conn = match opened {
        Ok(Ok(Some(conn))) => conn,
        Ok(Ok(None)) => return "not in a vault with another device; skipped".to_string(),
        Ok(Err(e)) => return format!("couldn't open the database: {e}"),
        Err(e) => return format!("couldn't open the database: {e}"),
    };
    let store = Arc::new(Db::new(conn));
    match tokio::time::timeout(budget, super::service::sync_round(&store)).await {
        Err(_) => "ran out of time; carries on next run".to_string(),
        Ok(round) => {
            let synced: Vec<String> = round
                .outcomes
                .iter()
                .map(|o| {
                    let state = if o.complete { "complete" } else { "incomplete" };
                    format!("{} ({} in, {state})", o.peer_name, o.received.applied)
                })
                .collect();
            match (synced.is_empty(), round.errors.first()) {
                (_, Some(e)) => {
                    crate::diagnostics::note("sync", format!("A background sync failed: {e}"));
                    format!("error: {e}")
                }
                (true, None) => "no device of the vault around".to_string(),
                (false, None) => format!("synced with {}", synced.join(", ")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_database_the_app_has_not_upgraded_is_left_alone() {
        let dir = std::env::temp_dir().join("learn-polish-tests").join(format!("background-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        let mut conn = db::Conn::establish(&path.to_string_lossy()).unwrap();
        conn.batch_execute("CREATE TABLE words (id INTEGER PRIMARY KEY); PRAGMA user_version = 7;").unwrap();
        drop(conn);
        let summary = run_once(&path, Duration::from_secs(5)).await;
        assert!(summary.contains("skipped"), "{summary}");
        // Untouched: still the old version, no sync tables.
        let mut conn = db::Conn::establish(&path.to_string_lossy()).unwrap();
        assert!(!db::is_current(&path).unwrap());
        assert!(conn.batch_execute("SELECT 1 FROM sync_state").is_err());
    }

    #[tokio::test]
    async fn no_database_is_not_made() {
        let path = std::env::temp_dir().join("learn-polish-tests").join(format!("missing-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let summary = run_once(&path, Duration::from_secs(5)).await;
        assert!(summary.contains("skipped"), "{summary}");
        assert!(!path.exists());
    }

    /// A device on its own, or in a vault of one (it showed a code nobody
    /// scanned): the run returns at once and writes nothing — not even the
    /// instance check's file, which would otherwise be written back.
    #[tokio::test]
    async fn a_device_without_another_in_its_vault_is_left_alone() {
        let dir = std::env::temp_dir().join("learn-polish-tests").join(format!("background-alone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("learn-polish.db");
        let mut conn = db::open(&path).unwrap();
        let id = super::super::device_id(&mut conn).unwrap();
        let seq = super::super::engine::local_state(&mut conn).unwrap().seq;
        drop(conn);
        // Gone, as if the database had been carried here: the check would
        // give it a new id and write the file again, if it ran.
        std::fs::remove_file(instance::sidecar(&path)).unwrap();

        let summary = run_once(&path, Duration::from_secs(5)).await;
        assert!(summary.contains("not in a vault"), "{summary}");
        assert!(!instance::sidecar(&path).exists());

        // In a vault of its own: still nothing to do.
        let mut conn = open_light(&path).unwrap();
        super::super::session::ensure_vault(&mut conn).unwrap();
        let seq_in_vault = super::super::engine::local_state(&mut conn).unwrap().seq;
        assert!(seq_in_vault > seq);
        drop(conn);
        let summary = run_once(&path, Duration::from_secs(5)).await;
        assert!(summary.contains("not in a vault"), "{summary}");
        assert!(!instance::sidecar(&path).exists());

        let mut conn = open_light(&path).unwrap();
        assert_eq!(super::super::device_id(&mut conn).unwrap(), id);
        assert_eq!(super::super::engine::local_state(&mut conn).unwrap().seq, seq_in_vault, "nothing written");
    }
}
