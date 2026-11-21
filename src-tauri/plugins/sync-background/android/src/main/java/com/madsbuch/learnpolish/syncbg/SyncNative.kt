package com.madsbuch.learnpolish.syncbg

/**
 * The app's Rust library, entered directly: the worker runs without the web
 * view, so there is no Tauri bridge to go through. Implemented by
 * `Java_com_madsbuch_learnpolish_syncbg_SyncNative_syncNow`, which the app
 * crate expands from this plugin's `jni_entry!` (src-tauri/src/sync/background.rs).
 */
object SyncNative {
    init {
        // Already loaded when the app is running; loading again does nothing.
        System.loadLibrary("learn_polish_lib")
    }

    /** One sync with whichever of the vault's devices answer; returns a line for the log. */
    @JvmStatic
    external fun syncNow(dbPath: String): String
}
