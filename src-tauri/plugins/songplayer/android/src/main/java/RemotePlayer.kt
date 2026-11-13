package com.polski.songplayer

import android.os.Handler
import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.DeviceInfo
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import org.json.JSONObject
import java.io.BufferedReader
import java.io.InputStreamReader
import java.net.InetSocketAddress
import java.net.Socket
import java.net.URLEncoder
import java.util.concurrent.Executors

/**
 * Talks to the app's Rust core, which drives the Sonos speaker.
 *
 * A raw socket on 127.0.0.1 rather than HttpURLConnection: the app's network
 * security config may forbid cleartext HTTP, and this never leaves the phone.
 */
class RemoteClient(private val port: Int, private val token: String) {
    class Gone(message: String) : Exception(message)

    private fun get(path: String, query: Map<String, String>): String {
        val params = (query + ("token" to token)).entries.joinToString("&") { (k, v) ->
            "${URLEncoder.encode(k, "UTF-8")}=${URLEncoder.encode(v, "UTF-8")}"
        }
        Socket().use { socket ->
            socket.connect(InetSocketAddress("127.0.0.1", port), 2000)
            socket.soTimeout = 8000
            val request = "GET $path?$params HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
            socket.getOutputStream().apply {
                write(request.toByteArray(Charsets.UTF_8))
                flush()
            }
            val reader = BufferedReader(InputStreamReader(socket.getInputStream(), Charsets.UTF_8))
            val status = reader.readLine() ?: throw Exception("No answer")
            // Skip headers; the body follows the blank line.
            while (true) {
                val line = reader.readLine() ?: break
                if (line.isEmpty()) break
            }
            val body = reader.readText()
            if (status.contains(" 409 ")) throw Gone(body)
            if (!status.contains(" 200 ")) throw Exception(status)
            return body
        }
    }

    fun state(): JSONObject = JSONObject(get("/control/state", emptyMap()))

    fun command(action: String, value: Double? = null) {
        val query = mutableMapOf("action" to action)
        if (value != null) query["value"] = value.toString()
        get("/control/command", query)
    }
}

/**
 * A stand-in player for the media session while a Sonos speaker plays: it
 * shows what the speaker is playing and forwards every button press to it,
 * including the phone's volume keys.
 */
class RemotePlayer(
    looper: Looper,
    private val client: RemoteClient,
    private val deviceName: String,
    private val onEnded: () -> Unit,
) : SimpleBasePlayer(looper) {
    private val handler = Handler(looper)
    private val io = Executors.newSingleThreadExecutor()
    private var latest: JSONObject? = null
    private var released = false
    private var misses = 0

    private val poll = object : Runnable {
        override fun run() {
            if (released) return
            io.execute {
                val result = try {
                    Result.success(client.state())
                } catch (error: Exception) {
                    Result.failure(error)
                }
                handler.post { apply(result) }
            }
            handler.postDelayed(this, 1000)
        }
    }

    init {
        handler.post(poll)
    }

    private fun apply(result: Result<JSONObject>) {
        if (released) return
        result.onSuccess {
            misses = 0
            latest = it
            invalidateState()
        }
        result.onFailure { error ->
            // The core says the cast is over, or has gone quiet for a while.
            misses += if (error is RemoteClient.Gone) 3 else 1
            if (misses >= 3) onEnded()
        }
    }

    override fun getState(): State {
        val commands = Player.Commands.Builder()
            .addAll(
                COMMAND_PLAY_PAUSE,
                COMMAND_STOP,
                COMMAND_SEEK_TO_NEXT,
                COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
                COMMAND_SEEK_TO_PREVIOUS,
                COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
                COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
                COMMAND_SEEK_TO_MEDIA_ITEM,
                COMMAND_GET_CURRENT_MEDIA_ITEM,
                COMMAND_GET_TIMELINE,
                COMMAND_GET_METADATA,
                COMMAND_GET_DEVICE_VOLUME,
                COMMAND_SET_DEVICE_VOLUME_WITH_FLAGS,
                COMMAND_ADJUST_DEVICE_VOLUME_WITH_FLAGS,
            )
            .build()
        val builder = State.Builder()
            .setAvailableCommands(commands)
            // Remote playback: the volume keys move the speaker's volume.
            .setDeviceInfo(
                DeviceInfo.Builder(DeviceInfo.PLAYBACK_TYPE_REMOTE)
                    .setMinVolume(0)
                    .setMaxVolume(100)
                    .build(),
            )
        val state = latest ?: return builder.setPlaybackState(STATE_IDLE).build()

        val items = state.optJSONArray("items")
        val index = state.optInt("index", -1)
        val playing = state.optBoolean("playing")
        val durationUs = (state.optDouble("duration", 0.0) * 1_000_000).toLong()
        val playlist = mutableListOf<MediaItemData>()
        if (items != null) {
            for (i in 0 until items.length()) {
                val item = items.getJSONObject(i)
                val id = item.optLong("trackId")
                val metadata = MediaMetadata.Builder()
                    .setTitle(item.optString("title"))
                    .setArtist(item.optString("artist"))
                    .setAlbumTitle("Playing on $deviceName")
                    .build()
                playlist.add(
                    MediaItemData.Builder(id)
                        .setMediaItem(
                            MediaItem.Builder()
                                .setMediaId(id.toString())
                                .setMediaMetadata(metadata)
                                .build(),
                        )
                        .setDurationUs(if (i == index && durationUs > 0) durationUs else C.TIME_UNSET)
                        .build(),
                )
            }
        }
        if (playlist.isEmpty() || index < 0) {
            return builder.setPlaybackState(STATE_IDLE).build()
        }
        val positionMs = (state.optDouble("position", 0.0) * 1000).toLong()
        val volume = state.optInt("volume", -1)
        builder
            .setPlaylist(playlist)
            .setCurrentMediaItemIndex(index.coerceIn(0, playlist.size - 1))
            // Between polls, let the position run on by itself.
            .setContentPositionMs(
                PositionSupplier.getExtrapolating(positionMs, if (playing) 1f else 0f),
            )
            .setPlayWhenReady(playing, PLAY_WHEN_READY_CHANGE_REASON_REMOTE)
            .setPlaybackState(STATE_READY)
            .setRepeatMode(
                when (state.optString("repeat")) {
                    "one" -> REPEAT_MODE_ONE
                    "off" -> REPEAT_MODE_OFF
                    else -> REPEAT_MODE_ALL
                },
            )
        if (volume >= 0) builder.setDeviceVolume(volume)
        return builder.build()
    }

    /** Sends a command, then waits for the speaker's new state. */
    private fun send(action: String, value: Double? = null): ListenableFuture<*> {
        val done = SettableFuture.create<Any?>()
        io.execute {
            val result = try {
                client.command(action, value)
                Result.success(client.state())
            } catch (error: Exception) {
                Result.failure(error)
            }
            handler.post {
                apply(result)
                done.set(null)
            }
        }
        return done
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> =
        send(if (playWhenReady) "play" else "pause")

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
        val current = latest?.optInt("index", -1) ?: -1
        return if (mediaItemIndex >= 0 && mediaItemIndex != current) {
            send("track", mediaItemIndex.toDouble())
        } else {
            val seconds = if (positionMs == C.TIME_UNSET) 0.0 else positionMs / 1000.0
            send("seek", seconds)
        }
    }

    override fun handleStop(): ListenableFuture<*> = send("stop")

    override fun handleSetDeviceVolume(deviceVolume: Int, flags: Int): ListenableFuture<*> =
        send("volume", deviceVolume.toDouble())

    override fun handleIncreaseDeviceVolume(flags: Int): ListenableFuture<*> =
        send("volume", ((latest?.optInt("volume", 0) ?: 0) + 2).toDouble())

    override fun handleDecreaseDeviceVolume(flags: Int): ListenableFuture<*> =
        send("volume", ((latest?.optInt("volume", 0) ?: 0) - 2).toDouble())

    override fun handleRelease(): ListenableFuture<*> {
        released = true
        handler.removeCallbacks(poll)
        io.shutdown()
        return Futures.immediateVoidFuture()
    }
}
