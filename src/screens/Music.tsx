import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { onSignal } from "../lib/events";

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
  SplitView,
  Stack,
  Text,
  useWideLayout,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { toast } from "../lib/toast";
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
import type { LessonId, Playlist, PlaylistId, Settings, Song, SongId, TrackId, WikiSource } from "../lib/types";
import { EpisodeSources, PodcastSound } from "../components/Podcast";
import { ArticleReader } from "../components/Sources";
import { PodcastComposer } from "./PodcastComposer";
import { SongComposer } from "./SongComposer";

/**
 * The music library: songs and playlists. Playing happens in the player
 * (the bar above the tabs, and the full screen it opens); this screen only
 * chooses what to play and manages the collection.
 */
type View =
  | { kind: "library" }
  | { kind: "song"; songId: SongId }
  | { kind: "playlist"; playlistId: PlaylistId }
  | { kind: "compose"; what: "song" | "podcast" }
  | { kind: "article"; songId: SongId; source: WikiSource };

/** Every downloaded take, in library order. */
function allTakes(songs: Song[]): QueueItem[] {
  return songs
    .filter((song) => song.status === "ready")
    .flatMap((song) => song.tracks.map((track) => ({ song, track })));
}

/** What a song or episode still being made is doing. */
function making(song: Song): string {
  if (song.kind !== "podcast") return "Suno is singing… (2–4 min)";
  if (song.sections.length > 0) return "Recording…";
  return song.origin?.news ? "Reading today's news…" : "Writing the script…";
}

/** A row's subtitle: a song's chorus, or that it is a podcast. */
function describe(song: Song): string {
  return song.kind === "podcast" ? `Podcast · ${song.chorusPl}` : `„${song.chorusPl}”`;
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/**
 * What is playing, at the top of the Music tab, while something is: the way
 * into the full player. Controls outside the app live in the phone's media
 * controls (control center, lock screen).
 */
function NowPlaying({ onOpen }: { onOpen: () => void }) {
  const player = usePlayer();
  const item = currentItem(player);
  if (!item) return null;
  return (
    <List>
      <ListItem
        leading={<Cover fileId={item.track.imageFileId} size={44} />}
        title={item.song.title}
        subtitle={player.cast ? `Now playing on ${player.cast.device.name}` : "Now playing"}
        label="Open the player"
        onClick={onOpen}
        trailing={
          <IconButton
            icon={player.playing ? <PlayerIcon.pause size={20} /> : <PlayerIcon.play size={20} />}
            label={player.playing ? "Pause" : "Play"}
            onClick={toggle}
          />
        }
      />
    </List>
  );
}

/** The library as last loaded, kept while the app runs (see Music). */
let cachedSongs: Song[] | null = null;
let cachedPlaylists: Playlist[] | null = null;

/**
 * Loads the library in the background, a moment after the app starts, so
 * even the first visit to Music finds it ready.
 */
export function preloadLibrary(): void {
  if (cachedSongs !== null) return;
  void api
    .songsList()
    .then((list) => {
      if (cachedSongs === null) cachedSongs = list;
    })
    .catch(() => {});
  void api
    .playlistsList()
    .then((list) => {
      if (cachedPlaylists === null) cachedPlaylists = list;
    })
    .catch(() => {});
}

export function Music({
  settings,
  openSongId,
  onOpenSettings,
  onSongOpened,
  onOpenLesson,
  onOpenPlayer,
}: {
  settings: Settings;
  /** Set when the player asks to show a song's page. */
  openSongId: SongId | null;
  onOpenSettings: () => void;
  onSongOpened: () => void;
  /** Opens a lesson's page (a song's listening lesson). */
  onOpenLesson: (lessonId: LessonId) => void;
  /** Opens the full player (lyrics that follow the music). */
  onOpenPlayer: () => void;
}) {
  // The last library seen shows at once; a fresh one replaces it when it
  // arrives, so opening the tab never waits.
  const [songs, setSongs] = useState<Song[] | null>(cachedSongs);
  const [playlists, setPlaylists] = useState<Playlist[] | null>(cachedPlaylists);
  const [tab, setTab] = useState<"songs" | "playlists">("songs");
  const [view, setView] = useState<View>({ kind: "library" });
  const [selecting, setSelecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api
      .songsList()
      .then((list) => {
        cachedSongs = list;
        setSongs(list);
        syncLibrary(list);
      })
      .catch((cause) => setError(errorMessage(cause)));
    api
      .playlistsList()
      .then((list) => {
        cachedPlaylists = list;
        setPlaylists(list);
      })
      .catch((cause) => setError(errorMessage(cause)));
  }, []);

  useEffect(refresh, [refresh]);

  // Songs finish in the background; the backend says when.
  useEffect(() => {
    const unlisten = onSignal("songs://changed", refresh);
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
    const map = new Map<TrackId, QueueItem>();
    for (const item of allTakes(songs ?? [])) map.set(item.track.id, item);
    return map;
  }, [songs]);

  // Confirmations and failed actions float over the screen, seen wherever
  // the page is scrolled.
  const message = (text: string, isError = false) => toast(text, isError ? "error" : "ok");

  const back = () => {
    setView({ kind: "library" });
    setError(null);
  };

  // On a wide window the library stays on the left and what is open in it
  // shows beside it, without a way back (the library is in view).
  const wide = useWideLayout();
  const onBack = wide ? undefined : back;

  let detail: ReactNode = null;
  if (view.kind === "compose") {
    const created = () => {
      back();
      refresh();
    };
    detail =
      view.what === "podcast" ? (
        <PodcastComposer
          settings={settings}
          onCancel={back}
          onCreated={created}
          onSwitch={() => setView({ kind: "compose", what: "song" })}
        />
      ) : (
        <SongComposer
          settings={settings}
          onCancel={back}
          onCreated={created}
          onSwitch={() => setView({ kind: "compose", what: "podcast" })}
        />
      );
  }

  const feedback = <>{error ? <Banner tone="error">{error}</Banner> : null}</>;

  if (view.kind === "song") {
    const song = songs?.find((entry) => entry.id === view.songId);
    detail = (
      <Screen title={song?.kind === "podcast" ? "Podcast" : "Song"} onBack={onBack}>
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
                onOpenLesson((await api.lessonForSong(song.id)).id);
              } catch (cause) {
                toast(errorMessage(cause), "error");
              }
            }}
            onRead={(source) => setView({ kind: "article", songId: song.id, source })}
          />
        ) : songs === null ? (
          <Spinner />
        ) : (
          <EmptyState title="This song no longer exists" />
        )}
      </Screen>
    );
  }

  if (view.kind === "article") {
    const songId = view.songId;
    detail = <ArticleReader source={view.source} onBack={() => setView({ kind: "song", songId })} />;
  }

  if (view.kind === "playlist") {
    const playlist = playlists?.find((entry) => entry.id === view.playlistId);
    detail = (
      <Screen title={playlist?.name ?? "Playlist"} onBack={onBack}>
        {feedback}
        {playlist ? (
          <PlaylistPage
            playlist={playlist}
            tracks={tracks}
            onChanged={(next) =>
              setPlaylists((current) => (current ?? []).map((entry) => (entry.id === next.id ? next : entry)))
            }
            onDeleted={() => {
              back();
              refresh();
            }}
            onError={(text) => toast(text, "error")}
          />
        ) : playlists === null ? (
          <Spinner />
        ) : (
          <EmptyState title="This playlist no longer exists" />
        )}
      </Screen>
    );
  }

  // Songs need both keys; podcasts only OpenRouter's.
  const ready = settings.hasApiKey;
  if (detail !== null && !wide) return detail;

  const library = (
    <Screen
      title="Music"
      info={
        <>
          <p>
            Short lines in English, then straight into Polish — and a Polish chorus that gets stuck in your head. Play
            them on repeat.
          </p>
          <p>
            Make one with <b>+</b>: pick a theme and a chorus sentence, the songwriter drafts the lyrics, and Suno sings
            two takes. Put the ones you like into playlists, and save or share any take as an MP3.
          </p>
          <p>
            Or make a <b>podcast</b>: a short spoken episode on a subject you like, written from a Wikipedia article.
            Every sentence is said in English, then in Polish.
          </p>
        </>
      }
      actions={
        <IconButton
          variant="primary"
          icon={<PlusIcon />}
          label="New song or podcast"
          disabled={!ready}
          onClick={() => setView({ kind: "compose", what: "song" })}
        />
      }
    >
      {feedback}

      <NowPlaying onOpen={onOpenPlayer} />

      {!ready || !settings.hasKieKey ? (
        <Banner tone="info">
          {!settings.hasKieKey && !settings.hasApiKey
            ? "Making songs needs kie.ai and OpenRouter keys; podcasts need the OpenRouter key. "
            : !settings.hasKieKey
              ? "Making songs needs a kie.ai key; podcasts work without one. "
              : "Making songs and podcasts needs an OpenRouter key. "}
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
          onCompose={ready ? () => setView({ kind: "compose", what: "song" }) : undefined}
          onDeleted={refresh}
          onError={(text) => toast(text, "error")}
        />
      ) : (
        <PlaylistList
          playlists={playlists}
          tracks={tracks}
          current={wide && view.kind === "playlist" ? view.playlistId : undefined}
          onOpen={(playlistId) => setView({ kind: "playlist", playlistId })}
          onCreated={(playlist) => {
            setPlaylists((current) => [playlist, ...(current ?? [])]);
            setView({ kind: "playlist", playlistId: playlist.id });
          }}
          onError={(text) => toast(text, "error")}
        />
      )}
    </Screen>
  );
  if (!wide) return library;
  return (
    <SplitView
      list={library}
      detail={
        detail ?? (
          <Screen>
            <EmptyState icon="🎵" title="Pick a song or a playlist">
              It opens here, beside your library{ready ? " — or make a new song or podcast with +" : ""}.
            </EmptyState>
          </Screen>
        )
      }
    />
  );
}

/** Play and Shuffle for a list of takes. */
function PlayButtons({ queue }: { queue: QueueItem[] }) {
  if (queue.length === 0) return null;
  return (
    <Inline gap={2}>
      <div className="grow">
        <Button variant="primary" wide icon={<PlayerIcon.play size={16} />} onClick={() => playQueue(queue, 0)}>
          Play
        </Button>
      </div>
      <div className="grow">
        <Button
          wide
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
      <Cover fileId={item.track.imageFileId} size={size} />
      {current ? (
        <span className="cover-badge">
          {player.playing ? <PlayerIcon.pause size={16} /> : <PlayerIcon.play size={16} />}
        </span>
      ) : null}
    </span>
  );
}

function useIsCurrent(trackId: TrackId): boolean {
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
  onOpen: (songId: SongId) => void;
  onCompose?: () => void;
  onDeleted: () => void;
  onError: (message: string) => void;
}) {
  const [selected, setSelected] = useState<Set<SongId>>(new Set());

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

  function pick(songId: SongId) {
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
                      <Cover fileId={song.tracks[0]?.imageFileId ?? null} size={48} />
                    </Inline>
                  }
                  title={song.title}
                  subtitle={describe(song)}
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
                  leading={<Cover fileId={null} size={48} />}
                  title={song.title}
                  subtitle={song.status === "generating" ? making(song) : "Could not be made"}
                  trailing={song.status === "generating" ? <Spinner /> : <Badge tone="crimson">Failed</Badge>}
                  onClick={() => onOpen(song.id)}
                />
              );
            }
            const item = queue[index];
            return (
              <TrackRow
                key={song.id}
                item={item}
                subtitle={`${describe(song)} · ${formatTime(item.track.duration)}`}
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
              wide
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
  onRead,
}: {
  song: Song;
  /** Every song, so a take plays on into the next song — not the other take. */
  library: Song[];
  onChanged: () => void;
  onDeleted: () => void;
  onMessage: (message: string, isError?: boolean) => void;
  /** Opens (or makes) the song's listening lesson. */
  onPractise: () => void;
  /** Opens one of an episode's articles in the reader. */
  onRead: (source: WikiSource) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [adding, setAdding] = useState<TrackId | null>(null);
  const exporter = useTrackExport(onMessage);
  const player = usePlayer();
  const playingId = currentItem(player)?.track.id ?? null;
  const takes: QueueItem[] = song.tracks.map((track) => ({ song, track }));
  const podcast = song.kind === "podcast";

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
        <Cover fileId={song.tracks[0]?.imageFileId ?? null} size={112} />
        <Stack gap={1} className="grow">
          <h2>{song.title}</h2>
          <Text weight="bold" tone="polish" selectable>
            {podcast ? song.chorusPl : `„${song.chorusPl}”`}
          </Text>
          <Text variant="small" tone="dim">
            {song.chorusEn}
          </Text>
        </Stack>
      </div>

      {song.status === "failed" ? (
        <Stack gap={3}>
          {song.error ? <Banner tone="error">{song.error}</Banner> : null}
          <Button
            variant="primary"
            wide
            loading={busy}
            onClick={() => act(() => (podcast ? api.podcastRerender(song.id) : api.songRetry(song.id)))}
          >
            Try again
          </Button>
        </Stack>
      ) : null}
      {song.status === "generating" ? (
        <Banner tone="info">
          {podcast
            ? song.sections.length === 0
              ? "Writing the script — then each sentence is read aloud. A few minutes; you can leave meanwhile."
              : "Recording — each sentence is read aloud and joined. You can leave meanwhile."
            : "Suno is singing — this usually takes 2–4 minutes."}
        </Banner>
      ) : null}

      {takes.length > 0 ? (
        <Section title={podcast ? "Episode" : "Takes"}>
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
                      label={
                        current && player.playing
                          ? "Pause"
                          : podcast
                            ? "Play the episode"
                            : `Play take ${item.track.ord + 1}`
                      }
                      onClick={() => play(item)}
                    />
                  }
                  title={podcast ? "Play the episode" : `Take ${item.track.ord + 1}`}
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
                        label={podcast ? "Save audio" : "Save MP3"}
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
              subtitle="A listening lesson: the examiner asks about what you heard"
              onClick={onPractise}
            />
          </List>
        </Section>
      ) : null}

      {podcast ? <PodcastSound song={song} onChanged={onChanged} /> : null}

      {podcast ? <EpisodeSources song={song} onRead={onRead} /> : null}

      <Section title={podcast ? "Script" : "Lyrics"}>
        <Lyrics sections={song.sections} parts={podcast} />
        {podcast ? (
          <Text variant="caption" tone="faint" clamp={2}>
            {song.chorusEn}
          </Text>
        ) : (
          <Text variant="caption" tone="faint" clamp={2}>
            Style: {song.style}
          </Text>
        )}
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
                  wide
                  onConfirm={() => act(() => api.songTrackDelete(item.track.id))}
                />
              ))
            : null}
          <ConfirmButton
            label={podcast ? "Delete episode" : "Delete song"}
            confirmLabel="Delete"
            question={
              podcast
                ? "Delete the episode and its audio? It also leaves your playlists, and its listening lesson goes too."
                : "Delete the song and all its takes? It also leaves your playlists, and its listening lesson goes too."
            }
            wide
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

function playlistItems(playlist: Playlist, tracks: Map<TrackId, QueueItem>) {
  return playlist.entries
    .map((entry) => ({ entry, item: tracks.get(entry.trackId) }))
    .filter((row): row is { entry: (typeof playlist.entries)[number]; item: QueueItem } => Boolean(row.item));
}

function PlaylistList({
  playlists,
  tracks,
  current,
  onOpen,
  onCreated,
  onError,
}: {
  playlists: Playlist[] | null;
  tracks: Map<TrackId, QueueItem>;
  /** The playlist open beside the list (wide windows). */
  current?: PlaylistId;
  onOpen: (playlistId: PlaylistId) => void;
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
                leading={<Cover fileId={items[0]?.item.track.imageFileId ?? null} size={48} />}
                title={playlist.name}
                subtitle={`${plural(items.length, "song")}${seconds > 0 ? ` · ${formatTime(seconds)}` : ""}`}
                selected={playlist.id === current}
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
  tracks: Map<TrackId, QueueItem>;
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
            <Button wide onClick={() => setEditing(true)}>
              Rename playlist
            </Button>
          )}
          <ConfirmButton
            label="Delete playlist"
            confirmLabel="Delete"
            question="Delete this playlist? Its songs stay in your library."
            wide
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
