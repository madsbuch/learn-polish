package com.polski.songplayer

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
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
 * The app stops it as soon as the work is done.
 */
class WorkService : Service() {
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val label = intent?.getStringExtra(EXTRA_LABEL) ?: "Working…"
        // A service started with startForegroundService must call
        // startForeground — even when it is only being told to stop (work
        // that ended at once); otherwise Android kills the app.
        try {
            startForegroundWith(notification(label))
        } catch (error: Exception) {
            // Not allowed from the background on newer Android: the work
            // runs as long as Android lets it, without the service.
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

    private fun notification(label: String): Notification {
        val open = packageManager.getLaunchIntentForPackage(packageName)?.let {
            PendingIntent.getActivity(this, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "Working in the background", NotificationManager.IMPORTANCE_LOW)
            )
            Notification.Builder(this, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        return builder
            // A plain system icon: launcher icons (adaptive) can be refused
            // as a notification icon, which crashes the app.
            .setSmallIcon(android.R.drawable.ic_popup_sync)
            .setContentTitle("Polski")
            .setContentText(label)
            .setOngoing(true)
            .setContentIntent(open)
            .build()
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
        const val ACTION_STOP = "com.polski.songplayer.WORK_STOP"

        /** In the foreground right now. */
        @Volatile
        var running = false

        /** Told to stop before it got to start: stop as soon as it does. */
        @Volatile
        var stopRequested = false
        private const val CHANNEL_ID = "polski-work"
        private const val NOTIFICATION_ID = 7301
    }
}
