/**
 * The song player: one queue for the whole app, so music keeps playing while
 * the learner moves between tabs. It drives one of three engines:
 *
 *  - native (Android): the songplayer plugin plays through Media3 in a
 *    foreground service — lock screen, notification, Bluetooth and headphone
 *    controls, and playback with the screen off;
 *  - cast: a Sonos speaker plays the queue, steered by the Rust core;
 *  - web (desktop): an <audio> element, with Media Session controls where
 *    the webview offers them.
 *
 * Screens only see `PlayerState` and the functions below.
 */
import { useSyncExternalStore } from "react";
import { addPluginListener, convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { platform } from "@tauri-apps/plugin-os";

import { api, errorMessage } from "./api";
import type { CastDevice, CastState, Song, SongTrack } from "./types";

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
  /** Set while a Sonos speaker is playing the queue. */
  cast: CastState | null;
  /** The last playback problem, for the UI to show. */
  error: string | null;
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
  cast: null,
  error: null,
};

const listeners = new Set<() => void>();

function set(patch: Partial<PlayerState>) {
  state = { ...state, ...patch };
  listeners.forEach((listener) => listener());
}

function fail(cause: unknown) {
  set({ error: errorMessage(cause) });
}

const isAndroid = (() => {
  try {
    return platform() === "android";
  } catch {
    return false;
  }
})();

// ------------------------------------------------------------- play counts

/** A play counts once per pass, when the learner is half-way through. */
let counted: { trackId: number } | null = null;

function maybeCount(trackId: number | undefined, position: number, duration: number) {
  if (trackId === undefined) return;
  if (position < 5) counted = null; // back at the start: a new pass
  if (counted?.trackId === trackId) return;
  if (duration > 0 && position / duration > 0.5) {
    counted = { trackId };
    void api.songTrackPlayed(trackId).catch(() => {});
  }
}

// ------------------------------------------------------------------- native

interface NativeState {
  playing: boolean;
  index: number;
  id: string;
  position: number;
  duration: number;
  ended: boolean;
  remote: boolean;
}

let nativeListening = false;

function listenNative() {
  if (!isAndroid || nativeListening) return;
  nativeListening = true;
  void addPluginListener<NativeState>("songplayer", "state", (native) => {
    // While casting, the speaker's state is the truth.
    if (state.cast || native.remote) return;
    const index = state.queue.findIndex((item) => String(item.track.id) === native.id);
    const current = state.queue[index >= 0 ? index : native.index];
    set({
      index: index >= 0 ? index : native.index,
      playing: native.playing,
      position: native.position,
      duration: native.duration || current?.track.duration || 0,
    });
    maybeCount(current?.track.id, native.position, native.duration);
  }).catch(fail);
}

function native(command: string, args?: Record<string, unknown>) {
  listenNative();
  return invoke(`plugin:songplayer|${command}`, args).catch(fail);
}

// --------------------------------------------------------------------- web

let audio: HTMLAudioElement | null = null;

function element(): HTMLAudioElement {
  if (audio) return audio;
  audio = new Audio();
  audio.preload = "auto";
  // While a speaker plays, the local element's late events must not
  // overwrite the speaker's state (pausing it fires "pause" afterwards).
  audio.addEventListener("play", () => !state.cast && set({ playing: true }));
  audio.addEventListener("pause", () => !state.cast && set({ playing: false }));
  audio.addEventListener("loadedmetadata", () => {
    if (!state.cast) set({ duration: Number.isFinite(audio!.duration) ? audio!.duration : 0 });
  });
  audio.addEventListener("timeupdate", () => {
    if (state.cast) return;
    set({ position: audio!.currentTime });
    maybeCount(state.queue[state.index]?.track.id, audio!.currentTime, audio!.duration);
  });
  audio.addEventListener("ended", () => {
    if (state.cast) return;
    if (state.repeat === "one") {
      audio!.currentTime = 0;
      void audio!.play();
      return;
    }
    const last = state.index >= state.queue.length - 1;
    if (last && state.repeat === "off") {
      set({ playing: false, position: 0 });
      return;
    }
    webLoad((state.index + 1) % state.queue.length, true);
  });
  setupMediaSession();
  return audio;
}

function webLoad(index: number, autoplay: boolean) {
  const item = state.queue[index];
  if (!item) return;
  const a = element();
  a.src = convertFileSrc(item.track.audioPath);
  set({ index, position: 0, duration: item.track.duration || 0 });
  updateMediaMetadata(item);
  if (autoplay) void a.play().catch(() => set({ playing: false }));
}

// -------------------------------------------------------------------- cast

let castListening = false;

function listenCast() {
  if (castListening) return;
  castListening = true;
  void listen<CastState | null>("cast://state", (event) => applyCast(event.payload));
}

function applyCast(cast: CastState | null) {
  if (!cast) {
    if (state.cast) {
      set({ cast: null, playing: false });
      if (isAndroid) void native("remote_stop");
    }
    return;
  }
  const index = state.queue.findIndex((item) => item.track.id === cast.trackId);
  set({
    cast,
    playing: cast.playing,
    position: cast.position,
    duration: cast.duration,
    index: index >= 0 ? index : state.index,
  });
}

function castCommand(action: string, value?: number) {
  return api.castCommand(action, value).catch(fail);
}

async function startCast(device: CastDevice, index: number) {
  listenCast();
  const trackIds = state.queue.map((item) => item.track.id);
  if (trackIds.length === 0) throw new Error("Pick a song to play first.");
  // Hand over: the phone goes quiet, the speaker takes the queue.
  if (isAndroid) await native("pause");
  else audio?.pause();
  const cast = await api.castStart(device, trackIds, index, state.repeat);
  applyCast(cast);
  if (isAndroid) {
    const control = await api.castControlInfo();
    await native("remote_start", { port: control.port, token: control.token, deviceName: device.name });
  }
}

// ---------------------------------------------------------------- public API

function shuffled<T>(items: T[], first: number): T[] {
  const head = items[first];
  const rest = items.filter((_, i) => i !== first);
  for (let i = rest.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [rest[i], rest[j]] = [rest[j], rest[i]];
  }
  return head === undefined ? rest : [head, ...rest];
}

/** Starts `queue` at `index` — on the speaker if casting, else on the phone. */
export function playQueue(queue: QueueItem[], index = 0, options?: { shuffle?: boolean }) {
  if (queue.length === 0) return;
  const shuffle = options?.shuffle ?? false;
  const ordered = shuffle ? shuffled(queue, index) : queue;
  const start = shuffle ? 0 : index;
  counted = null;
  set({ queue: ordered, shuffle, index: start, position: 0, error: null });

  if (state.cast) {
    void startCast(state.cast.device, start).catch(fail);
  } else if (isAndroid) {
    void native("set_queue", {
      items: ordered.map((item) => ({
        id: String(item.track.id),
        path: item.track.audioPath,
        title: item.song.title,
        artist: item.song.chorusPl,
        artwork: item.track.imagePath,
      })),
      index: start,
      play: true,
      repeat: state.repeat,
    });
  } else {
    webLoad(start, true);
  }
}

export function toggle() {
  if (state.index < 0) return;
  if (state.cast) void castCommand(state.playing ? "pause" : "play");
  else if (isAndroid) void native("toggle");
  else {
    const a = element();
    if (a.paused) void a.play().catch(() => {});
    else a.pause();
  }
}

export function next() {
  if (state.queue.length === 0) return;
  if (state.cast) void castCommand("next");
  else if (isAndroid) void native("next");
  else webLoad((state.index + 1) % state.queue.length, true);
}

export function previous() {
  if (state.queue.length === 0) return;
  if (state.cast) void castCommand("previous");
  else if (isAndroid) void native("previous");
  else if (element().currentTime > 4) element().currentTime = 0;
  else webLoad((state.index - 1 + state.queue.length) % state.queue.length, true);
}

export function seek(seconds: number) {
  set({ position: seconds });
  if (state.cast) void castCommand("seek", seconds);
  else if (isAndroid) void native("seek_to", { seconds });
  else {
    const a = element();
    a.currentTime = Math.max(0, Math.min(seconds, a.duration || seconds));
  }
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
  if (state.cast) void api.castSetRepeat(repeat).catch(fail);
  else if (isAndroid) void native("set_repeat", { repeat });
}

export function stopPlayback() {
  if (state.cast) void castCommand("stop");
  if (isAndroid) void native("stop");
  audio?.pause();
  set({ queue: [], index: -1, playing: false, position: 0, duration: 0, cast: null });
}

/** Finds Sonos rooms. `address` asks one speaker directly. */
export async function discoverSpeakers(address?: string): Promise<CastDevice[]> {
  // Android drops multicast replies unless the app asks for them.
  if (isAndroid) await native("multicast", { enabled: true });
  try {
    return await api.castDiscover(address);
  } finally {
    if (isAndroid) void native("multicast", { enabled: false });
  }
}

/** Moves the current queue (or `queue`) to a speaker. */
export async function castTo(device: CastDevice, queue?: QueueItem[]) {
  if (queue && queue.length > 0 && state.queue.length === 0) {
    set({ queue, index: 0 });
  }
  set({ error: null });
  await startCast(device, Math.max(0, state.index));
}

/** Stops the speaker. Playback does not jump back to the phone by itself. */
export async function stopCasting() {
  await castCommand("stop");
  applyCast(null);
}

export function setCastVolume(volume: number) {
  if (state.cast) set({ cast: { ...state.cast, volume } });
  void castCommand("volume", volume);
}

/** Re-attaches to a cast that was running before the screen reloaded. */
export function resumeCast() {
  listenCast();
  void api
    .castState()
    .then((cast) => cast && applyCast(cast))
    .catch(() => {});
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
  if (current && !tracks.has(current.track.id) && !state.cast) {
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

export const playsNatively = isAndroid;

// ------------------------------------------------- desktop media controls

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
