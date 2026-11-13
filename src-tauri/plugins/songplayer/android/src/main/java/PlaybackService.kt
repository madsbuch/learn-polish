package com.polski.songplayer

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.wifi.WifiManager
import android.os.Looper
import android.os.PowerManager
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

/**
 * Owns the phone's media session. Normally the session plays downloaded songs
 * with ExoPlayer; while a Sonos speaker is playing, it swaps in [RemotePlayer]
 * so the lock screen, notification and volume keys control the speaker.
 */
class PlaybackService : MediaSessionService() {
    companion object {
        @Volatile
        var instance: PlaybackService? = null
            private set
    }

    private lateinit var local: ExoPlayer
    private var remote: RemotePlayer? = null
    private var session: MediaSession? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private var wakeLock: PowerManager.WakeLock? = null

    val isRemote: Boolean
        get() = remote != null

    override fun onCreate() {
        super.onCreate()
        local = ExoPlayer.Builder(this)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(C.USAGE_MEDIA)
                    .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
                    .build(),
                /* handleAudioFocus= */ true,
            )
            // Pause when headphones are unplugged, like every music app.
            .setHandleAudioBecomingNoisy(true)
            .setWakeMode(C.WAKE_MODE_LOCAL)
            .build()

        val builder = MediaSession.Builder(this, local).setCallback(SessionCallback())
        packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
            // Tapping the notification opens the app.
            builder.setSessionActivity(
                PendingIntent.getActivity(
                    this,
                    0,
                    launch,
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                ),
            )
        }
        session = builder.build()
        instance = this
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    override fun onTaskRemoved(rootIntent: Intent?) {
        // Swiping the app away stops the service unless music is playing.
        val player = session?.player
        if (player == null || !player.playWhenReady || player.mediaItemCount == 0) {
            stopRemote()
            stopSelf()
        }
    }

    override fun onDestroy() {
        instance = null
        stopRemote()
        session?.release()
        session = null
        local.release()
        super.onDestroy()
    }

    /** Hands the media session to a Sonos speaker. */
    fun startRemote(port: Int, token: String, deviceName: String) {
        local.pause()
        remote?.release()
        val player = RemotePlayer(Looper.getMainLooper(), RemoteClient(port, token), deviceName) {
            // The cast ended elsewhere (Sonos app, speaker gone): go back.
            stopRemote()
        }
        remote = player
        session?.player = player
        holdNetwork(true)
    }

    /** Takes the media session back for playback on the phone. */
    fun stopRemote() {
        val player = remote ?: return
        remote = null
        session?.player = local
        player.release()
        holdNetwork(false)
    }

    /**
     * The speaker streams each song from this phone, so Wi-Fi and the CPU must
     * stay awake while it plays, even with the screen off.
     */
    private fun holdNetwork(hold: Boolean) {
        if (hold) {
            if (wifiLock == null) {
                val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
                @Suppress("DEPRECATION")
                wifiLock = wifi.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "polski:cast")
                    .apply { setReferenceCounted(false); acquire() }
            }
            if (wakeLock == null) {
                val power = getSystemService(Context.POWER_SERVICE) as PowerManager
                wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "polski:cast")
                    .apply { setReferenceCounted(false); acquire() }
            }
        } else {
            wifiLock?.let { if (it.isHeld) it.release() }
            wifiLock = null
            wakeLock?.let { if (it.isHeld) it.release() }
            wakeLock = null
        }
    }

    private inner class SessionCallback : MediaSession.Callback {
        /**
         * Controllers hand media items over without their file URI (Media3
         * strips it), so the plugin sends the path in the request metadata
         * and it is restored here.
         */
        override fun onAddMediaItems(
            mediaSession: MediaSession,
            controller: MediaSession.ControllerInfo,
            mediaItems: MutableList<MediaItem>,
        ): ListenableFuture<MutableList<MediaItem>> {
            val restored = mediaItems.map { item ->
                val uri = item.requestMetadata.mediaUri
                if (uri != null) item.buildUpon().setUri(uri).build() else item
            }.toMutableList()
            return Futures.immediateFuture(restored)
        }
    }
}
