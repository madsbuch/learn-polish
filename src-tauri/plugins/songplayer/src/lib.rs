//! Native song playback on Android.
//!
//! The web view cannot reach the phone's media controls, and it pauses when
//! the app goes to the background. This plugin plays songs through Media3 in
//! a foreground service instead, which gives the lock screen, notification,
//! headphone and Bluetooth controls, and keeps playing with the screen off.
//! On desktop it does nothing and the app keeps using web audio.

use tauri::{
    plugin::{Builder, TauriPlugin},
    AppHandle, Runtime,
};

#[cfg(target_os = "android")]
struct Native<R: Runtime>(tauri::plugin::PluginHandle<R>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("songplayer")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = _api.register_android_plugin("com.polski.songplayer", "PlayerPlugin")?;
                _app.manage(Native(handle));
            }
            Ok(())
        })
        .build()
}

/// Where taps on the work's notifications lead: addresses of the app's pages
/// (a course downloading), opened through the "open" event.
#[derive(Debug, Clone, Copy, Default)]
pub struct Work<'a> {
    /// The notification while the work goes on.
    pub address: Option<&'a str>,
    /// The notification that says how it went.
    pub result_address: Option<&'a str>,
}

/// What Android said to [`keep_working`].
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkReport {
    /// Why an earlier start could not come to the foreground (the service
    /// itself was refused, after it was asked to start).
    pub refused: Option<String>,
    /// Whether the work's notification can be seen: on Android 13 and later
    /// only once notifications are allowed (the work runs either way).
    pub notifications_allowed: bool,
}

/// Keeps the app running with the screen off while it works in the
/// background (on), or lets Android freeze it again (off). Shows a quiet
/// notification with `label` meanwhile, with a progress bar when there is
/// `progress` (done, total); asked again while on, it redraws it. A
/// `result` is said in a notification of its own that stays until seen (a
/// course ready offline); `work` says what a tap on either opens. Does
/// nothing off Android. An error is Android
/// refusing (a foreground service started while the app is in the
/// background, say), with its reason.
pub fn keep_working<R: Runtime>(
    app: &AppHandle<R>,
    on: bool,
    label: &str,
    progress: Option<(i64, i64)>,
    result: Option<&str>,
    work: Option<Work<'_>>,
) -> Result<WorkReport, String> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        #[derive(serde::Serialize)]
        struct Args<'a> {
            on: bool,
            label: &'a str,
            /// No progress bar: both 0.
            done: i64,
            total: i64,
            result: Option<&'a str>,
            /// Where a tap on each notification leads (the app's addresses).
            #[serde(rename = "address")]
            address: Option<&'a str>,
            #[serde(rename = "resultAddress")]
            result_address: Option<&'a str>,
        }
        let (done, total) = progress.unwrap_or((0, 0));
        let work = work.unwrap_or_default();
        if let Some(native) = app.try_state::<Native<R>>() {
            return native
                .0
                .run_mobile_plugin::<WorkReport>(
                    "keepWorking",
                    Args { on, label, done, total, result, address: work.address, result_address: work.result_address },
                )
                .map_err(|e| e.to_string());
        }
    }
    let _ = (app, on, label, progress, result, work);
    Ok(WorkReport { refused: None, notifications_allowed: true })
}

/// Shows a message (a tutor reply) in a notification of its own, which a
/// tap opens at `address` (the app's address for its page), also when the
/// tap starts the app. Returns whether it was shown here: off Android it is
/// not, and the caller shows it through the notification plugin.
pub fn show_message<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str, address: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        #[derive(serde::Serialize)]
        struct Args<'a> {
            title: &'a str,
            body: &'a str,
            address: &'a str,
        }
        if let Some(native) = app.try_state::<Native<R>>() {
            return native.0.run_mobile_plugin::<serde::de::IgnoredAny>("showMessage", Args { title, body, address }).is_ok();
        }
    }
    let _ = (app, title, body, address);
    false
}
