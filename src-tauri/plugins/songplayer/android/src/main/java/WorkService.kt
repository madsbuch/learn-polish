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
        startForegroundWith(notification(label))
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
            .setSmallIcon(applicationInfo.icon)
            .setContentTitle("Polski")
            .setContentText(label)
            .setOngoing(true)
            .setContentIntent(open)
            .build()
    }

    // Android 15 caps data-sync work; stop rather than be stopped.
    override fun onTimeout(startId: Int, fgsType: Int) {
        stopSelf()
    }

    override fun onDestroy() {
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
        super.onDestroy()
    }

    companion object {
        const val EXTRA_LABEL = "label"
        private const val CHANNEL_ID = "polski-work"
        private const val NOTIFICATION_ID = 7301
    }
}
