import { useEffect, useState } from "react";

import { CastPicker } from "../components/CastPicker";
import { Cover } from "../components/Cover";
import { FollowLyrics } from "../components/FollowLyrics";
import { MoreIcon, PlayerIcon } from "../components/icons";
import { AddToPlaylist, useTrackExport } from "../components/TrackActions";
import {
  Action,
  ActionBar,
  Banner,
  Button,
  EmptyState,
  Hero,
  IconButton,
  Inline,
  List,
  ListItem,
  Screen,
  Segmented,
  Stack,
  Text,
} from "../design";
import {
  currentItem,
  cycleRepeat,
  formatTime,
  keepScreenAwake,
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

  // Following the lyrics means not touching the phone: keep the screen on
  // while a song plays here. Pausing or closing the player lets it sleep.
  useEffect(() => {
    keepScreenAwake(player.playing);
  }, [player.playing]);
  useEffect(() => () => keepScreenAwake(false), []);

  if (!item) {
    return (
      <Screen title="Now playing" onClose={onClose} nav={false}>
        <EmptyState icon="🎵" title="Nothing is playing" />
      </Screen>
    );
  }

  const cast = player.cast;
  const duration = player.duration || item.track.duration;

  return (
    <Screen
      title={cast ? `Playing on ${cast.device.name}` : "Now playing"}
      onClose={onClose}
      nav={false}
    >
      {player.error ? <Banner tone="error">{player.error}</Banner> : null}
      {message ? <Banner tone={message.error ? "error" : "ok"}>{message.text}</Banner> : null}

      <Hero className="player-card">
        <div className="player-cover">
          <Cover fileId={item.track.imageFileId} size={232} />
        </div>
        <Stack gap={1} align="center">
          <h2 className="t-center">{item.song.title}</h2>
          <Text weight="bold" align="center" selectable>
            „{item.song.chorusPl}”
          </Text>
          <Text variant="small" tone="dim" align="center">
            {item.song.chorusEn} · take {item.track.ord + 1}
          </Text>
        </Stack>

        <Stack gap={1}>
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
          <Inline justify="between">
            <Text variant="caption" tone="dim">
              {formatTime(player.position)}
            </Text>
            <Text variant="caption" tone="dim">
              {formatTime(duration)}
            </Text>
          </Inline>
        </Stack>

        <div className="transport">
          <IconButton
            variant="inverse"
            icon={
              <span className="icon-mark">
                <PlayerIcon.repeat />
                {player.repeat === "one" ? <b>1</b> : null}
              </span>
            }
            label={`Repeat: ${player.repeat === "one" ? "this song" : player.repeat === "all" ? "all" : "off"}`}
            off={player.repeat === "off"}
            onClick={cycleRepeat}
          />
          <IconButton variant="inverse" icon={<PlayerIcon.previous />} label="Previous" onClick={previous} />
          <IconButton
            variant="inverse-solid"
            size="xl"
            icon={player.playing ? <PlayerIcon.pause size={26} /> : <PlayerIcon.play size={26} />}
            label={player.playing ? "Pause" : "Play"}
            onClick={toggle}
          />
          <IconButton variant="inverse" icon={<PlayerIcon.next />} label="Next" onClick={next} />
          <IconButton
            variant="inverse"
            icon={<PlayerIcon.speaker />}
            label="Play on a speaker"
            disabled={Boolean(cast)}
            onClick={() => setPicking(true)}
          />
        </div>

        {cast ? (
          <div className="cast-bar">
            <PlayerIcon.speaker />
            {cast.detached ? (
              <Text variant="small" className="grow">
                {cast.device.name} is playing something else.
              </Text>
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
              <Text variant="small" className="grow">
                {cast.device.name}
              </Text>
            )}
            <Button variant="inverse" size="sm" onClick={() => void stopCasting()}>
              Stop
            </Button>
          </div>
        ) : null}
      </Hero>

      <ActionBar>
        <Action icon={<PlayerIcon.addToPlaylist />} label="Playlist" onClick={() => setAdding(true)} />
        {exporter.canShare ? (
          <Action
            icon={<PlayerIcon.share />}
            label="Share"
            loading={exporter.busy === "share"}
            disabled={exporter.busy !== null}
            onClick={() => exporter.share(item)}
          />
        ) : null}
        <Action
          icon={<PlayerIcon.download />}
          label="Save MP3"
          loading={exporter.busy === "save"}
          disabled={exporter.busy !== null}
          onClick={() => exporter.save(item)}
        />
        <Action icon={<MoreIcon />} label="Song" onClick={() => onOpenSong(item.song.id)} />
      </ActionBar>

      <Stack gap={3}>
        <Segmented
          value={panel}
          onChange={setPanel}
          options={[
            { value: "lyrics", label: "Lyrics" },
            { value: "queue", label: `Up next · ${player.queue.length}` },
          ]}
        />
        {panel === "lyrics" ? (
          <FollowLyrics item={item} />
        ) : (
          <List>
            {player.queue.map((entry, index) => (
              <ListItem
                key={`${entry.track.id}-${index}`}
                leading={<Cover fileId={entry.track.imageFileId} size={44} />}
                title={entry.song.title}
                subtitle={`${index === player.index ? "Playing · " : ""}take ${entry.track.ord + 1} · ${formatTime(entry.track.duration)}`}
                selected={index === player.index}
                chevron={false}
                onClick={() => skipTo(index)}
              />
            ))}
          </List>
        )}
      </Stack>

      {picking ? <CastPicker fallbackQueue={player.queue} onClose={() => setPicking(false)} /> : null}
      {adding ? (
        <AddToPlaylist
          trackId={item.track.id}
          onClose={() => setAdding(false)}
          onDone={(text) => setMessage({ text, error: false })}
        />
      ) : null}
    </Screen>
  );
}
