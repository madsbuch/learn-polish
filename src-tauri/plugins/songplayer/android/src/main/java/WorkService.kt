package com.polski.songplayer

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager

/**
 * Keeps the app running while it works in the background — the tutor
 * writing a reply, a song being made — so turning the screen off does not
 * freeze it half-way. A foreground service with a quiet notification, and a
 * partial wake lock so the network requests carry on with the screen off.
 * A course downloading shows its progress there (work.rs counts it).
 * Not stopped with the task: swiping the app away leaves the work running.
 * The app stops it as soon as the work is done.
 */
class WorkService : Service() {
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val label = intent?.getStringExtra(EXTRA_LABEL) ?: "Working…"
        val done = intent?.getLongExtra(EXTRA_DONE, 0L) ?: 0L
        val total = intent?.getLongExtra(EXTRA_TOTAL, 0L) ?: 0L
        val address = intent?.getStringExtra(EXTRA_ADDRESS)
        // A service started with startForegroundService must call
        // startForeground — even when it is only being told to stop (work
        // that ended at once); otherwise Android kills the app.
        try {
            startForegroundWith(notification(this, label, done, total, address))
        } catch (error: Exception) {
            // Not allowed from the background on newer Android: the work
            // runs as long as Android lets it, without the service. Said
            // to the app with its next keepWorking (Diagnostics).
            lastFailure = "${error.javaClass.simpleName}: ${error.message ?: "refused"}"
            stopSelf()
            return START_NOT_STICKY
        }
        running = true
        if (intent?.action == ACTION_STOP || stopRequested) {
            stopRequested = false
            finish()
            return START_NOT_STICKY
        }
        if (wakeLock == null) {
            val power = getSystemService(POWER_SERVICE) as PowerManager
            // A ceiling, in case the app never says it is done.
            wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "polski:work").apply {
                setReferenceCounted(false)
                acquire(30 * 60 * 1000L)
            }
        }
        return START_NOT_STICKY
    }

    private fun startForegroundWith(notification: Notification) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun finish() {
        running = false
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            stopForeground(STOP_FOREGROUND_REMOVE)
        } else {
            @Suppress("DEPRECATION")
            stopForeground(true)
        }
        stopSelf()
    }

    // Android 15 caps data-sync work; stop rather than be stopped.
    override fun onTimeout(startId: Int, fgsType: Int) {
        finish()
    }

    override fun onDestroy() {
        running = false
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
        super.onDestroy()
    }

    companion object {
        const val EXTRA_LABEL = "label"
        const val EXTRA_DONE = "done"
        const val EXTRA_TOTAL = "total"
        const val EXTRA_ADDRESS = "address"
        /** On the app's launch intent: the page a tapped notification opens (an address of the app's). */
        const val OPEN_ADDRESS = "com.polski.songplayer.OPEN_ADDRESS"
        private const val OPEN_WORK = 7301
        private const val OPEN_RESULT = 7302
        const val OPEN_PLAYER = 7303
        const val ACTION_STOP = "com.polski.songplayer.WORK_STOP"

        /** In the foreground right now. */
        @Volatile
        var running = false

        /** Told to stop before it got to start: stop as soon as it does. */
        @Volatile
        var stopRequested = false

        /** Why the service last could not come to the foreground, until the app is told. */
        @Volatile
        var lastFailure: String? = null
        private const val CHANNEL_ID = "polski-work"
        private const val NOTIFICATION_ID = 7301
        private const val RESULT_ID = 7302

        /** Redraws the notification of the service running: new words, a progress bar moving. */
        fun redraw(context: Context, label: String, done: Long, total: Long, address: String?) {
            try {
                context.getSystemService(NotificationManager::class.java)
                    .notify(NOTIFICATION_ID, notification(context, label, done, total, address))
            } catch (error: Exception) {
                // Notifications off: the work goes on without being seen.
            }
        }

        /** How finished work went (a course ready offline), until it is seen. */
        fun showResult(context: Context, text: String, address: String?) {
            try {
                val notification = builder(context)
                    .setSmallIcon(android.R.drawable.stat_sys_download_done)
                    .setContentTitle("Polski")
                    .setContentText(text)
                    .setAutoCancel(true)
                    .setContentIntent(openApp(context, address, OPEN_RESULT))
                    .build()
                context.getSystemService(NotificationManager::class.java).notify(RESULT_ID, notification)
            } catch (error: Exception) {
                // Notifications off: the course page says the same.
            }
        }

        fun notification(context: Context, label: String, done: Long, total: Long, address: String?): Notification {
            val builder = builder(context)
                // A plain system icon: launcher icons (adaptive) can be refused
                // as a notification icon, which crashes the app.
                .setSmallIcon(android.R.drawable.ic_popup_sync)
                .setContentTitle("Polski")
                .setContentText(label)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setContentIntent(openApp(context, address, OPEN_WORK))
            if (total > 0) {
                // Notifications take an Int; a course is far smaller.
                builder.setProgress(total.coerceAtMost(Int.MAX_VALUE.toLong()).toInt(), done.coerceIn(0L, total).toInt(), false)
            }
            return builder.build()
        }

        /**
         * Tapping a notification opens the app: at `address` when it has one
         * (a course downloading), else where the learner left it. Each kind
         * has its own request code, so one's address never ends up on another.
         */
        fun openApp(context: Context, address: String?, requestCode: Int): PendingIntent? =
            context.packageManager.getLaunchIntentForPackage(context.packageName)?.let {
                if (address != null) it.putExtra(OPEN_ADDRESS, address)
                PendingIntent.getActivity(context, requestCode, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
            }

        private fun builder(context: Context): Notification.Builder =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val manager = context.getSystemService(NotificationManager::class.java)
                manager.createNotificationChannel(
                    NotificationChannel(CHANNEL_ID, "Working in the background", NotificationManager.IMPORTANCE_LOW)
                )
                Notification.Builder(context, CHANNEL_ID)
            } else {
                @Suppress("DEPRECATION")
                Notification.Builder(context)
            }
    }
}
