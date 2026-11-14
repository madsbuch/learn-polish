//! Background work that must survive the screen turning off: the tutor
//! writing a reply, a song being made. While any is running, Android is told
//! to keep the app alive (a foreground service with a quiet notification,
//! see the songplayer plugin's WorkService); when the last one ends, it may
//! freeze the app again.

use std::sync::atomic::{AtomicUsize, Ordering};

use tauri::AppHandle;

static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Held for as long as a piece of work runs; dropping it ends the work.
pub struct Busy {
    app: AppHandle,
}

/// Starts a piece of work. `label` is shown in the notification if this is
/// the first one running.
pub fn begin(app: &AppHandle, label: &str) -> Busy {
    if RUNNING.fetch_add(1, Ordering::SeqCst) == 0 {
        tauri_plugin_songplayer::keep_working(app, true, label);
    }
    Busy { app: app.clone() }
}

impl Drop for Busy {
    fn drop(&mut self) {
        if RUNNING.fetch_sub(1, Ordering::SeqCst) == 1 {
            tauri_plugin_songplayer::keep_working(&self.app, false, "");
        }
    }
}
