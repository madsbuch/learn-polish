//! Native song playback on Android.
//!
//! The web view cannot reach the phone's media controls, and it pauses when
//! the app goes to the background. This plugin plays songs through Media3 in
//! a foreground service instead, which gives the lock screen, notification,
//! headphone and Bluetooth controls, and keeps playing with the screen off.
//! On desktop it does nothing and the app keeps using web audio.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("songplayer")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin("com.polski.songplayer", "PlayerPlugin")?;
            Ok(())
        })
        .build()
}
