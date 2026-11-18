//! A sync run with the app closed (as Tally's): Android's background job
//! (the sync-background plugin's WorkManager worker, every half hour on
//! unmetered Wi-Fi) calls [`run_blocking`] through JNI. There is no app to
//! borrow a connection from, so it opens the database itself, syncs once with
//! whichever of the vault's devices answer, and returns.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::db::{self, Db};

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

/// One round against the database at `db_path`, given at most `budget`.
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
    let conn = match tokio::task::spawn_blocking(move || db::open(&path)).await {
        Ok(Ok(conn)) => conn,
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
    use diesel::connection::SimpleConnection;
    use diesel::Connection;

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
}
