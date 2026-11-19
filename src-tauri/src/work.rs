//! Background work that must survive the screen turning off: the tutor
//! writing a reply, a song being made, any request to the AI. While any is
//! running, Android is told to keep the app alive (a foreground service with
//! a quiet notification, see the songplayer plugin's WorkService); when the
//! last one ends, it may freeze the app again.
//!
//! Counted, so pieces of work nest for free. The app's handle is kept here
//! once set (at startup, [`set_app`]), so code with no handle of its own —
//! every OpenRouter request in ai.rs — can hold a piece too ([`busy`]).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter};

static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// The app, for the work that has no handle of its own. Unset in tests:
/// work is then only counted.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Called once at startup.
pub fn set_app(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// Held for as long as a piece of work runs; dropping it ends the work.
pub struct Busy(());

/// Starts a piece of work. `label` is shown in the notification if this is
/// the first one running.
pub fn begin(app: &AppHandle, label: &str) -> Busy {
    set_app(app);
    busy(label)
}

/// Starts a piece of work, from anywhere (see [`begin`]).
pub fn busy(label: &str) -> Busy {
    if RUNNING.fetch_add(1, Ordering::SeqCst) == 0 {
        if let Some(app) = APP.get() {
            // The screens: the first time, notifications are asked for, so
            // the work's notification can show (Android 13 and later).
            let _ = app.emit("work://began", ());
            report(tauri_plugin_songplayer::keep_working(app, true, label));
        }
    }
    Busy(())
}

impl Drop for Busy {
    fn drop(&mut self) {
        if RUNNING.fetch_sub(1, Ordering::SeqCst) == 1 {
            if let Some(app) = APP.get() {
                report(tauri_plugin_songplayer::keep_working(app, false, ""));
            }
        }
    }
}

/// The kinds of trouble already in Diagnostics this run: each is said once.
static NOTED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

fn note_once(kind: &'static str, message: String) {
    {
        let mut noted = NOTED.lock().unwrap_or_else(|e| e.into_inner());
        if noted.contains(&kind) {
            return;
        }
        noted.push(kind);
    }
    crate::diagnostics::note("background", message);
}

/// Puts what Android refused in Diagnostics: work then runs only as long as
/// Android lets an app in the background run.
fn report(result: Result<tauri_plugin_songplayer::WorkReport, String>) {
    match result {
        Ok(report) => {
            if let Some(reason) = report.refused {
                note_once("refused", format!("Android would not keep Polski working in the background: {reason}"));
            }
            if !report.notifications_allowed {
                note_once(
                    "notifications",
                    "Polski works in the background without its notification: notifications are off for Polski".into(),
                );
            }
        }
        Err(reason) => note_once("start", format!("Could not keep working in the background: {reason}")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn work_nests_without_an_app() {
        let outer = super::busy("outer");
        let inner = super::busy("inner");
        assert!(super::RUNNING.load(super::Ordering::SeqCst) >= 2);
        drop(inner);
        drop(outer);
    }
}
