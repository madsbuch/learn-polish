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
/// notification with `label` meanwhile. Does nothing off Android. An error
/// is Android refusing (a foreground service started while the app is in
/// the background, say), with its reason.
pub fn keep_working<R: Runtime>(app: &AppHandle<R>, on: bool, label: &str) -> Result<WorkReport, String> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        #[derive(serde::Serialize)]
        struct Args<'a> {
            on: bool,
            label: &'a str,
        }
        if let Some(native) = app.try_state::<Native<R>>() {
            return native.0.run_mobile_plugin::<WorkReport>("keepWorking", Args { on, label }).map_err(|e| e.to_string());
        }
    }
    let _ = (app, on, label);
    Ok(WorkReport { refused: None, notifications_allowed: true })
}
