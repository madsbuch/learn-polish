package com.madsbuch.learnpolish.syncbg

/**
 * The app's Rust library, entered directly for the courses kept offline,
 * as [SyncNative] is for sync. Implemented by the functions the app crate
 * expands from this plugin's `offline_jni_entry!`
 * (src-tauri/src/offline/background.rs).
 */
object OfflineNative {
    init {
        // Already loaded when the app is running; loading again does nothing.
        System.loadLibrary("learn_polish_lib")
    }

    /**
     * Downloads what the courses kept offline lack, one course at a time,
     * and returns how it went as JSON: `log` (a line for the log), `retry`
     * (the app is downloading in this process: come back later) and
     * `result` (what to tell the learner, if anything).
     */
    @JvmStatic
    external fun downloadNow(dbPath: String): String

    /** How far [downloadNow] has come, as JSON (`text`, `done`, `total`), or "" before it starts one. */
    @JvmStatic
    external fun progress(): String
}
