//! Background sync on Android: a WorkManager job every half hour, on
//! unmetered networks only, that runs one round of the app's Rust sync while
//! the app is closed. Elsewhere it does nothing: a computer syncs while the
//! app runs (docs/SYNC.md).
//!
//! And the same for courses kept offline: a one-off job, on any network,
//! that finishes a download the app could not ([`schedule_offline`]; the
//! app's offline/background.rs).
//!
//! Two halves. Rust → Kotlin is Tauri's bridge: [`schedule`] asks the
//! plugin to set up the job, [`cancel`] to drop it. Kotlin → Rust is JNI: the job runs without the
//! web view or Tauri, so it calls a native method, and [`jni_entry!`] makes
//! that method out of a plain function of the app's.

use tauri::{
    plugin::{Builder, TauriPlugin},
    AppHandle, Runtime,
};

#[cfg(target_os = "android")]
pub use jni;

#[cfg(target_os = "android")]
struct Native<R: Runtime>(tauri::plugin::PluginHandle<R>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("sync-background")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = _api.register_android_plugin("com.madsbuch.learnpolish.syncbg", "SyncBackgroundPlugin")?;
                _app.manage(Native(handle));
            }
            Ok(())
        })
        .build()
}

/// Schedules the background sync for the database at `db_path` (again: the
/// schedule is kept, only the path is updated). Does nothing off Android.
pub fn schedule<R: Runtime>(app: &AppHandle<R>, db_path: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Args<'a> {
            db_path: &'a str,
        }
        let native = app.try_state::<Native<R>>().ok_or("the background sync plugin is not set up")?;
        native.0.run_mobile_plugin::<serde::de::IgnoredAny>("schedule", Args { db_path }).map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "android"))]
    let _ = (app, db_path);
    Ok(())
}

/// Makes sure the courses kept offline finish downloading even if the app
/// is stopped before they do: a job, kept by WorkManager across the app
/// being closed, that waits for a connection and runs the download for the
/// database at `db_path` in the background. Asked again while one is
/// waiting, the waiting one stays. Does nothing off Android.
pub fn schedule_offline<R: Runtime>(app: &AppHandle<R>, db_path: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Args<'a> {
            db_path: &'a str,
        }
        let native = app.try_state::<Native<R>>().ok_or("the background plugin is not set up")?;
        native.0.run_mobile_plugin::<serde::de::IgnoredAny>("scheduleOffline", Args { db_path }).map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "android"))]
    let _ = (app, db_path);
    Ok(())
}

/// Cancels the background sync (this device has nobody to sync with any
/// more). Does nothing off Android, or when nothing was scheduled.
pub fn cancel<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        let native = app.try_state::<Native<R>>().ok_or("the background sync plugin is not set up")?;
        // An empty object, not nothing: the Kotlin side reads its arguments
        // as a JSON object.
        #[derive(serde::Serialize)]
        struct NoArgs {}
        native.0.run_mobile_plugin::<serde::de::IgnoredAny>("cancel", NoArgs {}).map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "android"))]
    let _ = app;
    Ok(())
}

/// What the JNI entry point does, as safe code: reads the database path
/// Kotlin passed, runs `run` with it, and hands back its summary (null if
/// either string can't cross).
#[cfg(target_os = "android")]
pub fn sync_now<'local>(
    env: &mut jni::JNIEnv<'local>,
    db_path: &jni::objects::JString<'local>,
    run: fn(&str) -> String,
) -> jni::sys::jstring {
    let path: String = match env.get_string(db_path) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let summary = run(&path);
    env.new_string(summary).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// What a JNI entry point without arguments does, as safe code: hands back
/// the string `run` returns (null if it can't cross).
#[cfg(target_os = "android")]
pub fn string_from(env: &mut jni::JNIEnv<'_>, run: fn() -> String) -> jni::sys::jstring {
    env.new_string(run()).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// Defines `SyncNative.syncNow` (Kotlin, `com.madsbuch.learnpolish.syncbg`)
/// as a call to `$run: fn(&str) -> String`, which syncs the database at that
/// path once and returns a line for the log.
///
/// A JNI method must be an exported, unmangled symbol in the library the
/// app loads (`learn_polish_lib`), so it has to be defined in the app crate,
/// and it needs `#[no_mangle]` — which `unsafe_code = "forbid"` counts as
/// unsafe. Written here, as a macro, the attribute is this crate's, the way
/// `tauri::mobile_entry_point` writes the app's other JNI entry points; the
/// app's own code stays a safe function. Android only.
#[macro_export]
macro_rules! jni_entry {
    ($run:path) => {
        #[cfg(target_os = "android")]
        #[no_mangle]
        pub extern "system" fn Java_com_madsbuch_learnpolish_syncbg_SyncNative_syncNow<'local>(
            mut env: $crate::jni::JNIEnv<'local>,
            _class: $crate::jni::objects::JClass<'local>,
            db_path: $crate::jni::objects::JString<'local>,
        ) -> $crate::jni::sys::jstring {
            $crate::sync_now(&mut env, &db_path, $run)
        }
    };
}

/// Defines `OfflineNative.downloadNow` (Kotlin, `com.madsbuch.learnpolish.syncbg`)
/// as a call to `$run: fn(&str) -> String`, which downloads what the courses
/// kept offline lack for the database at that path and says how it went,
/// and `OfflineNative.progress` as a call to `$progress: fn() -> String`,
/// how far that download has come. Written here for the reason
/// [`jni_entry!`] is. Android only.
#[macro_export]
macro_rules! offline_jni_entry {
    ($run:path, $progress:path) => {
        #[cfg(target_os = "android")]
        #[no_mangle]
        pub extern "system" fn Java_com_madsbuch_learnpolish_syncbg_OfflineNative_downloadNow<'local>(
            mut env: $crate::jni::JNIEnv<'local>,
            _class: $crate::jni::objects::JClass<'local>,
            db_path: $crate::jni::objects::JString<'local>,
        ) -> $crate::jni::sys::jstring {
            $crate::sync_now(&mut env, &db_path, $run)
        }

        #[cfg(target_os = "android")]
        #[no_mangle]
        pub extern "system" fn Java_com_madsbuch_learnpolish_syncbg_OfflineNative_progress<'local>(
            mut env: $crate::jni::JNIEnv<'local>,
            _class: $crate::jni::objects::JClass<'local>,
        ) -> $crate::jni::sys::jstring {
            $crate::string_from(&mut env, $progress)
        }
    };
}
