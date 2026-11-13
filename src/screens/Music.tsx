import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";

import { Cover } from "../components/Cover";
import { PlayerIcon } from "../components/icons";
import { TitleWithInfo } from "../components/Info";
import { Lyrics } from "../components/Lyrics";
import { AddToPlaylist, useTrackExport } from "../components/TrackActions";
import { Banner, Spinner } from "../components/ui";
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


export function Music({
  settings,
  topics,
  openSongId,
  onOpenSettings,
  onSongOpened,
}: {
  settings: Settings;
  topics: TopicProgress[];
  /** Set when the player asks to show a song's page. */
  openSongId: number | null;
  onOpenSettings: () => void;
  onSongOpened: () => void;
}) {
  const [songs, setSongs] = useState<Song[] | null>(null);
  const [playlists, setPlaylists] = useState<Playlist[] | null>(null);
  const [tab, setTab] = useState<"songs" | "playlists">("songs");
  const [view, setView] = useState<View>({ kind: "library" });
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

  if (view.kind === "compose") {
    return (
      <SongComposer
        settings={settings}
        topics={topics}
        onCancel={() => setView({ kind: "library" })}
        onCreated={() => {
          setView({ kind: "library" });
          refresh();
        }}
      />
    );
  }

  const back = () => {
    setView({ kind: "library" });
    setError(null);
    setNotice(null);
  };

  if (view.kind === "song") {
    const song = songs?.find((entry) => entry.id === view.songId);
    return (
      <div className="scroll">
        <BackHeader title={song?.title ?? "Song"} onBack={back} />
        {error ? <Banner kind="error">{error}</Banner> : null}
        {notice ? <Banner kind="info">{notice}</Banner> : null}
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
          />
        ) : songs === null ? (
          <Spinner />
        ) : (
          <p className="faint">This song no longer exists.</p>
        )}
      </div>
    );
  }

  if (view.kind === "playlist") {
    const playlist = playlists?.find((entry) => entry.id === view.playlistId);
    return (
      <div className="scroll">
        <BackHeader title={playlist?.name ?? "Playlist"} onBack={back} />
        {error ? <Banner kind="error">{error}</Banner> : null}
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
          <p className="faint">This playlist no longer exists.</p>
        )}
      </div>
    );
  }

  const ready = settings.hasKieKey && settings.hasApiKey;

  return (
    <div className="scroll">
      <div className="row between" style={{ marginBottom: 4 }}>
        <TitleWithInfo
          as="h1"
          title="Music"
          info={
            <>
              <p>
                Short lines in English, then straight into Polish — and a Polish chorus that
                gets stuck in your head. Play them on repeat.
              </p>
              <p>
                Make one with <b>+ New song</b>: pick a theme and a chorus sentence, the
                songwriter drafts the lyrics, and Suno sings two takes. Both are kept on this
                device. Put the ones you like into playlists, and save or share any take as
                an MP3.
              </p>
            </>
          }
        />
        <button
          className="btn primary small"
          onClick={() => setView({ kind: "compose" })}
          disabled={!ready}
        >
          + New song
        </button>
      </div>

      {error ? <Banner kind="error">{error}</Banner> : null}

      {!ready ? (
        <Banner kind="info">
          {!settings.hasKieKey && !settings.hasApiKey
            ? "Making songs needs kie.ai and OpenRouter keys."
            : !settings.hasKieKey
              ? "Making songs needs a kie.ai key."
              : "Making songs needs an OpenRouter key."}{" "}
          <button className="link" onClick={onOpenSettings}>
            Open Settings
          </button>
        </Banner>
      ) : null}

      <div className="segmented" style={{ marginBottom: 14 }}>
        <button data-active={tab === "songs"} onClick={() => setTab("songs")}>
          Songs
        </button>
        <button data-active={tab === "playlists"} onClick={() => setTab("playlists")}>
          Playlists
        </button>
      </div>

      {tab === "songs" ? (
        <SongList
          songs={songs}
          onOpen={(songId) => setView({ kind: "song", songId })}
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
    </div>
  );
}

function BackHeader({ title, onBack, action }: { title: string; onBack: () => void; action?: ReactNode }) {
  return (
    <div className="row" style={{ marginBottom: 14 }}>
      <button className="icon-btn" onClick={onBack} aria-label="Back to music">
        ‹
      </button>
      <h1 className="grow" style={{ marginBottom: 0, fontSize: 21 }}>
        {title}
      </h1>
      {action}
    </div>
  );
}

function PlayButtons({ queue }: { queue: QueueItem[] }) {
  if (queue.length === 0) return null;
  return (
    <div className="row" style={{ gap: 8, marginBottom: 14 }}>
      <button className="btn primary small grow" onClick={() => playQueue(queue, 0)}>
        <PlayerIcon.play size={16} /> Play
      </button>
      <button
        className="btn ghost small grow"
        onClick={() => playQueue(queue, Math.floor(Math.random() * queue.length), { shuffle: true })}
      >
        <PlayerIcon.shuffle /> Shuffle
      </button>
    </div>
  );
}

/** A row that plays its take when tapped, or pauses it if it is playing. */
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
  const player = usePlayer();
  const current = currentItem(player)?.track.id === item.track.id;
  return (
    <div className="track-row" data-current={current}>
      <button className="list-row grow" onClick={() => (current ? toggle() : onPlay())}>
        <span className="cover-wrap">
          <Cover path={item.track.imagePath} size={44} />
          {current ? (
            <span className="cover-badge">
              {player.playing ? <PlayerIcon.pause size={16} /> : <PlayerIcon.play size={16} />}
            </span>
          ) : null}
        </span>
        <span className="grow" style={{ minWidth: 0 }}>
          <span className="pl ellipsis">{item.song.title}</span>
          <span className="tr ellipsis">{subtitle}</span>
        </span>
      </button>
      {trailing}
    </div>
  );
}

// ------------------------------------------------------------------ songs

function SongList({
  songs,
  onOpen,
  onDeleted,
  onError,
}: {
  songs: Song[] | null;
  onOpen: (songId: number) => void;
  onDeleted: () => void;
  onError: (message: string) => void;
}) {
  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [confirming, setConfirming] = useState(false);
  const [deleting, setDeleting] = useState(false);

  if (songs === null) {
    return (
      <div className="center" style={{ padding: 30 }}>
        <Spinner />
      </div>
    );
  }
  if (songs.length === 0) {
    return (
      <div className="card center">
        <div style={{ fontSize: 40 }}>🎵</div>
        <h3 style={{ marginBottom: 0 }}>No songs yet</h3>
        <p className="small faint">Make one with + New song, or ask the tutor.</p>
      </div>
    );
  }
  const queue = libraryQueue(songs);

  function pick(songId: number) {
    setConfirming(false);
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(songId)) next.delete(songId);
      else next.add(songId);
      return next;
    });
  }

  function stopSelecting() {
    setSelecting(false);
    setSelected(new Set());
    setConfirming(false);
  }

  async function deleteSelected() {
    setDeleting(true);
    try {
      for (const songId of selected) await api.songDelete(songId);
      stopSelecting();
    } catch (cause) {
      onError(errorMessage(cause));
    } finally {
      setDeleting(false);
      onDeleted();
    }
  }

  return (
    <>
      {selecting ? (
        <div className="row" style={{ gap: 8, marginBottom: 14 }}>
          <button
            className="btn ghost small"
            onClick={() =>
              setSelected(selected.size === songs.length ? new Set() : new Set(songs.map((s) => s.id)))
            }
          >
            {selected.size === songs.length ? "Select none" : "Select all"}
          </button>
          <span className="grow small faint center">{selected.size} selected</span>
          <button className="btn ghost small" onClick={stopSelecting}>
            Done
          </button>
        </div>
      ) : (
        <div className="row" style={{ gap: 8, marginBottom: 14 }}>
          {queue.length > 0 ? (
            <>
              <button className="btn primary small grow" onClick={() => playQueue(queue, 0)}>
                <PlayerIcon.play size={16} /> Play
              </button>
              <button
                className="btn ghost small grow"
                onClick={() =>
                  playQueue(queue, Math.floor(Math.random() * queue.length), { shuffle: true })
                }
              >
                <PlayerIcon.shuffle /> Shuffle
              </button>
            </>
          ) : (
            <span className="grow" />
          )}
          <button className="btn ghost small" onClick={() => setSelecting(true)}>
            Select
          </button>
        </div>
      )}

      <div className="card flush">
        {songs.map((song) => {
          if (selecting) {
            const checked = selected.has(song.id);
            return (
              <button
                key={song.id}
                className="list-row"
                data-current={checked}
                onClick={() => pick(song.id)}
                aria-pressed={checked}
              >
                <span className="check" data-checked={checked}>
                  {checked ? "✓" : ""}
                </span>
                <Cover path={song.tracks[0]?.imagePath ?? null} size={44} />
                <span className="grow" style={{ minWidth: 0 }}>
                  <span className="pl ellipsis">{song.title}</span>
                  <span className="tr ellipsis">„{song.chorusPl}”</span>
                </span>
              </button>
            );
          }
          const index = queue.findIndex((item) => item.song.id === song.id);
          if (index < 0) {
            // Still being made, or failed: no take to play yet.
            return (
              <button key={song.id} className="list-row" onClick={() => onOpen(song.id)}>
                <Cover path={null} size={44} />
                <span className="grow" style={{ minWidth: 0 }}>
                  <span className="pl ellipsis">{song.title}</span>
                  <span className="tr">
                    {song.status === "generating" ? "Suno is singing…" : "Could not be made"}
                  </span>
                </span>
                {song.status === "generating" ? <Spinner /> : <span className="chip crimson">Failed</span>}
              </button>
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
                <button className="icon-btn" onClick={() => onOpen(song.id)} aria-label={`More about ${song.title}`}>
                  ⋯
                </button>
              }
            />
          );
        })}
      </div>

      {selecting && selected.size > 0 ? (
        <div className="select-bar">
          {confirming ? (
            <>
              <button className="btn danger grow" disabled={deleting} onClick={deleteSelected}>
                {deleting ? <Spinner /> : `Yes, delete ${selected.size} song${selected.size === 1 ? "" : "s"}`}
              </button>
              <button className="btn ghost" onClick={() => setConfirming(false)}>
                Cancel
              </button>
            </>
          ) : (
            <button className="btn danger grow" onClick={() => setConfirming(true)}>
              Delete {selected.size} song{selected.size === 1 ? "" : "s"}
            </button>
          )}
        </div>
      ) : null}
    </>
  );
}

function SongPage({
  song,
  library,
  onChanged,
  onDeleted,
  onMessage,
}: {
  song: Song;
  /** Every song, so a take plays on into the next song — not the other take. */
  library: Song[];
  onChanged: () => void;
  onDeleted: () => void;
  onMessage: (message: string, isError?: boolean) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [adding, setAdding] = useState<number | null>(null);
  const exporter = useTrackExport(onMessage);
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

  return (
    <>
      <div className="row" style={{ alignItems: "flex-start", marginBottom: 14 }}>
        <Cover path={song.tracks[0]?.imagePath ?? null} size={84} />
        <div className="grow">
          <div className="selectable" style={{ fontSize: 16, fontWeight: 700 }}>
            „{song.chorusPl}”
          </div>
          <div className="small faint">{song.chorusEn}</div>
          <div className="tiny faint" style={{ marginTop: 6 }}>
            {song.style}
          </div>
        </div>
      </div>

      {song.status === "failed" ? (
        <>
          {song.error ? <Banner kind="error">{song.error}</Banner> : null}
          <button
            className="btn primary block"
            style={{ marginBottom: 14 }}
            disabled={busy}
            onClick={() => act(() => api.songRetry(song.id))}
          >
            {busy ? <Spinner /> : "Try again"}
          </button>
        </>
      ) : null}
      {song.status === "generating" ? (
        <Banner kind="info">Suno is singing — this usually takes 2–4 minutes.</Banner>
      ) : null}

      {takes.length > 0 ? (
        <>
          <h3 style={{ margin: "4px 0 8px" }}>Takes</h3>
          <div className="card flush" style={{ marginBottom: 16 }}>
            {takes.map((item) => (
              <div key={item.track.id} className="take-block">
                <TrackRow
                  item={item}
                  subtitle={`Take ${item.track.ord + 1} · ${formatTime(item.track.duration)} · played ${item.track.playCount}×`}
                  onPlay={() => {
                    const queue = libraryQueue(library, item.track);
                    const start = queue.findIndex((entry) => entry.track.id === item.track.id);
                    playQueue(start >= 0 ? queue : [item], Math.max(0, start));
                  }}
                />
                <div className="row wrap take-actions">
                  <button className="chip" onClick={() => setAdding(item.track.id)}>
                    + Playlist
                  </button>
                  {exporter.canShare ? (
                    <button
                      className="chip"
                      disabled={exporter.busy !== null}
                      onClick={() => exporter.share(item)}
                    >
                      {exporter.busy === "share" ? <Spinner /> : "↗ Share"}
                    </button>
                  ) : null}
                  <button
                    className="chip"
                    disabled={exporter.busy !== null}
                    onClick={() => exporter.save(item)}
                  >
                    {exporter.busy === "save" ? <Spinner /> : "⬇ Save MP3"}
                  </button>
                  {takes.length > 1 ? (
                    <button
                      className="chip"
                      disabled={busy}
                      onClick={() => act(() => api.songTrackDelete(item.track.id))}
                    >
                      Remove take
                    </button>
                  ) : null}
                </div>
              </div>
            ))}
          </div>
        </>
      ) : null}

      <h3 style={{ margin: "4px 0 8px" }}>Lyrics</h3>
      <div style={{ marginBottom: 16 }}>
        <Lyrics sections={song.sections} />
      </div>

      {confirmDelete ? (
        <div className="row">
          <button
            className="btn danger grow"
            disabled={busy}
            onClick={() => act(() => api.songDelete(song.id), onDeleted)}
          >
            Yes, delete the song
          </button>
          <button className="btn ghost" onClick={() => setConfirmDelete(false)}>
            Cancel
          </button>
        </div>
      ) : (
        <button className="btn danger block" onClick={() => setConfirmDelete(true)}>
          Delete song
        </button>
      )}

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
    <>
      <div className="composer" style={{ marginBottom: 14 }}>
        <input
          value={name}
          placeholder="New playlist…"
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && void create()}
        />
        <button className="btn primary" disabled={busy || !name.trim()} onClick={create}>
          Create
        </button>
      </div>
      {playlists === null ? (
        <Spinner />
      ) : playlists.length === 0 ? (
        <p className="small faint center">
          No playlists yet. Make one here, or use <b>+ Playlist</b> on a song.
        </p>
      ) : (
        <div className="card flush">
          {playlists.map((playlist) => {
            const items = playlistItems(playlist, tracks);
            const seconds = items.reduce((sum, row) => sum + row.item.track.duration, 0);
            return (
              <button key={playlist.id} className="list-row" onClick={() => onOpen(playlist.id)}>
                <Cover path={items[0]?.item.track.imagePath ?? null} size={44} />
                <span className="grow">
                  <span className="pl">{playlist.name}</span>
                  <span className="tr">
                    {items.length} song{items.length === 1 ? "" : "s"}
                    {seconds > 0 ? ` · ${formatTime(seconds)}` : ""}
                  </span>
                </span>
                <span className="faint">›</span>
              </button>
            );
          })}
        </div>
      )}
    </>
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
  const [confirmDelete, setConfirmDelete] = useState(false);
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

      {rows.length === 0 ? (
        <p className="small faint center">
          Empty. Open a song and tap <b>+ Playlist</b> to add it here.
        </p>
      ) : (
        <div className="card flush" style={{ marginBottom: 16 }}>
          {rows.map(({ entry, item }, index) => (
            <TrackRow
              key={entry.id}
              item={item}
              subtitle={`Take ${item.track.ord + 1} · ${formatTime(item.track.duration)}`}
              onPlay={() => playQueue(queue, index)}
              trailing={
                <span className="row" style={{ gap: 2, flex: "none" }}>
                  <button
                    className="icon-btn small"
                    disabled={index === 0}
                    onClick={() => act(() => api.playlistMove(playlist.id, entry.id, -1))}
                    aria-label="Move up"
                  >
                    ↑
                  </button>
                  <button
                    className="icon-btn small"
                    disabled={index === rows.length - 1}
                    onClick={() => act(() => api.playlistMove(playlist.id, entry.id, 1))}
                    aria-label="Move down"
                  >
                    ↓
                  </button>
                  <button
                    className="icon-btn small"
                    onClick={() => act(() => api.playlistRemove(playlist.id, entry.id))}
                    aria-label="Remove from playlist"
                  >
                    ✕
                  </button>
                </span>
              }
            />
          ))}
        </div>
      )}

      {editing ? (
        <div className="composer" style={{ marginBottom: 10 }}>
          <input value={name} onChange={(event) => setName(event.target.value)} />
          <button
            className="btn primary"
            disabled={!name.trim()}
            onClick={async () => {
              await act(() => api.playlistRename(playlist.id, name));
              setEditing(false);
            }}
          >
            Save
          </button>
        </div>
      ) : (
        <button className="btn ghost block" style={{ marginBottom: 10 }} onClick={() => setEditing(true)}>
          Rename playlist
        </button>
      )}

      {confirmDelete ? (
        <div className="row">
          <button
            className="btn danger grow"
            onClick={async () => {
              try {
                await api.playlistDelete(playlist.id);
                onDeleted();
              } catch (cause) {
                onError(errorMessage(cause));
              }
            }}
          >
            Yes, delete the playlist
          </button>
          <button className="btn ghost" onClick={() => setConfirmDelete(false)}>
            Cancel
          </button>
        </div>
      ) : (
        <button className="btn danger block" onClick={() => setConfirmDelete(true)}>
          Delete playlist
        </button>
      )}
      <p className="tiny faint center">Deleting a playlist keeps its songs.</p>
    </>
  );
}
