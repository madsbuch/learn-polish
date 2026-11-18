package com.madsbuch.learnpolish.syncbg

import android.content.Context
import android.util.Log
import androidx.work.Worker
import androidx.work.WorkerParameters

/**
 * One background sync: finds the vault's other devices on the network and
 * syncs with them, all in Rust (`SyncNative.syncNow`). Always reports
 * success: a computer that is off is the normal case, and the next run is
 * half an hour away anyway.
 */
class SyncWorker(ctx: Context, params: WorkerParameters) : Worker(ctx, params) {
    override fun doWork(): Result {
        val dbPath = inputData.getString(KEY_DB_PATH) ?: return Result.success()
        try {
            val summary = SyncNative.syncNow(dbPath)
            Log.i(TAG, "Background sync: $summary")
        } catch (e: Throwable) {
            Log.w(TAG, "Background sync failed", e)
        }
        return Result.success()
    }

    companion object {
        const val TAG = "Polski"
        const val NAME = "polski-sync"
        const val KEY_DB_PATH = "dbPath"
    }
}
