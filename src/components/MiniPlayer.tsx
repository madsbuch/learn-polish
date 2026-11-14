import { useEffect, useRef, useState, type CSSProperties, type PointerEvent } from "react";

import { IconButton, Text } from "../design";
import { currentItem, next, stopPlayback, toggle, usePlayer } from "../lib/player";
import { Cover } from "./Cover";
import { PlayerIcon } from "./icons";

/** How far right (as a share of its width) the bar must go to be dismissed. */
const DISMISS_AT = 0.35;

interface Drag {
  x: number;
  y: number;
  /** Decided on the first few pixels: a sideways swipe or a scroll. */
  sideways: boolean | null;
}

/**
 * The player bar: the song in reach on every tab, just above the tab bar.
 * Swiping it to the right dismisses it and stops the music.
 */
export function MiniPlayer({ onOpen }: { onOpen: () => void }) {
  const player = usePlayer();
  const item = currentItem(player);
  const bar = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  /** A swipe just ended: the click it would otherwise produce is swallowed. */
  const swiped = useRef(false);
  const [offset, setOffset] = useState(0);
  const [dragging, setDragging] = useState(false);

  // A new song brings the bar back in place.
  useEffect(() => {
    if (item) setOffset(0);
  }, [item?.track.id]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!item) return null;
  const duration = player.duration || item.track.duration;
  const pct = duration > 0 ? Math.min(100, (player.position / duration) * 100) : 0;
  const width = bar.current?.offsetWidth || 1;

  function down(event: PointerEvent<HTMLDivElement>) {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    drag.current = { x: event.clientX, y: event.clientY, sideways: null };
    swiped.current = false;
  }

  function move(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current) return;
    const dx = event.clientX - current.x;
    const dy = event.clientY - current.y;
    if (current.sideways === null) {
      if (Math.abs(dx) < 8 && Math.abs(dy) < 8) return;
      current.sideways = Math.abs(dx) > Math.abs(dy);
      if (!current.sideways) {
        drag.current = null;
        return;
      }
      bar.current?.setPointerCapture(event.pointerId);
      setDragging(true);
    }
    swiped.current = true;
    setOffset(Math.max(0, dx));
  }

  function up() {
    const current = drag.current;
    drag.current = null;
    if (!current?.sideways) return;
    setDragging(false);
    if (offset > width * DISMISS_AT) {
      // Off to the right, then the music stops and the bar is gone.
      setOffset(width + 32);
      window.setTimeout(stopPlayback, 200);
    } else {
      setOffset(0);
    }
  }

  return (
    <div
      ref={bar}
      className="mini-player fade-in"
      data-dragging={dragging}
      style={
        {
          "--swipe": `${offset}px`,
          "--swipe-fade": String(Math.max(0.2, 1 - offset / width)),
        } as CSSProperties
      }
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
      onClickCapture={(event) => {
        if (swiped.current) {
          swiped.current = false;
          event.stopPropagation();
          event.preventDefault();
        }
      }}
    >
      <span className="mini-progress" style={{ width: `${pct}%` }} />
      <button className="mini-open" onClick={onOpen} aria-label="Open the player">
        <Cover fileId={item.track.imageFileId} size={44} />
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
