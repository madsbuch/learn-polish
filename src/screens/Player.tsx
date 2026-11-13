import { useState } from "react";

import { CastPicker } from "../components/CastPicker";
import { Cover } from "../components/Cover";
import { FollowLyrics } from "../components/FollowLyrics";
import { PlayerIcon } from "../components/icons";
import { AddToPlaylist, useTrackExport } from "../components/TrackActions";
import { Banner, Spinner } from "../components/ui";
import {
  currentItem,
  cycleRepeat,
  formatTime,
  next,
  previous,
  seek,
  setCastVolume,
  skipTo,
  stopCasting,
  toggle,
  usePlayer,
} from "../lib/player";

/**
 * The player, on its own screen: what is playing, the controls, the lyrics
 * following along, and what comes next. Opened from the bar above the tabs.
 */
export function Player({
  onClose,
  onOpenSong,
}: {
  onClose: () => void;
  /** Shows the song's page in the music library. */
  onOpenSong: (songId: number) => void;
}) {
  const player = usePlayer();
  const item = currentItem(player);
  const [panel, setPanel] = useState<"lyrics" | "queue">("lyrics");
  const [picking, setPicking] = useState(false);
  const [adding, setAdding] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);
  const exporter = useTrackExport((text, error = false) => setMessage({ text, error }));

  if (!item) {
    return (
      <div className="scroll no-nav">
        <div className="row" style={{ marginBottom: 14 }}>
          <button className="icon-btn" onClick={onClose} aria-label="Close the player">
            ⌄
          </button>
        </div>
        <p className="faint center">Nothing is playing.</p>
      </div>
    );
  }

  const cast = player.cast;
  const duration = player.duration || item.track.duration;

  return (
    <div className="scroll no-nav player-screen">
      <div className="row between" style={{ marginBottom: 10 }}>
        <button className="icon-btn" onClick={onClose} aria-label="Close the player">
          ⌄
        </button>
        <span className="eyebrow" style={{ margin: 0 }}>
          {cast ? `Playing on ${cast.device.name}` : "Now playing"}
        </span>
        <button
          className="icon-btn"
          onClick={() => onOpenSong(item.song.id)}
          aria-label="Song details"
          title="Song details"
        >
          ⋯
        </button>
      </div>

      {player.error ? <Banner kind="error">{player.error}</Banner> : null}
      {message ? <Banner kind={message.error ? "error" : "info"}>{message.text}</Banner> : null}

      <div className="hero now-playing">
        <div className="center" style={{ marginBottom: 14 }}>
          <Cover path={item.track.imagePath} size={200} />
        </div>
        <div className="center">
          <div style={{ fontSize: 20, fontWeight: 760, lineHeight: 1.25 }}>{item.song.title}</div>
          <div className="selectable" style={{ fontSize: 14.5, fontWeight: 650, marginTop: 4 }}>
            „{item.song.chorusPl}”
          </div>
          <div style={{ fontSize: 12.5, opacity: 0.75 }}>
            {item.song.chorusEn} · take {item.track.ord + 1}
          </div>
        </div>

        <input
          className="scrubber"
          type="range"
          min={0}
          max={Math.max(1, duration)}
          step={0.5}
          value={Math.min(player.position, duration || player.position)}
          onChange={(event) => seek(Number(event.target.value))}
          aria-label="Position"
        />
        <div className="row between tiny" style={{ opacity: 0.75, marginTop: -2 }}>
          <span>{formatTime(player.position)}</span>
          <span>{formatTime(duration)}</span>
        </div>

        <div className="row transport">
          <button
            className="icon-btn ghost-light"
            onClick={cycleRepeat}
            aria-label={`Repeat: ${player.repeat}`}
            title={`Repeat: ${player.repeat}`}
            data-mode={player.repeat}
            style={{ opacity: player.repeat === "off" ? 0.55 : 1 }}
          >
            <PlayerIcon.repeat />
            {player.repeat === "one" ? <span className="tiny">1</span> : null}
          </button>
          <button className="icon-btn ghost-light" onClick={previous} aria-label="Previous">
            <PlayerIcon.previous />
          </button>
          <button className="play-btn" onClick={toggle} aria-label={player.playing ? "Pause" : "Play"}>
            {player.playing ? <PlayerIcon.pause size={26} /> : <PlayerIcon.play size={26} />}
          </button>
          <button className="icon-btn ghost-light" onClick={next} aria-label="Next">
            <PlayerIcon.next />
          </button>
          <button
            className="icon-btn ghost-light"
            onClick={() => setPicking(true)}
            aria-label="Play on a speaker"
            title="Play on a speaker"
            disabled={Boolean(cast)}
          >
            <PlayerIcon.speaker />
          </button>
        </div>

        {cast ? (
          <div className="cast-bar">
            <span aria-hidden="true">🔊</span>
            {cast.detached ? (
              <span className="grow small">{cast.device.name} is playing something else.</span>
            ) : cast.volume >= 0 ? (
              <input
                className="grow"
                type="range"
                min={0}
                max={100}
                value={cast.volume}
                onChange={(event) => setCastVolume(Number(event.target.value))}
                aria-label={`Volume on ${cast.device.name}`}
              />
            ) : (
              <span className="grow small">{cast.device.name}</span>
            )}
            <button className="cast-chip" onClick={() => void stopCasting()}>
              Stop
            </button>
          </div>
        ) : null}
      </div>

      <div className="row wrap take-actions" style={{ justifyContent: "center", marginBottom: 14 }}>
        <button className="chip" onClick={() => setAdding(true)}>
          + Playlist
        </button>
        {exporter.canShare ? (
          <button className="chip" disabled={exporter.busy !== null} onClick={() => exporter.share(item)}>
            {exporter.busy === "share" ? <Spinner /> : "↗ Share"}
          </button>
        ) : null}
        <button className="chip" disabled={exporter.busy !== null} onClick={() => exporter.save(item)}>
          {exporter.busy === "save" ? <Spinner /> : "⬇ Save MP3"}
        </button>
      </div>

      <div className="segmented" style={{ marginBottom: 12 }}>
        <button data-active={panel === "lyrics"} onClick={() => setPanel("lyrics")}>
          Lyrics
        </button>
        <button data-active={panel === "queue"} onClick={() => setPanel("queue")}>
          Up next · {player.queue.length}
        </button>
      </div>

      {panel === "lyrics" ? (
        <FollowLyrics item={item} />
      ) : (
        <div className="card flush">
          {player.queue.map((entry, index) => (
            <button
              key={`${entry.track.id}-${index}`}
              className="list-row"
              data-current={index === player.index}
              onClick={() => skipTo(index)}
            >
              <Cover path={entry.track.imagePath} size={40} />
              <span className="grow" style={{ minWidth: 0 }}>
                <span className="pl ellipsis">{entry.song.title}</span>
                <span className="tr ellipsis">
                  {index === player.index ? "Playing · " : ""}take {entry.track.ord + 1} ·{" "}
                  {formatTime(entry.track.duration)}
                </span>
              </span>
            </button>
          ))}
        </div>
      )}

      {picking ? <CastPicker fallbackQueue={player.queue} onClose={() => setPicking(false)} /> : null}
      {adding ? (
        <AddToPlaylist
          trackId={item.track.id}
          onClose={() => setAdding(false)}
          onDone={(text) => setMessage({ text, error: false })}
        />
      ) : null}
    </div>
  );
}
