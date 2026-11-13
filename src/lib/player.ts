/**
 * The song player. One audio element for the whole app, so music keeps
 * playing while the learner moves between tabs — the point is to have the
 * songs on in the background, again and again.
 */
import { useSyncExternalStore } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api } from "./api";
import type { Song, SongTrack } from "./types";

export type RepeatMode = "all" | "one" | "off";

export interface QueueItem {
  song: Song;
  track: SongTrack;
}

export interface PlayerState {
  queue: QueueItem[];
  index: number;
  playing: boolean;
  position: number;
  duration: number;
  repeat: RepeatMode;
  shuffle: boolean;
}

const REPEAT_KEY = "polski.repeat";

function storedRepeat(): RepeatMode {
  try {
    const value = localStorage.getItem(REPEAT_KEY);
    if (value === "one" || value === "off" || value === "all") return value;
  } catch {
    /* default below */
  }
  return "all";
}

let state: PlayerState = {
  queue: [],
  index: -1,
  playing: false,
  position: 0,
  duration: 0,
  repeat: storedRepeat(),
  shuffle: false,
};

const listeners = new Set<() => void>();

function set(patch: Partial<PlayerState>) {
  state = { ...state, ...patch };
  listeners.forEach((listener) => listener());
}

let audio: HTMLAudioElement | null = null;
/** The play is counted once per load, when the learner is half-way through. */
let counted = false;

function element(): HTMLAudioElement {
  if (audio) return audio;
  audio = new Audio();
  audio.preload = "auto";
  audio.addEventListener("play", () => set({ playing: true }));
  audio.addEventListener("pause", () => set({ playing: false }));
  audio.addEventListener("loadedmetadata", () =>
    set({ duration: Number.isFinite(audio!.duration) ? audio!.duration : 0 }),
  );
  audio.addEventListener("timeupdate", () => {
    const a = audio!;
    set({ position: a.currentTime });
    const current = state.queue[state.index];
    if (!counted && current && a.duration > 0 && a.currentTime / a.duration > 0.5) {
      counted = true;
      void api.songTrackPlayed(current.track.id).catch(() => {});
    }
  });
  audio.addEventListener("ended", () => {
    if (state.repeat === "one") {
      counted = false;
      audio!.currentTime = 0;
      void audio!.play();
      return;
    }
    const last = state.index >= state.queue.length - 1;
    if (last && state.repeat === "off") {
      set({ playing: false, position: 0 });
      return;
    }
    next();
  });
  setupMediaSession();
  return audio;
}

function load(index: number, autoplay: boolean) {
  const item = state.queue[index];
  if (!item) return;
  const a = element();
  counted = false;
  a.src = convertFileSrc(item.track.audioPath);
  set({ index, position: 0, duration: item.track.duration || 0 });
  updateMediaMetadata(item);
  if (autoplay) {
    void a.play().catch(() => set({ playing: false }));
  }
}

function shuffled<T>(items: T[], first: number): T[] {
  const head = items[first];
  const rest = items.filter((_, i) => i !== first);
  for (let i = rest.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [rest[i], rest[j]] = [rest[j], rest[i]];
  }
  return head === undefined ? rest : [head, ...rest];
}

/** Starts `queue` at `index`. */
export function playQueue(queue: QueueItem[], index = 0, options?: { shuffle?: boolean }) {
  if (queue.length === 0) return;
  const shuffle = options?.shuffle ?? false;
  const ordered = shuffle ? shuffled(queue, index) : queue;
  set({ queue: ordered, shuffle });
  load(shuffle ? 0 : index, true);
}

export function toggle() {
  const a = element();
  if (state.index < 0) return;
  if (a.paused) void a.play().catch(() => {});
  else a.pause();
}

export function next() {
  if (state.queue.length === 0) return;
  load((state.index + 1) % state.queue.length, true);
}

export function previous() {
  if (state.queue.length === 0) return;
  // Like every music player: restart the song unless it has only just begun.
  if (element().currentTime > 4) {
    element().currentTime = 0;
    return;
  }
  load((state.index - 1 + state.queue.length) % state.queue.length, true);
}

export function seek(seconds: number) {
  const a = element();
  a.currentTime = Math.max(0, Math.min(seconds, a.duration || seconds));
}

export function cycleRepeat() {
  const order: RepeatMode[] = ["all", "one", "off"];
  const repeat = order[(order.indexOf(state.repeat) + 1) % order.length];
  try {
    localStorage.setItem(REPEAT_KEY, repeat);
  } catch {
    /* applies for this run */
  }
  set({ repeat });
}

export function stopPlayback() {
  audio?.pause();
  set({ queue: [], index: -1, playing: false, position: 0, duration: 0 });
}

/**
 * Keeps the queue in step with the library: picks up new play counts and
 * stops a song that was deleted while it was queued.
 */
export function syncLibrary(songs: Song[]) {
  if (state.queue.length === 0) return;
  const tracks = new Map<number, QueueItem>();
  for (const song of songs) for (const track of song.tracks) tracks.set(track.id, { song, track });
  const current = state.queue[state.index];
  if (current && !tracks.has(current.track.id)) {
    stopPlayback();
    return;
  }
  const queue = state.queue
    .map((item) => tracks.get(item.track.id))
    .filter((item): item is QueueItem => Boolean(item));
  set({ queue, index: current ? queue.findIndex((i) => i.track.id === current.track.id) : -1 });
}

export function usePlayer(): PlayerState {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => state,
  );
}

export function currentItem(player: PlayerState): QueueItem | null {
  return player.queue[player.index] ?? null;
}

// ------------------------------------------------------- lock-screen controls

function setupMediaSession() {
  if (typeof navigator === "undefined" || !("mediaSession" in navigator)) return;
  const session = navigator.mediaSession;
  const handle = (action: MediaSessionAction, handler: () => void) => {
    try {
      session.setActionHandler(action, handler);
    } catch {
      /* unsupported action on this platform */
    }
  };
  handle("play", toggle);
  handle("pause", toggle);
  handle("nexttrack", next);
  handle("previoustrack", previous);
}

function updateMediaMetadata(item: QueueItem) {
  if (typeof navigator === "undefined" || !("mediaSession" in navigator)) return;
  try {
    navigator.mediaSession.metadata = new MediaMetadata({
      title: item.song.title,
      artist: item.song.chorusPl,
      album: "Polski",
      artwork: item.track.imagePath
        ? [{ src: convertFileSrc(item.track.imagePath), sizes: "512x512" }]
        : [],
    });
  } catch {
    /* MediaMetadata missing in some webviews */
  }
}

export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "0:00";
  const whole = Math.floor(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}
