import { convertFileSrc } from "@tauri-apps/api/core";

import { currentItem, next, toggle, usePlayer } from "../lib/player";
import { PlayerIcon } from "./icons";

/** Keeps the song in reach on every tab, just above the tab bar. */
export function MiniPlayer({ onOpen }: { onOpen: () => void }) {
  const player = usePlayer();
  const item = currentItem(player);
  if (!item) return null;
  const duration = player.duration || item.track.duration;
  const pct = duration > 0 ? Math.min(100, (player.position / duration) * 100) : 0;

  return (
    <div className="mini-player fade-in">
      <span className="mini-progress" style={{ width: `${pct}%` }} />
      <button className="row grow mini-open" onClick={onOpen}>
        {item.track.imagePath ? (
          <img className="cover" src={convertFileSrc(item.track.imagePath)} alt="" />
        ) : (
          <span className="cover placeholder">♪</span>
        )}
        <span className="grow" style={{ minWidth: 0 }}>
          <span className="mini-title">{item.song.title}</span>
          <span className="mini-sub">„{item.song.chorusPl}”</span>
        </span>
      </button>
      <button className="icon-btn" onClick={toggle} aria-label={player.playing ? "Pause" : "Play"}>
        {player.playing ? <PlayerIcon.pause size={18} /> : <PlayerIcon.play size={18} />}
      </button>
      <button className="icon-btn" onClick={next} aria-label="Next song">
        <PlayerIcon.next />
      </button>
    </div>
  );
}
