import { useEffect, useState } from "react";

import { Spinner } from "./ui";
import { api, errorMessage } from "../lib/api";
import { canShare, shareTrack, type QueueItem } from "../lib/player";
import type { Playlist } from "../lib/types";

/** Picks a playlist for a take — or makes a new one with it. */
export function AddToPlaylist({
  trackId,
  onClose,
  onDone,
}: {
  trackId: number;
  onClose: () => void;
  /** Called with a short confirmation, e.g. "Added to Morning run". */
  onDone: (message: string) => void;
}) {
  const [playlists, setPlaylists] = useState<Playlist[] | null>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.playlistsList().then(setPlaylists).catch((cause) => setError(errorMessage(cause)));
  }, []);

  async function act(action: () => Promise<Playlist>) {
    setBusy(true);
    setError(null);
    try {
      const playlist = await action();
      onDone(`Added to ${playlist.name}`);
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  }

  return (
    <div className="sheet-backdrop fade-in" onClick={onClose}>
      <div className="sheet card" onClick={(event) => event.stopPropagation()}>
        <h3>Add to playlist</h3>
        {error ? <p className="small" style={{ color: "var(--crimson-ink)" }}>{error}</p> : null}
        {playlists === null ? (
          <Spinner />
        ) : (
          <div className="stack" style={{ marginBottom: 12 }}>
            {playlists.map((playlist) => (
              <button
                key={playlist.id}
                className="btn ghost"
                style={{ justifyContent: "space-between" }}
                disabled={busy}
                onClick={() => act(() => api.playlistAdd(playlist.id, [trackId]))}
              >
                <span>{playlist.name}</span>
                <span className="small faint">{playlist.entries.length}</span>
              </button>
            ))}
          </div>
        )}
        <div className="composer">
          <input
            value={name}
            placeholder="New playlist…"
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && name.trim()) {
                void act(() => api.playlistCreate(name, [trackId]));
              }
            }}
          />
          <button
            className="btn primary"
            disabled={busy || !name.trim()}
            onClick={() => act(() => api.playlistCreate(name, [trackId]))}
          >
            Create
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Gets a take's MP3 out of the app: the share sheet on a phone, a save
 * dialog everywhere. Reports what happened through `onMessage`.
 */
export function useTrackExport(onMessage: (message: string, isError?: boolean) => void) {
  const [busy, setBusy] = useState<"save" | "share" | null>(null);

  async function save(item: QueueItem) {
    setBusy("save");
    try {
      const where = await api.songTrackSave(item.track.id);
      if (where) onMessage(`Saved to ${decodeURIComponent(where)}`);
    } catch (cause) {
      onMessage(errorMessage(cause), true);
    } finally {
      setBusy(null);
    }
  }

  async function share(item: QueueItem) {
    setBusy("share");
    try {
      await shareTrack(item);
    } catch (cause) {
      onMessage(errorMessage(cause), true);
    } finally {
      setBusy(null);
    }
  }

  return { busy, save, share, canShare };
}
