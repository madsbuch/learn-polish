//! Background work that must survive the screen turning off: the tutor
//! writing a reply, a song being made, a course downloading, any request to
//! the AI. While any is running, Android is told to keep the app alive (a
//! foreground service with a quiet notification, see the songplayer plugin's
//! WorkService); when the last one ends, it may freeze the app again.
//!
//! Counted, so pieces of work nest for free. The app's handle is kept here
//! once set (at startup, [`set_app`]), so code with no handle of its own —
//! every OpenRouter request in ai.rs — can hold a piece too ([`busy`]).
//!
//! A piece of work can also have a title and say how far it has come
//! ([`begin_counted`], [`Busy::progress`]): a course downloading. The one
//! notification then shows that, with a progress bar, instead of the label
//! of whatever started first; a few at once are added up.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// The app, for the work that has no handle of its own. Unset in tests:
/// work is then only counted.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Called once at startup.
pub fn set_app(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// The app, when it has started in this process: a run in the background
/// (offline/background.rs) tells its screens what changed through it.
pub fn app() -> Option<&'static AppHandle> {
    APP.get()
}

/// How often the notification is redrawn while work counts up: often
/// enough to see it move, rarely enough to cost nothing.
const REDRAW: Duration = Duration::from_secs(1);

/// A piece of work with a title, and how far it has come.
#[derive(Debug, Clone, PartialEq)]
struct Counted {
    id: u64,
    title: String,
    done: i64,
    total: i64,
}

/// What the notification says now.
#[derive(Default)]
struct Shown {
    /// The label of the work that started first: shown when nothing counts.
    label: String,
    counted: Vec<Counted>,
    next_id: u64,
    drawn_at: Option<Instant>,
}

static SHOWN: Mutex<Shown> = Mutex::new(Shown { label: String::new(), counted: Vec::new(), next_id: 0, drawn_at: None });

/// The notification's text and progress bar (`total` 0: none) for the work
/// running: the counted work if there is any — one by its title, a few
/// added up — else the label.
fn notice(label: &str, counted: &[Counted]) -> (String, i64, i64) {
    let done = counted.iter().map(|c| c.done).sum::<i64>();
    let total = counted.iter().map(|c| c.total).sum::<i64>();
    match counted {
        [] => (label.to_string(), 0, 0),
        [one] => (format!("{} · {done} of {total}", one.title), done, total),
        many => (format!("{} downloads · {done} of {total}", many.len()), done, total),
    }
}

/// Whether the notification may be drawn again: not more often than [`REDRAW`].
fn due(drawn_at: Option<Instant>, now: Instant) -> bool {
    drawn_at.map_or(true, |at| now.duration_since(at) >= REDRAW)
}

/// Held for as long as a piece of work runs; dropping it ends the work.
pub struct Busy {
    /// Its place among the counted work, if it counts.
    counted: Option<u64>,
    /// Said once it ends, in a notification of its own.
    result: Option<String>,
}

/// Starts a piece of work. `label` is shown in the notification if this is
/// the first one running.
pub fn begin(app: &AppHandle, label: &str) -> Busy {
    set_app(app);
    busy(label)
}

/// Starts a piece of work, from anywhere (see [`begin`]).
pub fn busy(label: &str) -> Busy {
    if RUNNING.fetch_add(1, Ordering::SeqCst) == 0 {
        SHOWN.lock().unwrap_or_else(|e| e.into_inner()).label = label.to_string();
        if let Some(app) = APP.get() {
            // The screens: the first time, notifications are asked for, so
            // the work's notification can show (Android 13 and later).
            let _ = app.emit("work://began", ());
            report(tauri_plugin_songplayer::keep_working(app, true, label, None, None));
        }
    }
    Busy { counted: None, result: None }
}

/// Starts a piece of work that says how far it has come, under `title` (a
/// course's): the notification shows it with a progress bar.
pub fn begin_counted(app: &AppHandle, title: &str) -> Busy {
    set_app(app);
    busy_counted(title)
}

fn busy_counted(title: &str) -> Busy {
    let mut busy = busy(title);
    let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    shown.next_id += 1;
    let id = shown.next_id;
    shown.counted.push(Counted { id, title: title.to_string(), done: 0, total: 0 });
    // Drawn with the first count, which follows at once.
    shown.drawn_at = None;
    busy.counted = Some(id);
    busy
}

impl Busy {
    /// How far the work has come: `done` of `total`. Redrawn at most once a
    /// second; the last count before a pause shows with the next.
    pub fn progress(&self, done: i64, total: i64) {
        let Some(id) = self.counted else { return };
        let drawn = {
            let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = shown.counted.iter_mut().find(|c| c.id == id) {
                entry.done = done;
                entry.total = total;
            }
            let now = Instant::now();
            if !due(shown.drawn_at, now) {
                return;
            }
            shown.drawn_at = Some(now);
            notice(&shown.label, &shown.counted)
        };
        if let Some(app) = APP.get() {
            let (text, done, total) = drawn;
            report(tauri_plugin_songplayer::keep_working(app, true, &text, Some((done, total)), None));
        }
    }

    /// Ends the work, saying how it went (a course ready offline, or what
    /// could not be downloaded) in a notification that stays until seen.
    pub fn finish(mut self, result: Option<String>) {
        self.result = result;
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        let next = {
            let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(id) = self.counted {
                shown.counted.retain(|c| c.id != id);
            }
            notice(&shown.label, &shown.counted)
        };
        let result = self.result.as_deref();
        if RUNNING.fetch_sub(1, Ordering::SeqCst) == 1 {
            if let Some(app) = APP.get() {
                report(tauri_plugin_songplayer::keep_working(app, false, "", None, result));
            }
        } else if self.counted.is_some() {
            // Other work goes on: the notification says what is left.
            if let Some(app) = APP.get() {
                let (text, done, total) = next;
                let progress = (total > 0).then_some((done, total));
                report(tauri_plugin_songplayer::keep_working(app, true, &text, progress, result));
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
    use super::*;

    #[test]
    fn work_nests_without_an_app() {
        let outer = busy("outer");
        let inner = busy("inner");
        assert!(RUNNING.load(Ordering::SeqCst) >= 2);
        drop(inner);
        drop(outer);
    }

    #[test]
    fn counted_work_is_shown_by_its_title_then_added_up() {
        let course = |id, title: &str, done, total| Counted { id, title: title.into(), done, total };
        assert_eq!(notice("Working…", &[]), ("Working…".into(), 0, 0));
        assert_eq!(
            notice("Working…", &[course(1, "Pierwsze kroki", 34, 120)]),
            ("Pierwsze kroki · 34 of 120".into(), 34, 120)
        );
        assert_eq!(
            notice("Working…", &[course(1, "Pierwsze kroki", 34, 120), course(2, "W kawiarni", 16, 80)]),
            ("2 downloads · 50 of 200".into(), 50, 200)
        );
    }

    #[test]
    fn the_notification_is_redrawn_at_most_once_a_second() {
        let now = Instant::now();
        assert!(due(None, now));
        assert!(!due(Some(now), now + Duration::from_millis(400)));
        assert!(due(Some(now), now + Duration::from_millis(1000)));
    }

    #[test]
    fn counted_work_leaves_the_notification_when_it_ends() {
        let download = busy_counted("Pierwsze kroki");
        let id = download.counted.unwrap();
        download.progress(3, 10);
        let entry = |id| SHOWN.lock().unwrap().counted.iter().find(|c| c.id == id).cloned();
        assert_eq!(entry(id).map(|c| (c.done, c.total)), Some((3, 10)));
        download.finish(Some("Ready offline".into()));
        assert_eq!(entry(id), None);
    }
}
