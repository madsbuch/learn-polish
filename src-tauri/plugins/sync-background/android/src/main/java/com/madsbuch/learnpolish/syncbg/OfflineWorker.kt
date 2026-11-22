package com.madsbuch.learnpolish.syncbg

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.pm.ServiceInfo
import android.os.Build
import android.util.Log
import androidx.work.ForegroundInfo
import androidx.work.Worker
import androidx.work.WorkerParameters
import org.json.JSONObject

/**
 * Finishes downloading the courses kept offline when the app could not: it
 * was stopped, or Android ended it, mid-download. All in Rust
 * (`OfflineNative.downloadNow`), which skips a database the app hasn't
 * upgraded and courses the app is downloading itself; while the app
 * downloads in this process, it asks to be run again later instead.
 *
 * Shows the download's progress, as the app does: in the foreground when
 * Android allows it, else as a plain notification (Android 12 and later
 * refuse a foreground service started from the background) — the work runs
 * either way, for as long as Android gives it, and the next run carries on.
 */
class OfflineWorker(ctx: Context, params: WorkerParameters) : Worker(ctx, params) {
    @Volatile
    private var foreground = false

    @Volatile
    private var shown = false

    override fun doWork(): Result {
        val dbPath = inputData.getString(KEY_DB_PATH) ?: return Result.success()
        // Rust counts; this draws, once a second.
        val watcher = Thread {
            try {
                while (true) {
                    val now = try {
                        OfflineNative.progress()
                    } catch (e: Throwable) {
                        ""
                    }
                    if (now.isNotEmpty()) show(JSONObject(now))
                    Thread.sleep(1000)
                }
            } catch (e: InterruptedException) {
                // Done.
            } catch (e: Exception) {
                Log.w(TAG, "Offline download: could not show progress", e)
            }
        }
        watcher.start()
        val outcome = try {
            JSONObject(OfflineNative.downloadNow(dbPath))
        } catch (e: Throwable) {
            Log.w(TAG, "Offline download failed", e)
            null
        }
        watcher.interrupt()
        watcher.join()
        val manager = applicationContext.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        // In the foreground, WorkManager takes it away; otherwise this does.
        if (shown && !foreground) manager.cancel(NOTIFICATION_ID)
        if (outcome != null && !outcome.isNull("result")) {
            try {
                manager.notify(RESULT_ID, result(outcome.getString("result"), outcome.optString("address").ifEmpty { null }))
            } catch (e: Exception) {
                // Notifications off: the course page says the same.
            }
        }
        Log.i(TAG, "Offline download: ${outcome?.optString("log") ?: "failed"}")
        return if (outcome?.optBoolean("retry") == true) Result.retry() else Result.success()
    }

    /** Expedited work on Android 11 and earlier runs as a foreground service with this. */
    override fun getForegroundInfo(): ForegroundInfo = foregroundInfo(progress("Downloading for offline use…", 0, 0, null))

    private fun show(now: JSONObject) {
        val notification = progress(now.optString("text"), now.optLong("done"), now.optLong("total"), now.optString("address").ifEmpty { null })
        if (!shown) {
            shown = true
            foreground = try {
                setForegroundAsync(foregroundInfo(notification)).get()
                true
            } catch (e: Exception) {
                false
            }
        }
        try {
            val manager = applicationContext.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            manager.notify(NOTIFICATION_ID, notification)
        } catch (e: Exception) {
            // Notifications off: the download goes on without being seen.
        }
    }

    private fun foregroundInfo(notification: Notification): ForegroundInfo =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            ForegroundInfo(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            ForegroundInfo(NOTIFICATION_ID, notification)
        }

    private fun progress(text: String, done: Long, total: Long, address: String?): Notification {
        val builder = builder(address, OPEN_PROGRESS)
            // A plain system icon, as the app's other work notification.
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle("Polski")
            .setContentText(text)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
        if (total > 0) {
            builder.setProgress(total.coerceAtMost(Int.MAX_VALUE.toLong()).toInt(), done.coerceIn(0L, total).toInt(), false)
        }
        return builder.build()
    }

    private fun result(text: String, address: String?): Notification =
        builder(address, OPEN_RESULT)
            .setSmallIcon(android.R.drawable.stat_sys_download_done)
            .setContentTitle("Polski")
            .setContentText(text)
            .setAutoCancel(true)
            .build()

    /** A tap opens the app: at `address` when there is one (the course), else where it was left. */
    // Spec: SHELL-20
    private fun builder(address: String?, requestCode: Int): Notification.Builder {
        val context = applicationContext
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            // The channel of the app's other work in the background.
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "Working in the background", NotificationManager.IMPORTANCE_LOW),
            )
            Notification.Builder(context, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(context)
        }
        context.packageManager.getLaunchIntentForPackage(context.packageName)?.let {
            if (address != null) it.putExtra(OPEN_ADDRESS, address)
            builder.setContentIntent(
                PendingIntent.getActivity(context, requestCode, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT),
            )
        }
        return builder
    }

    companion object {
        const val TAG = "Polski"
        const val NAME = "polski-offline"
        const val KEY_DB_PATH = "dbPath"
        private const val CHANNEL_ID = "polski-work"
        private const val NOTIFICATION_ID = 7303

        // Shared with the app's own work notification (songplayer's WorkService): the latest result.
        private const val RESULT_ID = 7302

        // The song player's WorkService.OPEN_ADDRESS, which it reads as the app starts.
        private const val OPEN_ADDRESS = "com.polski.songplayer.OPEN_ADDRESS"

        // Each notification's own request code, so one's address never ends up on another's tap;
        // the result's is WorkService's, as it takes the same place.
        private const val OPEN_PROGRESS = 7304
        private const val OPEN_RESULT = 7302
    }
}
