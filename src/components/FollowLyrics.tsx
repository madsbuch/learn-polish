import { useEffect, useRef, useState } from "react";

import { api } from "../lib/api";
import { sectionLabel } from "../lib/lyrics";
import { livePosition, seek, usePlayer, type QueueItem } from "../lib/player";
import type { TimedLine, TrackId } from "../lib/types";

/**
 * Lyrics that follow the music: the line being sung is highlighted and kept
 * in view, and tapping a line jumps there. Without Suno's timings the lyrics
 * are shown whole, so there is always something to read along with.
 */
/** Who spoke last before line `i` — a name shows only when the speaker changes. */
function lastSpeaker(lines: { speaker?: string | null; section: string | null }[], i: number): string | null {
  for (let j = i - 1; j >= 0; j--) {
    if (lines[j].section) return j === i - 1 ? null : (lines[j].speaker ?? null);
    if (lines[j].speaker) return lines[j].speaker ?? null;
  }
  return null;
}

export function FollowLyrics({ item }: { item: QueueItem }) {
  const player = usePlayer();
  const trackId = item.track.id;
  const [fetched, setFetched] = useState<{ id: TrackId; lines: TimedLine[] | null } | null>(null);
  const [, tick] = useState(0);
  const box = useRef<HTMLDivElement>(null);
  const currentRef = useRef<HTMLButtonElement>(null);

  // Older songs get their timings the first time they are played.
  useEffect(() => {
    if (item.track.lines || fetched?.id === trackId) return;
    let cancelled = false;
    api
      .songTrackLyrics(trackId)
      .then((lines) => !cancelled && setFetched({ id: trackId, lines }))
      .catch(() => !cancelled && setFetched({ id: trackId, lines: null }));
    return () => {
      cancelled = true;
    };
  }, [trackId, item.track.lines, fetched?.id]);

  // Re-render a few times a second so the highlight keeps up between reports.
  useEffect(() => {
    if (!player.playing) return;
    const timer = window.setInterval(() => tick((n) => n + 1), 200);
    return () => window.clearInterval(timer);
  }, [player.playing]);

  const lines = item.track.lines ?? (fetched?.id === trackId ? fetched.lines : null);
  const position = livePosition(player);
  let current = -1;
  if (lines) {
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].start <= position + 0.15) current = i;
      else break;
    }
  }

  // Keep the sung line in the upper part of the box, without moving the page.
  useEffect(() => {
    const container = box.current;
    const line = currentRef.current;
    if (!container || !line) return;
    const target = Math.max(0, Math.round(line.offsetTop - container.clientHeight * 0.35));
    // One smooth scroll per line change, and none when it is already there.
    if (Math.abs(container.scrollTop - target) < 2) return;
    container.scrollTo({ top: target, behavior: "smooth" });
  }, [current, trackId]);

  if (!lines || lines.length === 0) {
    return (
      <div className="follow-lyrics static" ref={box}>
        {item.song.sections.map((section, i) => (
          <div key={i} className="follow-section">
            <div className="t-eyebrow">{sectionLabel(section.kind)}</div>
            {section.lines.map((line, j) => (
              <div key={j} className="follow-line" data-lang="pair">
                {section.kind !== "chorus" && line.en ? <span className="en">{line.en}</span> : null}
                <span className="pl">{line.pl}</span>
                {section.kind === "chorus" && line.en ? <span className="gloss">{line.en}</span> : null}
              </div>
            ))}
          </div>
        ))}
      </div>
    );
  }

  return (
    <div className="follow-lyrics" ref={box}>
      {lines.map((line, i) => (
        <div key={i}>
          {line.section ? <div className="t-eyebrow follow-eyebrow">{line.section}</div> : null}
          {line.speaker && line.speaker !== lastSpeaker(lines, i) ? (
            <div className="follow-speaker">{line.speaker}</div>
          ) : null}
          <button
            ref={i === current ? currentRef : undefined}
            className="follow-line"
            data-lang={line.lang || "other"}
            data-state={i === current ? "current" : i < current ? "past" : "next"}
            onClick={() => seek(line.start)}
          >
            <span className={line.lang === "en" ? "en" : "pl"}>{line.text}</span>
            {line.gloss ? <span className="gloss">{line.gloss}</span> : null}
          </button>
        </div>
      ))}
      <div className="follow-tail" />
    </div>
  );
}
