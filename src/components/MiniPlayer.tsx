import { IconButton, Text } from "../design";
import { currentItem, next, toggle, usePlayer } from "../lib/player";
import { Cover } from "./Cover";
import { PlayerIcon } from "./icons";

/** The player bar: the song in reach on every tab, just above the tab bar. */
export function MiniPlayer({ onOpen }: { onOpen: () => void }) {
  const player = usePlayer();
  const item = currentItem(player);
  if (!item) return null;
  const duration = player.duration || item.track.duration;
  const pct = duration > 0 ? Math.min(100, (player.position / duration) * 100) : 0;

  return (
    <div className="mini-player fade-in">
      <span className="mini-progress" style={{ width: `${pct}%` }} />
      <button className="mini-open" onClick={onOpen} aria-label="Open the player">
        <Cover path={item.track.imagePath} size={44} />
        <span className="grow">
          <Text weight="bold" ellipsis>
            {item.song.title}
          </Text>
          <Text variant="small" tone={player.cast ? "brand" : "dim"} ellipsis>
            {player.cast ? `On ${player.cast.device.name}` : `„${item.song.chorusPl}”`}
          </Text>
        </span>
      </button>
      <IconButton
        icon={player.playing ? <PlayerIcon.pause size={20} /> : <PlayerIcon.play size={20} />}
        label={player.playing ? "Pause" : "Play"}
        onClick={toggle}
      />
      <IconButton icon={<PlayerIcon.next />} label="Next song" onClick={next} />
    </div>
  );
}
