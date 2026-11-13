package com.polski.songplayer

import android.app.Activity
import android.content.ComponentName
import android.content.Context
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Handler
import android.os.Looper
import android.webkit.WebView
import androidx.core.content.ContextCompat
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.common.util.concurrent.ListenableFuture
import java.io.File

@InvokeArg
class QueueItem {
    lateinit var id: String
    lateinit var path: String
    var title: String = ""
    var artist: String = ""
    var artwork: String? = null
}

@InvokeArg
class SetQueueArgs {
    lateinit var items: List<QueueItem>
    var index: Int = 0
    var position: Double = 0.0
    var play: Boolean = true
    var repeat: String = "all"
}

@InvokeArg
class SeekArgs {
    var seconds: Double = 0.0
}

@InvokeArg
class SkipArgs {
    var index: Int = 0
}

@InvokeArg
class RepeatArgs {
    var repeat: String = "all"
}

@InvokeArg
class RemoteArgs {
    var port: Int = 0
    lateinit var token: String
    var deviceName: String = "Sonos"
}

@InvokeArg
class MulticastArgs {
    var enabled: Boolean = false
}

/**
 * The app's bridge to [PlaybackService]. Playback state is pushed to the app
 * as "state" events: on every change, and once a second while playing.
 */
@TauriPlugin
class PlayerPlugin(private val activity: Activity) : Plugin(activity) {
    private val main = Handler(Looper.getMainLooper())
    private var future: ListenableFuture<MediaController>? = null
    private var controller: MediaController? = null
    private val waiting = mutableListOf<(MediaController) -> Unit>()
    private var multicastLock: WifiManager.MulticastLock? = null

    private val ticker = object : Runnable {
        override fun run() {
            emitState()
            if (controller?.isPlaying == true) main.postDelayed(this, 1000)
        }
    }

    override fun load(webView: WebView) {
        super.load(webView)
        main.post { connect() }
    }

    private fun connect() {
        if (future != null) return
        val token = SessionToken(activity, ComponentName(activity, PlaybackService::class.java))
        val pending = MediaController.Builder(activity, token).buildAsync()
        future = pending
        pending.addListener({
            val ready = try {
                pending.get()
            } catch (error: Exception) {
                future = null
                return@addListener
            }
            controller = ready
            ready.addListener(object : Player.Listener {
                override fun onEvents(player: Player, events: Player.Events) {
                    emitState()
                    main.removeCallbacks(ticker)
                    if (player.isPlaying) main.postDelayed(ticker, 1000)
                }
            })
            waiting.forEach { it(ready) }
            waiting.clear()
            emitState()
        }, ContextCompat.getMainExecutor(activity))
    }

    /** Runs on the main thread once the controller is connected. */
    private fun withController(invoke: Invoke, action: (MediaController) -> Any?) {
        main.post {
            val run = { c: MediaController ->
                try {
                    when (val result = action(c)) {
                        is JSObject -> invoke.resolve(result)
                        else -> invoke.resolve()
                    }
                } catch (error: Exception) {
                    invoke.reject(error.message ?: "Player error")
                }
            }
            val ready = controller
            if (ready != null) {
                run(ready)
            } else {
                waiting.add(run)
                connect()
            }
        }
    }

    private fun stateObject(c: MediaController): JSObject {
        val state = JSObject()
        state.put("playing", c.isPlaying || (c.playWhenReady && c.playbackState == Player.STATE_BUFFERING))
        state.put("index", c.currentMediaItemIndex)
        state.put("id", c.currentMediaItem?.mediaId ?: "")
        state.put("position", c.currentPosition / 1000.0)
        val duration = c.duration
        state.put("duration", if (duration == C.TIME_UNSET) 0.0 else duration / 1000.0)
        state.put("ended", c.playbackState == Player.STATE_ENDED)
        state.put("count", c.mediaItemCount)
        state.put("remote", PlaybackService.instance?.isRemote == true)
        return state
    }

    private fun emitState() {
        val c = controller ?: return
        trigger("state", stateObject(c))
    }

    private fun repeatMode(name: String): Int = when (name) {
        "one" -> Player.REPEAT_MODE_ONE
        "off" -> Player.REPEAT_MODE_OFF
        else -> Player.REPEAT_MODE_ALL
    }

    @Command
    fun setQueue(invoke: Invoke) {
        val args = invoke.parseArgs(SetQueueArgs::class.java)
        withController(invoke) { c ->
            PlaybackService.instance?.stopRemote()
            val items = args.items.map { item ->
                val uri = Uri.fromFile(File(item.path))
                val metadata = MediaMetadata.Builder()
                    .setTitle(item.title)
                    .setArtist(item.artist)
                    .setAlbumTitle("Polski")
                    .setArtworkUri(item.artwork?.let { Uri.fromFile(File(it)) })
                    .build()
                MediaItem.Builder()
                    .setMediaId(item.id)
                    .setUri(uri)
                    // Survives the hand-over to the service; see onAddMediaItems.
                    .setRequestMetadata(MediaItem.RequestMetadata.Builder().setMediaUri(uri).build())
                    .setMediaMetadata(metadata)
                    .build()
            }
            val start = args.index.coerceIn(0, maxOf(0, items.size - 1))
            c.setMediaItems(items, start, (args.position * 1000).toLong())
            c.repeatMode = repeatMode(args.repeat)
            c.prepare()
            if (args.play) c.play()
            null
        }
    }

    @Command
    fun play(invoke: Invoke) = withController(invoke) { it.play() }

    @Command
    fun pause(invoke: Invoke) = withController(invoke) { it.pause() }

    @Command
    fun toggle(invoke: Invoke) = withController(invoke) { c ->
        if (c.isPlaying) c.pause() else {
            if (c.playbackState == Player.STATE_ENDED) c.seekToDefaultPosition(0)
            c.play()
        }
    }

    @Command
    fun next(invoke: Invoke) = withController(invoke) { it.seekToNext() }

    @Command
    fun previous(invoke: Invoke) = withController(invoke) { it.seekToPrevious() }

    @Command
    fun seekTo(invoke: Invoke) {
        val args = invoke.parseArgs(SeekArgs::class.java)
        withController(invoke) { it.seekTo((args.seconds * 1000).toLong()) }
    }

    @Command
    fun skipTo(invoke: Invoke) {
        val args = invoke.parseArgs(SkipArgs::class.java)
        withController(invoke) { c ->
            if (args.index in 0 until c.mediaItemCount) {
                c.seekToDefaultPosition(args.index)
                c.play()
            }
        }
    }

    @Command
    fun setRepeat(invoke: Invoke) {
        val args = invoke.parseArgs(RepeatArgs::class.java)
        withController(invoke) { c ->
            c.repeatMode = repeatMode(args.repeat)
            null
        }
    }

    @Command
    fun stop(invoke: Invoke) = withController(invoke) { c ->
        c.stop()
        c.clearMediaItems()
    }

    @Command
    fun getState(invoke: Invoke) = withController(invoke) { stateObject(it) }

    /** Lets the lock screen and notification control a Sonos speaker. */
    @Command
    fun remoteStart(invoke: Invoke) {
        val args = invoke.parseArgs(RemoteArgs::class.java)
        withController(invoke) {
            val service = PlaybackService.instance ?: throw Exception("The player is not running.")
            service.startRemote(args.port, args.token, args.deviceName)
        }
    }

    @Command
    fun remoteStop(invoke: Invoke) = withController(invoke) {
        PlaybackService.instance?.stopRemote()
    }

    /** Android drops multicast replies unless asked; needed to find speakers. */
    @Command
    fun multicast(invoke: Invoke) {
        val args = invoke.parseArgs(MulticastArgs::class.java)
        try {
            if (args.enabled) {
                if (multicastLock == null) {
                    val wifi = activity.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
                    multicastLock = wifi.createMulticastLock("polski:discovery").apply {
                        setReferenceCounted(false)
                        acquire()
                    }
                }
            } else {
                multicastLock?.let { if (it.isHeld) it.release() }
                multicastLock = null
            }
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "Could not change multicast")
        }
    }
}
