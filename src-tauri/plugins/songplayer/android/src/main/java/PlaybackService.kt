package com.polski.songplayer

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.wifi.WifiManager
import android.os.Bundle
import android.os.Looper
import android.os.PowerManager
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.CommandButton
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionResult
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

/**
 * Owns the phone's media session. Normally the session plays downloaded songs
 * with ExoPlayer; while a Sonos speaker is playing, it swaps in [RemotePlayer]
 * so the lock screen, notification and volume keys control the speaker.
 *
 * The notification with the controls stays for as long as songs are queued —
 * paused too — and has a Close button that stops the music and removes it.
 */
class PlaybackService : MediaSessionService() {
    companion object {
        @Volatile
        var instance: PlaybackService? = null
            private set

        /** The notification's Close button. */
        private val CLOSE = SessionCommand("polski.close", Bundle.EMPTY)
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

        @Suppress("DEPRECATION")
        val close = CommandButton.Builder()
            .setDisplayName("Close")
            .setIconResId(android.R.drawable.ic_menu_close_clear_cancel)
            .setSessionCommand(CLOSE)
            .build()
        @Suppress("DEPRECATION")
        val builder = MediaSession.Builder(this, local)
            .setCallback(SessionCallback())
            .setCustomLayout(ImmutableList.of(close))
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

    /**
     * Media3 lets the notification go once playback pauses (it can then be
     * swiped away, and goes after a while). Keeping the service in the
     * foreground while songs are queued keeps the controls up until Close.
     */
    override fun onUpdateNotification(session: MediaSession, startInForegroundRequired: Boolean) {
        val player = session.player
        val queued = player.mediaItemCount > 0 && player.playbackState != Player.STATE_IDLE
        super.onUpdateNotification(session, startInForegroundRequired || queued)
    }

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
        /** Every controller may use Close, the notification's included. */
        override fun onConnect(
            session: MediaSession,
            controller: MediaSession.ControllerInfo,
        ): MediaSession.ConnectionResult {
            val commands = MediaSession.ConnectionResult.DEFAULT_SESSION_COMMANDS.buildUpon().add(CLOSE).build()
            return MediaSession.ConnectionResult.AcceptedResultBuilder(session)
                .setAvailableSessionCommands(commands)
                .build()
        }

        /** Close: stop, forget the queue, and let the notification go. */
        override fun onCustomCommand(
            session: MediaSession,
            controller: MediaSession.ControllerInfo,
            customCommand: SessionCommand,
            args: Bundle,
        ): ListenableFuture<SessionResult> {
            if (customCommand.customAction == CLOSE.customAction) {
                stopRemote()
                session.player.stop()
                session.player.clearMediaItems()
                return Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
            }
            return super.onCustomCommand(session, controller, customCommand, args)
        }

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
