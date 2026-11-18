//! Diagnostics: a small log the learner can read in Settings and pass on —
//! how long slow screens took, model replies that could not be read,
//! background work that failed. Kept in a file in the app's data folder
//! (one JSON object per line), so anything can write to it without the
//! database, and it survives restarts. Only the newest entries are kept.
//!
//! On Android, plain stderr goes nowhere; this is where such notes belong.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// The log file, set once the app knows its data folder.
static PATH: OnceLock<PathBuf> = OnceLock::new();
/// One writer at a time.
static WRITING: Mutex<()> = Mutex::new(());

/// Entries kept: older ones are dropped when the file is trimmed.
const KEEP: usize = 300;
/// How much of a model's reply is kept with a note about it.
const EXCERPT: usize = 600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Unix seconds.
    pub at: i64,
    /// What it is about: "timing", "model reply", "background", "database".
    pub kind: String,
    pub message: String,
}

/// When the app started, for the startup timing note.
static STARTED: OnceLock<std::time::Instant> = OnceLock::new();

/// Marks the app's start (the first thing it does).
pub fn started() {
    let _ = STARTED.set(std::time::Instant::now());
}

/// Milliseconds since the app started.
pub fn since_start() -> u128 {
    STARTED.get().map_or(0, |t| t.elapsed().as_millis())
}

/// Where the log lives; called once at startup.
pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    let _ = PATH.set(dir.join("diagnostics.jsonl"));
}

/// Notes something. Never fails: a note that cannot be written is printed
/// instead (and lost on a phone).
pub fn note(kind: &str, message: impl Into<String>) {
    let entry = Entry { at: crate::db::now_secs(), kind: kind.into(), message: message.into() };
    eprintln!("{}: {}", entry.kind, entry.message);
    let Some(path) = PATH.get() else { return };
    let _lock = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(line) = serde_json::to_string(&entry) else { return };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
    trim(path);
}

/// A model reply that could not be read, with the start of it.
pub fn unreadable_reply(what: &str, reply: &str) {
    let excerpt: String = reply.chars().take(EXCERPT).collect();
    let more = if reply.chars().count() > EXCERPT { " …" } else { "" };
    note("model reply", format!("{what} could not be read. The reply began: {excerpt}{more}"));
}

/// Keeps the file to the newest entries, once it has grown well past them.
fn trim(path: &PathBuf) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= KEEP * 2 {
        return;
    }
    let kept = lines[lines.len() - KEEP..].join("\n") + "\n";
    let _ = std::fs::write(path, kept);
}

/// The entries, newest first.
pub fn entries() -> Vec<Entry> {
    let Some(path) = PATH.get() else { return Vec::new() };
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut list: Vec<Entry> = text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    list.reverse();
    list.truncate(KEEP);
    list
}

/// Notes an error the screens met: a command that failed, an answer of the
/// wrong shape, an uncaught exception. Messages are trimmed.
#[tauri::command]
pub fn diagnostics_note(kind: String, message: String) {
    let kind: String = kind.chars().take(40).collect();
    note(if kind.is_empty() { "error" } else { &kind }, message.chars().take(EXCERPT * 4).collect::<String>());
}

/// Every panic goes in the log too (with where it happened), then on to the
/// default handler.
pub fn note_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic".into());
        note("panic", format!("{message}{place}"));
        default(info);
    }));
}

#[tauri::command]
pub fn diagnostics_list() -> Vec<Entry> {
    entries()
}

#[tauri::command]
pub fn diagnostics_clear() -> Result<()> {
    if let Some(path) = PATH.get() {
        let _lock = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_kept_newest_first_and_trimmed() {
        // Other tests may note things meanwhile: find this test's own.
        let dir = std::env::temp_dir().join(format!("diagnostics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        init(dir.clone());
        note("timing", "diagnostics-test first");
        unreadable_reply("A diagnostics-test draft", &"x".repeat(2000));
        let list = entries();
        let first = list.iter().position(|e| e.message == "diagnostics-test first").unwrap();
        let reply = list.iter().position(|e| e.message.starts_with("A diagnostics-test draft")).unwrap();
        assert!(reply < first, "newest first");
        assert!(list[reply].message.ends_with(" …") && list[reply].message.len() < 800, "only the start of a reply");
        for n in 0..(KEEP * 2 + 5) {
            note("timing", format!("diagnostics-test n{n}"));
        }
        let list = entries();
        assert!(list.len() <= KEEP);
        assert!(list.iter().any(|e| e.message == format!("diagnostics-test n{}", KEEP * 2 + 4)));
        assert!(!list.iter().any(|e| e.message == "diagnostics-test first"), "the oldest went");
    }
}
