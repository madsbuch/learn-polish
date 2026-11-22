//! A course's download finished with the app closed: Android's job (the
//! sync-background plugin's OfflineWorker) calls [`run_blocking`] through
//! JNI when the app was stopped before a download ended, and polls
//! [`progress_now`] to show how far it has come.
//!
//! It opens the database as sync's background run does (sync/background.rs:
//! only one the app has upgraded, a plain connection, no upgrade or
//! clean-ups), and downloads what the courses kept offline still lack in
//! example sentences and audio, one course at a time; pictures are left to
//! the learner asking, as ever. The app may be open in this same process:
//! a course is never downloaded by both (`claim`), and while the app
//! downloads anything the job only asks to be run again later.

use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;

use super::{Step, Tally};
use crate::db::{self, Conn, Db};
use crate::error::Result;
use crate::ids::CourseId;

// `OfflineNative.downloadNow(dbPath)` and `OfflineNative.progress()` in
// Kotlin: exported JNI symbols, which need an attribute this crate forbids
// itself; the plugin's macro writes them.
tauri_plugin_sync_background::offline_jni_entry!(
    crate::offline::background::run_blocking,
    crate::offline::background::progress_now
);

/// How the run went, for the job: a line for Android's log, whether to come
/// back later, what to tell the learner, and the page a tap on that opens
/// (the course, when it is about one).
#[derive(Debug, Serialize, PartialEq)]
pub struct Outcome {
    log: String,
    retry: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    address: Option<String>,
}

impl Outcome {
    fn said(log: impl Into<String>) -> Self {
        Self { log: log.into(), retry: false, result: None, address: None }
    }
}

/// How far the download in the background has come, for its notification,
/// and the page a tap on it opens (the course downloading).
#[derive(Debug, Clone, Serialize, PartialEq)]
struct Shown {
    text: String,
    done: i64,
    total: i64,
    address: String,
}

static SHOWN: Mutex<Option<Shown>> = Mutex::new(None);

fn show(shown: Option<Shown>) {
    *SHOWN.lock().unwrap_or_else(|e| e.into_inner()) = shown;
}

/// The download's progress as JSON, or nothing before one starts.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn progress_now() -> String {
    SHOWN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|shown| serde_json::to_string(shown).ok()).unwrap_or_default()
}

/// One run for the database at `db_path`, on a runtime of its own. Returns
/// its [`Outcome`] as JSON.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn run_blocking(db_path: &str) -> String {
    let outcome = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(runtime) => runtime.block_on(run_once(Path::new(db_path))),
        Err(e) => Outcome::said(format!("couldn't start: {e}")),
    };
    show(None);
    serde_json::to_string(&outcome).unwrap_or_default()
}

/// What a run does.
#[derive(Debug, PartialEq)]
enum Plan {
    /// Nothing, and why.
    Skip(&'static str),
    /// The app is downloading in this process: later.
    Wait,
    /// These courses, one at a time.
    Download(Vec<CourseId>),
}

/// The rule: with a key, the courses kept offline that still lack example
/// sentences or audio and whose download has not run to its end in this
/// process — but nothing while the app downloads a course itself.
fn plan(conn: &mut Conn) -> Result<Plan> {
    if crate::ai::api_key(conn).is_err() {
        return Ok(Plan::Skip("no OpenRouter key; skipped"));
    }
    let mut pending = Vec::new();
    for course_id in super::kept(conn)? {
        if !super::finished(course_id) && super::needs_fetching(conn, course_id) {
            pending.push(course_id);
        }
    }
    if pending.is_empty() {
        return Ok(Plan::Skip("nothing left to download"));
    }
    if super::any_working() {
        return Ok(Plan::Wait);
    }
    Ok(Plan::Download(pending))
}

/// Tells the app's screens, when the app is open in this process.
fn changed(course_id: CourseId) {
    if let Some(app) = crate::work::app() {
        use tauri::Emitter;
        let _ = app.emit("offline://changed", super::OfflineChanged { course_id });
    }
}

/// One run against the database at `db_path`.
pub async fn run_once(db_path: &Path) -> Outcome {
    if let Some(dir) = db_path.parent() {
        crate::diagnostics::init(dir.to_path_buf());
    }
    // Only a database this build's app has already upgraded: the upgrade is
    // the app's to do (sync/background.rs).
    match db::is_current(db_path) {
        Ok(true) => {}
        Ok(false) => return Outcome::said("the app hasn't opened its database since it was updated; skipped"),
        Err(e) => return Outcome::said(format!("couldn't read the database: {e}")),
    }
    let path = db_path.to_path_buf();
    let store = match tokio::task::spawn_blocking(move || crate::sync::background::open_light(&path)).await {
        Ok(Ok(conn)) => Db::new(conn),
        Ok(Err(e)) => return Outcome::said(format!("couldn't open the database: {e}")),
        Err(e) => return Outcome::said(format!("couldn't open the database: {e}")),
    };
    let courses = match plan(&mut store.lock()) {
        Ok(Plan::Skip(why)) => return Outcome::said(why),
        Ok(Plan::Wait) => return Outcome { retry: true, ..Outcome::said("the app is downloading; later") },
        Ok(Plan::Download(courses)) => courses,
        Err(e) => return Outcome::said(format!("couldn't read the courses: {e}")),
    };
    let mut results = Vec::new();
    let mut about = Vec::new();
    for course_id in courses {
        // The app took it meanwhile.
        if !super::claim(course_id) {
            continue;
        }
        let title = super::title(&mut store.lock(), course_id);
        changed(course_id);
        let tally = Tally::default();
        // Tapped, its notification opens the course.
        // Spec: SHELL-20
        let told = |step: Step| {
            show(Some(Shown {
                text: format!("{title} · {} of {}", step.finished(), step.total),
                done: step.finished(),
                total: step.total,
                address: super::address(course_id),
            }))
        };
        super::fetch(&store, course_id, &tally, &told).await;
        super::release(course_id);
        let ready = super::status(&mut store.lock(), course_id).is_ok_and(|s| s.ready);
        if let Some(line) = super::result_line(&title, ready, tally.sum()) {
            results.push(line);
            about.push(course_id);
        }
        changed(course_id);
    }
    Outcome {
        log: format!("downloaded; {}", if results.is_empty() { "nothing to say".to_string() } else { results.join(" ") }),
        // The app began a download meanwhile: stay, in case it is stopped.
        retry: super::any_working(),
        result: (!results.is_empty()).then(|| results.join(" ")),
        // Tapped, it opens the course it is about; about several, none.
        // Spec: SHELL-20
        address: match about[..] {
            [one] => Some(super::address(one)),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::connection::SimpleConnection;
    use diesel::Connection;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("learn-polish-tests").join(format!("offline-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("learn-polish.db")
    }

    #[tokio::test]
    async fn a_database_the_app_has_not_upgraded_is_left_alone() {
        let path = scratch("old");
        let mut conn = Conn::establish(&path.to_string_lossy()).unwrap();
        conn.batch_execute("CREATE TABLE words (id INTEGER PRIMARY KEY); PRAGMA user_version = 7;").unwrap();
        drop(conn);
        let outcome = run_once(&path).await;
        assert!(outcome.log.contains("skipped") && !outcome.retry, "{outcome:?}");
        assert!(!db::is_current(&path).unwrap());
    }

    #[tokio::test]
    async fn no_database_is_not_made() {
        let path = scratch("missing");
        let outcome = run_once(&path).await;
        assert!(outcome.log.contains("skipped") && !outcome.retry, "{outcome:?}");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn without_a_key_nothing_is_downloaded() {
        let path = scratch("no-key");
        let mut conn = db::open(&path).unwrap();
        let course = db::require_course_id(&mut conn, 1).unwrap();
        super::super::set(&mut conn, course, true).unwrap();
        drop(conn);
        let outcome = run_once(&path).await;
        assert_eq!(outcome, Outcome::said("no OpenRouter key; skipped"));
    }

    /// The rule, and the app and the job never downloading one course at
    /// once. One test: the courses being downloaded are the process's.
    #[test]
    fn what_is_carried_on_with() {
        let path = scratch("plan");
        let mut conn = db::open(&path).unwrap();
        db::set_setting(&mut conn, "openrouter_api_key", "sk-test").unwrap();
        assert_eq!(plan(&mut conn).unwrap(), Plan::Skip("nothing left to download"), "no course kept offline");

        let course = db::require_course_id(&mut conn, 1).unwrap();
        super::super::set(&mut conn, course, true).unwrap();
        assert_eq!(plan(&mut conn).unwrap(), Plan::Download(vec![course]), "its audio is missing");

        // The app downloads it, in this process: the job waits.
        assert!(super::super::claim(course));
        assert!(!super::super::claim(course), "one download of a course at a time");
        assert_eq!(plan(&mut conn).unwrap(), Plan::Wait);

        // It ran to its end, failures and all: left be in this process.
        super::super::release(course);
        assert_eq!(plan(&mut conn).unwrap(), Plan::Skip("nothing left to download"));
        // A new process (the app was stopped mid-download) carries on.
        super::super::FINISHED.lock().unwrap().clear();
        assert_eq!(plan(&mut conn).unwrap(), Plan::Download(vec![course]));

        // Switched off: nothing.
        super::super::set(&mut conn, course, false).unwrap();
        assert_eq!(plan(&mut conn).unwrap(), Plan::Skip("nothing left to download"));
    }

    // Spec: SHELL-20
    #[test]
    fn progress_is_shown_once_a_download_starts() {
        show(None);
        assert_eq!(progress_now(), "");
        show(Some(Shown { text: "Pierwsze kroki · 34 of 120".into(), done: 34, total: 120, address: "/crs_1".into() }));
        assert_eq!(progress_now(), r#"{"text":"Pierwsze kroki · 34 of 120","done":34,"total":120,"address":"/crs_1"}"#);
        show(None);
        let outcome = Outcome { log: "x".into(), retry: true, result: None, address: None };
        assert_eq!(serde_json::to_string(&outcome).unwrap(), r#"{"log":"x","retry":true}"#);
        let outcome = Outcome { result: Some("Ready".into()), address: Some("/crs_1".into()), ..Outcome::said("x") };
        assert_eq!(serde_json::to_string(&outcome).unwrap(), r#"{"log":"x","retry":false,"result":"Ready","address":"/crs_1"}"#);
    }
}
