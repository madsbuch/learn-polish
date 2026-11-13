import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";

import { Cover } from "../components/Cover";
import {
  ArrowDownIcon,
  ArrowUpIcon,
  CheckIcon,
  CloseIcon,
  MoreIcon,
  PlayerIcon,
  PlusIcon,
  SpeakerIcon,
} from "../components/icons";
import { Lyrics } from "../components/Lyrics";
import { AddToPlaylist, useTrackExport } from "../components/TrackActions";
import {
  Badge,
  Banner,
  Button,
  ConfirmButton,
  EmptyState,
  IconButton,
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  Segmented,
  Spinner,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import {
  currentItem,
  formatTime,
  libraryQueue,
  playQueue,
  syncLibrary,
  toggle,
  usePlayer,
  type QueueItem,
} from "../lib/player";
import type { Playlist, Settings, Song, TopicProgress } from "../lib/types";
import { SongComposer } from "./SongComposer";

/**
 * The music library: songs and playlists. Playing happens in the player
 * (the bar above the tabs, and the full screen it opens); this screen only
 * chooses what to play and manages the collection.
 */
type View =
  | { kind: "library" }
  | { kind: "song"; songId: number }
  | { kind: "playlist"; playlistId: number }
  | { kind: "compose" };

/** Every downloaded take, in library order. */
function allTakes(songs: Song[]): QueueItem[] {
  return songs
    .filter((song) => song.status === "ready")
    .flatMap((song) => song.tracks.map((track) => ({ song, track })));
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

export function Music({
  settings,
  topics,
  openSongId,
  onOpenSettings,
  onSongOpened,
  onOpenBlock,
}: {
  settings: Settings;
  topics: TopicProgress[];
  /** Set when the player asks to show a song's page. */
  openSongId: number | null;
  onOpenSettings: () => void;
  onSongOpened: () => void;
  /** Opens a block's page (a song's listening block). */
  onOpenBlock: (blockId: number) => void;
}) {
  const [songs, setSongs] = useState<Song[] | null>(null);
  const [playlists, setPlaylists] = useState<Playlist[] | null>(null);
  const [tab, setTab] = useState<"songs" | "playlists">("songs");
  const [view, setView] = useState<View>({ kind: "library" });
  const [selecting, setSelecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api
      .songsList()
      .then((list) => {
        setSongs(list);
        syncLibrary(list);
      })
      .catch((cause) => setError(errorMessage(cause)));
    api.playlistsList().then(setPlaylists).catch((cause) => setError(errorMessage(cause)));
  }, []);

  useEffect(refresh, [refresh]);

  // Songs finish in the background; the backend says when.
  useEffect(() => {
    const unlisten = listen("songs://changed", refresh);
    return () => {
      void unlisten.then((off) => off());
    };
  }, [refresh]);

  useEffect(() => {
    if (openSongId !== null) {
      setView({ kind: "song", songId: openSongId });
      onSongOpened();
    }
  }, [openSongId, onSongOpened]);

  const tracks = useMemo(() => {
    const map = new Map<number, QueueItem>();
    for (const item of allTakes(songs ?? [])) map.set(item.track.id, item);
    return map;
  }, [songs]);

  const message = (text: string, isError = false) => {
    if (isError) setError(text);
    else setNotice(text);
  };

  const back = () => {
    setView({ kind: "library" });
    setError(null);
    setNotice(null);
  };

  if (view.kind === "compose") {
    return (
      <SongComposer
        settings={settings}
        topics={topics}
        onCancel={back}
        onCreated={() => {
          back();
          refresh();
        }}
      />
    );
  }

  const feedback = (
    <>
      {error ? <Banner tone="error">{error}</Banner> : null}
      {notice ? <Banner tone="ok">{notice}</Banner> : null}
    </>
  );

  if (view.kind === "song") {
    const song = songs?.find((entry) => entry.id === view.songId);
    return (
      <Screen title="Song" onBack={back}>
        {feedback}
        {song ? (
          <SongPage
            song={song}
            library={songs ?? []}
            onChanged={refresh}
            onDeleted={() => {
              back();
              refresh();
            }}
            onMessage={message}
            onPractise={async () => {
              try {
                onOpenBlock((await api.blockForSong(song.id)).id);
              } catch (cause) {
                setError(errorMessage(cause));
              }
            }}
          />
        ) : songs === null ? (
          <Spinner />
        ) : (
          <EmptyState title="This song no longer exists" />
        )}
      </Screen>
    );
  }

  if (view.kind === "playlist") {
    const playlist = playlists?.find((entry) => entry.id === view.playlistId);
    return (
      <Screen title={playlist?.name ?? "Playlist"} onBack={back}>
        {feedback}
        {playlist ? (
          <PlaylistPage
            playlist={playlist}
            tracks={tracks}
            onChanged={(next) =>
              setPlaylists((current) =>
                (current ?? []).map((entry) => (entry.id === next.id ? next : entry)),
              )
            }
            onDeleted={() => {
              back();
              refresh();
            }}
            onError={setError}
          />
        ) : playlists === null ? (
          <Spinner />
        ) : (
          <EmptyState title="This playlist no longer exists" />
        )}
      </Screen>
    );
  }

  const ready = settings.hasKieKey && settings.hasApiKey;

  return (
    <Screen
      title="Music"
      info={
        <>
          <p>
            Short lines in English, then straight into Polish — and a Polish chorus that gets
            stuck in your head. Play them on repeat.
          </p>
          <p>
            Make one with <b>+</b>: pick a theme and a chorus sentence, the songwriter drafts the
            lyrics, and Suno sings two takes. Put the ones you like into playlists, and save or
            share any take as an MP3.
          </p>
        </>
      }
      actions={
        <IconButton
          variant="primary"
          icon={<PlusIcon />}
          label="New song"
          disabled={!ready}
          onClick={() => setView({ kind: "compose" })}
        />
      }
    >
      {feedback}

      {!ready ? (
        <Banner tone="info">
          {!settings.hasKieKey && !settings.hasApiKey
            ? "Making songs needs kie.ai and OpenRouter keys. "
            : !settings.hasKieKey
              ? "Making songs needs a kie.ai key. "
              : "Making songs needs an OpenRouter key. "}
          <Button variant="plain" onClick={onOpenSettings}>
            Open Settings
          </Button>
        </Banner>
      ) : null}

      <Segmented
        value={tab}
        onChange={(next) => {
          setTab(next);
          setSelecting(false);
        }}
        options={[
          { value: "songs", label: "Songs" },
          { value: "playlists", label: "Playlists" },
        ]}
      />

      {tab === "songs" ? (
        <SongList
          songs={songs}
          selecting={selecting}
          onSelecting={setSelecting}
          onOpen={(songId) => setView({ kind: "song", songId })}
          onCompose={ready ? () => setView({ kind: "compose" }) : undefined}
          onDeleted={refresh}
          onError={setError}
        />
      ) : (
        <PlaylistList
          playlists={playlists}
          tracks={tracks}
          onOpen={(playlistId) => setView({ kind: "playlist", playlistId })}
          onCreated={(playlist) => {
            setPlaylists((current) => [playlist, ...(current ?? [])]);
            setView({ kind: "playlist", playlistId: playlist.id });
          }}
          onError={setError}
        />
      )}
    </Screen>
  );
}

/** Play and Shuffle for a list of takes. */
function PlayButtons({ queue }: { queue: QueueItem[] }) {
  if (queue.length === 0) return null;
  return (
    <Inline gap={2}>
      <div className="grow">
        <Button variant="primary" block icon={<PlayerIcon.play size={16} />} onClick={() => playQueue(queue, 0)}>
          Play
        </Button>
      </div>
      <div className="grow">
        <Button
          block
          icon={<PlayerIcon.shuffle />}
          onClick={() => playQueue(queue, Math.floor(Math.random() * queue.length), { shuffle: true })}
        >
          Shuffle
        </Button>
      </div>
    </Inline>
  );
}

/** A cover that shows play/pause over it while its take is the one playing. */
function TrackCover({ item, size = 48 }: { item: QueueItem; size?: number }) {
  const player = usePlayer();
  const current = currentItem(player)?.track.id === item.track.id;
  return (
    <span className="cover-wrap">
      <Cover path={item.track.imagePath} size={size} />
      {current ? (
        <span className="cover-badge">
          {player.playing ? <PlayerIcon.pause size={16} /> : <PlayerIcon.play size={16} />}
        </span>
      ) : null}
    </span>
  );
}

function useIsCurrent(trackId: number): boolean {
  const player = usePlayer();
  return currentItem(player)?.track.id === trackId;
}

/** A row that plays its take when tapped, or pauses it while it plays. */
function TrackRow({
  item,
  subtitle,
  onPlay,
  trailing,
}: {
  item: QueueItem;
  subtitle: string;
  onPlay: () => void;
  trailing?: ReactNode;
}) {
  const current = useIsCurrent(item.track.id);
  return (
    <ListItem
      leading={<TrackCover item={item} />}
      title={item.song.title}
      subtitle={subtitle}
      selected={current}
      label={`Play ${item.song.title}`}
      onClick={() => (current ? toggle() : onPlay())}
      trailing={trailing}
      chevron={false}
    />
  );
}

// ------------------------------------------------------------------ songs

function SongList({
  songs,
  selecting,
  onSelecting,
  onOpen,
  onCompose,
  onDeleted,
  onError,
}: {
  songs: Song[] | null;
  selecting: boolean;
  onSelecting: (selecting: boolean) => void;
  onOpen: (songId: number) => void;
  onCompose?: () => void;
  onDeleted: () => void;
  onError: (message: string) => void;
}) {
  const [selected, setSelected] = useState<Set<number>>(new Set());

  if (songs === null) return <Spinner />;
  if (songs.length === 0) {
    return (
      <List>
        <EmptyState
          icon="🎵"
          title="No songs yet"
          action={
            onCompose ? (
              <Button variant="primary" size="sm" onClick={onCompose}>
                Make a song
              </Button>
            ) : undefined
          }
        >
          Make one with +, or ask the tutor for a song about anything.
        </EmptyState>
      </List>
    );
  }

  const queue = libraryQueue(songs);

  function pick(songId: number) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(songId)) next.delete(songId);
      else next.add(songId);
      return next;
    });
  }

  function stop() {
    onSelecting(false);
    setSelected(new Set());
  }

  return (
    <Stack gap={4}>
      {selecting ? null : <PlayButtons queue={queue} />}

      <Section
        title={selecting ? `${selected.size} selected` : plural(songs.length, "song")}
        action={
          selecting ? (
            <Inline gap={1}>
              <Button
                variant="plain"
                size="sm"
                onClick={() =>
                  setSelected(selected.size === songs.length ? new Set() : new Set(songs.map((s) => s.id)))
                }
              >
                {selected.size === songs.length ? "None" : "All"}
              </Button>
              <Button variant="plain" size="sm" onClick={stop}>
                Done
              </Button>
            </Inline>
          ) : (
            <Button variant="plain" size="sm" onClick={() => onSelecting(true)}>
              Select
            </Button>
          )
        }
      >
        <List>
          {songs.map((song) => {
            if (selecting) {
              const checked = selected.has(song.id);
              return (
                <ListItem
                  key={song.id}
                  leading={
                    <Inline gap={3}>
                      <span className="check" data-checked={checked}>
                        {checked ? <CheckIcon /> : null}
                      </span>
                      <Cover path={song.tracks[0]?.imagePath ?? null} size={48} />
                    </Inline>
                  }
                  title={song.title}
                  subtitle={`„${song.chorusPl}”`}
                  selected={checked}
                  chevron={false}
                  onClick={() => pick(song.id)}
                />
              );
            }
            const index = queue.findIndex((item) => item.song.id === song.id);
            if (index < 0) {
              // Still being made, or failed: nothing to play yet.
              return (
                <ListItem
                  key={song.id}
                  leading={<Cover path={null} size={48} />}
                  title={song.title}
                  subtitle={song.status === "generating" ? "Suno is singing… (2–4 min)" : "Could not be made"}
                  trailing={
                    song.status === "generating" ? <Spinner /> : <Badge tone="crimson">Failed</Badge>
                  }
                  onClick={() => onOpen(song.id)}
                />
              );
            }
            const item = queue[index];
            return (
              <TrackRow
                key={song.id}
                item={item}
                subtitle={`„${song.chorusPl}” · ${formatTime(item.track.duration)}`}
                onPlay={() => playQueue(queue, index)}
                trailing={
                  <IconButton icon={<MoreIcon />} label={`More about ${song.title}`} onClick={() => onOpen(song.id)} />
                }
              />
            );
          })}
        </List>
      </Section>

      {selecting && selected.size > 0 ? (
        <div className="select-bar">
          <div className="grow">
            <ConfirmButton
              label={`Delete ${plural(selected.size, "song")}`}
              confirmLabel="Delete"
              block
              onConfirm={async () => {
                try {
                  for (const songId of selected) await api.songDelete(songId);
                  stop();
                } catch (cause) {
                  onError(errorMessage(cause));
                } finally {
                  onDeleted();
                }
              }}
            />
          </div>
        </div>
      ) : null}
    </Stack>
  );
}

function SongPage({
  song,
  library,
  onChanged,
  onDeleted,
  onMessage,
  onPractise,
}: {
  song: Song;
  /** Every song, so a take plays on into the next song — not the other take. */
  library: Song[];
  onChanged: () => void;
  onDeleted: () => void;
  onMessage: (message: string, isError?: boolean) => void;
  /** Opens (or makes) the song's listening block. */
  onPractise: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [adding, setAdding] = useState<number | null>(null);
  const exporter = useTrackExport(onMessage);
  const player = usePlayer();
  const playingId = currentItem(player)?.track.id ?? null;
  const takes: QueueItem[] = song.tracks.map((track) => ({ song, track }));

  async function act(action: () => Promise<unknown>, after: () => void = onChanged) {
    setBusy(true);
    try {
      await action();
      after();
    } catch (cause) {
      onMessage(errorMessage(cause), true);
    } finally {
      setBusy(false);
    }
  }

  function play(item: QueueItem) {
    if (item.track.id === playingId) {
      toggle();
      return;
    }
    const queue = libraryQueue(library, item.track);
    const start = queue.findIndex((entry) => entry.track.id === item.track.id);
    playQueue(start >= 0 ? queue : [item], Math.max(0, start));
  }

  return (
    <>
      <div className="song-head">
        <Cover path={song.tracks[0]?.imagePath ?? null} size={112} />
        <Stack gap={1} className="grow">
          <h2>{song.title}</h2>
          <Text weight="bold" tone="polish" selectable>
            „{song.chorusPl}”
          </Text>
          <Text variant="small" tone="dim">
            {song.chorusEn}
          </Text>
        </Stack>
      </div>

      {song.status === "failed" ? (
        <Stack gap={3}>
          {song.error ? <Banner tone="error">{song.error}</Banner> : null}
          <Button variant="primary" block loading={busy} onClick={() => act(() => api.songRetry(song.id))}>
            Try again
          </Button>
        </Stack>
      ) : null}
      {song.status === "generating" ? (
        <Banner tone="info">Suno is singing — this usually takes 2–4 minutes.</Banner>
      ) : null}

      {takes.length > 0 ? (
        <Section title="Takes">
          <List>
            {takes.map((item) => {
              const current = item.track.id === playingId;
              return (
                <ListItem
                  key={item.track.id}
                  selected={current}
                  leading={
                    <IconButton
                      variant="primary"
                      size="sm"
                      icon={current && player.playing ? <PlayerIcon.pause size={16} /> : <PlayerIcon.play size={16} />}
                      label={current && player.playing ? "Pause" : `Play take ${item.track.ord + 1}`}
                      onClick={() => play(item)}
                    />
                  }
                  title={`Take ${item.track.ord + 1}`}
                  subtitle={`${formatTime(item.track.duration)} · played ${item.track.playCount}×`}
                  onClick={() => play(item)}
                  chevron={false}
                  trailing={
                    <>
                      <IconButton
                        icon={<PlayerIcon.addToPlaylist />}
                        label="Add to playlist"
                        onClick={() => setAdding(item.track.id)}
                      />
                      {exporter.canShare ? (
                        <IconButton
                          icon={<PlayerIcon.share />}
                          label="Share"
                          loading={exporter.busy === "share"}
                          disabled={exporter.busy !== null}
                          onClick={() => exporter.share(item)}
                        />
                      ) : null}
                      <IconButton
                        icon={<PlayerIcon.download />}
                        label="Save MP3"
                        loading={exporter.busy === "save"}
                        disabled={exporter.busy !== null}
                        onClick={() => exporter.save(item)}
                      />
                    </>
                  }
                />
              );
            })}
          </List>
        </Section>
      ) : null}

      {song.status === "ready" ? (
        <Section title="Practise">
          <List>
            <ListItem
              leading={<SpeakerIcon />}
              title="Listen and answer questions"
              subtitle="A listening block: the examiner asks about what you heard"
              onClick={onPractise}
            />
          </List>
        </Section>
      ) : null}

      <Section title="Lyrics">
        <Lyrics sections={song.sections} />
        <Text variant="caption" tone="faint" clamp={2}>
          Style: {song.style}
        </Text>
      </Section>

      <Section title="Manage">
        <Stack gap={3}>
          {takes.length > 1
            ? takes.map((item) => (
                <ConfirmButton
                  key={item.track.id}
                  label={`Remove take ${item.track.ord + 1}`}
                  confirmLabel="Remove"
                  question={`Remove take ${item.track.ord + 1}? The other take stays.`}
                  block
                  onConfirm={() => act(() => api.songTrackDelete(item.track.id))}
                />
              ))
            : null}
          <ConfirmButton
            label="Delete song"
            confirmLabel="Delete"
            question="Delete the song and all its takes? It also leaves your playlists."
            block
            onConfirm={() => act(() => api.songDelete(song.id), onDeleted)}
          />
        </Stack>
      </Section>

      {adding !== null ? (
        <AddToPlaylist trackId={adding} onClose={() => setAdding(null)} onDone={(text) => onMessage(text)} />
      ) : null}
    </>
  );
}

// ------------------------------------------------------------------ playlists

function playlistItems(playlist: Playlist, tracks: Map<number, QueueItem>) {
  return playlist.entries
    .map((entry) => ({ entry, item: tracks.get(entry.trackId) }))
    .filter((row): row is { entry: (typeof playlist.entries)[number]; item: QueueItem } =>
      Boolean(row.item),
    );
}

function PlaylistList({
  playlists,
  tracks,
  onOpen,
  onCreated,
  onError,
}: {
  playlists: Playlist[] | null;
  tracks: Map<number, QueueItem>;
  onOpen: (playlistId: number) => void;
  onCreated: (playlist: Playlist) => void;
  onError: (message: string) => void;
}) {
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);

  async function create() {
    if (!name.trim()) return;
    setBusy(true);
    try {
      onCreated(await api.playlistCreate(name));
      setName("");
    } catch (cause) {
      onError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Stack gap={4}>
      <div className="composer">
        <input
          value={name}
          placeholder="New playlist name"
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && void create()}
        />
        <Button variant="primary" loading={busy} disabled={!name.trim()} onClick={create}>
          Create
        </Button>
      </div>
      {playlists === null ? (
        <Spinner />
      ) : playlists.length === 0 ? (
        <List>
          <EmptyState icon="🎧" title="No playlists yet">
            Name one above, or use “Add to playlist” on a song.
          </EmptyState>
        </List>
      ) : (
        <List>
          {playlists.map((playlist) => {
            const items = playlistItems(playlist, tracks);
            const seconds = items.reduce((sum, row) => sum + row.item.track.duration, 0);
            return (
              <ListItem
                key={playlist.id}
                leading={<Cover path={items[0]?.item.track.imagePath ?? null} size={48} />}
                title={playlist.name}
                subtitle={`${plural(items.length, "song")}${seconds > 0 ? ` · ${formatTime(seconds)}` : ""}`}
                onClick={() => onOpen(playlist.id)}
              />
            );
          })}
        </List>
      )}
    </Stack>
  );
}

function PlaylistPage({
  playlist,
  tracks,
  onChanged,
  onDeleted,
  onError,
}: {
  playlist: Playlist;
  tracks: Map<number, QueueItem>;
  onChanged: (playlist: Playlist) => void;
  onDeleted: () => void;
  onError: (message: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(playlist.name);
  const rows = playlistItems(playlist, tracks);
  const queue = rows.map((row) => row.item);

  async function act(action: () => Promise<Playlist>) {
    try {
      onChanged(await action());
    } catch (cause) {
      onError(errorMessage(cause));
    }
  }

  return (
    <>
      <PlayButtons queue={queue} />

      <Section title={plural(rows.length, "song")}>
        {rows.length === 0 ? (
          <List>
            <EmptyState icon="🎧" title="Empty playlist">
              Open a song and use “Add to playlist” to put a take here.
            </EmptyState>
          </List>
        ) : (
          <List>
            {rows.map(({ entry, item }, index) => (
              <TrackRow
                key={entry.id}
                item={item}
                subtitle={`Take ${item.track.ord + 1} · ${formatTime(item.track.duration)}`}
                onPlay={() => playQueue(queue, index)}
                trailing={
                  <>
                    <IconButton
                      size="sm"
                      icon={<ArrowUpIcon />}
                      label="Move up"
                      disabled={index === 0}
                      onClick={() => act(() => api.playlistMove(playlist.id, entry.id, -1))}
                    />
                    <IconButton
                      size="sm"
                      icon={<ArrowDownIcon />}
                      label="Move down"
                      disabled={index === rows.length - 1}
                      onClick={() => act(() => api.playlistMove(playlist.id, entry.id, 1))}
                    />
                    <IconButton
                      size="sm"
                      icon={<CloseIcon />}
                      label="Remove from playlist"
                      onClick={() => act(() => api.playlistRemove(playlist.id, entry.id))}
                    />
                  </>
                }
              />
            ))}
          </List>
        )}
      </Section>

      <Section title="Manage">
        <Stack gap={3}>
          {editing ? (
            <div className="composer">
              <input value={name} onChange={(event) => setName(event.target.value)} />
              <Button
                variant="primary"
                disabled={!name.trim()}
                onClick={async () => {
                  await act(() => api.playlistRename(playlist.id, name));
                  setEditing(false);
                }}
              >
                Save
              </Button>
            </div>
          ) : (
            <Button block onClick={() => setEditing(true)}>
              Rename playlist
            </Button>
          )}
          <ConfirmButton
            label="Delete playlist"
            confirmLabel="Delete"
            question="Delete this playlist? Its songs stay in your library."
            block
            onConfirm={async () => {
              try {
                await api.playlistDelete(playlist.id);
                onDeleted();
              } catch (cause) {
                onError(errorMessage(cause));
              }
            }}
          />
        </Stack>
      </Section>
    </>
  );
}

