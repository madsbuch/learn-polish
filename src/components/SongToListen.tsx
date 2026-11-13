import { useEffect, useState } from "react";

import { Button, Card, IconButton, Inline, Stack, Text } from "../design";
import { api } from "../lib/api";
import { currentItem, formatTime, libraryQueue, playQueue, preferredTake, toggle, usePlayer, type QueueItem } from "../lib/player";
import { Cover } from "./Cover";
import { PlayerIcon } from "./icons";
import { Lyrics } from "./Lyrics";

/** The take of a song to listen to: its favourite take, once it is ready. */
export function useSongItem(songId: number | null): { item: QueueItem | null; loading: boolean } {
  const [item, setItem] = useState<QueueItem | null>(null);
  const [loading, setLoading] = useState(songId !== null);
  useEffect(() => {
    if (songId === null) return;
    let cancelled = false;
    api
      .songsList()
      .then((songs) => {
        if (cancelled) return;
        const song = songs.find((entry) => entry.id === songId);
        const track = song && song.status === "ready" ? preferredTake(song) : null;
        setItem(song && track ? { song, track } : null);
      })
      .catch(() => setItem(null))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [songId]);
  return { item, loading };
}

/** Plays or pauses one song, starting it if something else is playing. */
export function playSong(item: QueueItem, isCurrent: boolean) {
  if (isCurrent) toggle();
  else playQueue(libraryQueue([item.song], item.track), 0);
}

/**
 * A listening block's song: play it, and reveal the lyrics only when asked —
 * the point is to understand it by ear.
 */
export function SongToListen({ songId }: { songId: number }) {
  const { item, loading } = useSongItem(songId);
  const player = usePlayer();
  const [showLyrics, setShowLyrics] = useState(false);
  if (loading) return null;
  if (!item) {
    return (
      <Card>
        <Text tone="dim">The song is still being made — Suno usually needs 2–4 minutes.</Text>
      </Card>
    );
  }
  const current = currentItem(player)?.track.id === item.track.id;
  const playing = current && player.playing;
  return (
    <Card>
      <Stack gap={3}>
        <Inline gap={3}>
          <Cover path={item.track.imagePath} size={64} />
          <div className="grow">
            <Text weight="bold" as="div">
              {item.song.title}
            </Text>
            <Text variant="small" tone="dim" as="div">
              Take {item.track.ord + 1} · {formatTime(item.track.duration)}
            </Text>
          </div>
          <IconButton
            variant="primary"
            size="lg"
            icon={playing ? <PlayerIcon.pause size={22} /> : <PlayerIcon.play size={22} />}
            label={playing ? "Pause" : "Play the song"}
            onClick={() => playSong(item, current)}
          />
        </Inline>
        <Button variant="plain" size="sm" onClick={() => setShowLyrics((value) => !value)}>
          {showLyrics ? "Hide the lyrics" : "Show the lyrics"}
        </Button>
        {showLyrics ? <Lyrics sections={item.song.sections} /> : null}
      </Stack>
    </Card>
  );
}
