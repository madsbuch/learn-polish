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
import { addPluginListener, invoke } from "@tauri-apps/api/core";

import { platform } from "@tauri-apps/plugin-os";

import { fileUrl } from "./files";
import { api, errorMessage } from "./api";
import { logCause } from "./log";
import type { Bridge, CastDevice, CastState, PlayableId, Song, SongTrack, TrackId } from "./schemas";
import { onEvent } from "./events";
import { CastStateSchema, NativePlayerStateSchema, PlatformSchema } from "./schemas";

export type RepeatMode = "all" | "one" | "off";

/** A take from the library, with its song. */
export interface TakeItem {
  song: Song;
  track: SongTrack;
  bridge?: undefined;
}

/** One of the radio's spoken bridges (RADIO-5). */
export interface BridgeItem {
  bridge: Bridge;
  song?: undefined;
  track?: undefined;
}

export type QueueItem = TakeItem | BridgeItem;

/** Who started the queue: the library (a song, a playlist…) or the radio. */
export type QueueOwner = "library" | "radio";

export function isBridge(item: QueueItem): item is BridgeItem {
  return item.bridge !== undefined;
}

/** The id the engines know an item by: its take's, or its bridge's. */
export function itemId(item: QueueItem): PlayableId {
  return item.bridge ? item.bridge.id : item.track.id;
}

/** What the screens and engines show for an item. */
export function itemTitle(item: QueueItem): { title: string; artist: string } {
  if (item.bridge) return { title: "Radio", artist: item.bridge.lines[0]?.pl ?? "" };
  return { title: item.song.title, artist: item.song.chorusPl };
}

function itemAudio(item: QueueItem) {
  return item.bridge
    ? { fileId: item.bridge.audioFileId, path: item.bridge.audioPath, duration: item.bridge.duration }
    : { fileId: item.track.audioFileId, path: item.track.audioPath, duration: item.track.duration };
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
  /** Who started the queue. */
  owner: QueueOwner;
  /** performance.now() when `position` was last reported, to extrapolate. */
  positionAt: number;
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
  owner: "library",
  positionAt: 0,
};

/** The radio's queue never repeats: it goes on with what the radio adds. */
function effectiveRepeat(): RepeatMode {
  return state.owner === "radio" ? "off" : state.repeat;
}

const listeners = new Set<() => void>();

function set(patch: Partial<PlayerState>) {
  if (patch.position !== undefined) patch.positionAt = performance.now();
  state = { ...state, ...patch };
  listeners.forEach((listener) => listener());
}

function fail(cause: unknown) {
  logCause("player", cause);
  set({ error: errorMessage(cause) });
}

const isAndroid = (() => {
  try {
    return PlatformSchema.parse(platform()) === "android";
  } catch {
    return false;
  }
})();

// ------------------------------------------------------------- play counts

/** A play counts once per pass, when the learner is half-way through. */
let counted: { id: PlayableId } | null = null;

function maybeCount(item: QueueItem | undefined, position: number, duration: number) {
  if (item === undefined) return;
  const id = itemId(item);
  if (position < 5) counted = null; // back at the start: a new pass
  if (counted?.id === id) return;
  if (duration > 0 && position / duration > 0.5) {
    counted = { id };
    const done = item.bridge ? api.radioBridgePlayed(item.bridge.id) : api.songTrackPlayed(item.track.id);
    void done.catch(() => {});
  }
}

// ------------------------------------------------------------------- native

let nativeListening = false;
let nativeCount = 0;

function listenNative() {
  if (!isAndroid || nativeListening) return;
  nativeListening = true;
  void addPluginListener("songplayer", "state", (payload: unknown) => {
    const parsed = NativePlayerStateSchema.safeParse(payload);
    if (!parsed.success) return;
    const native = parsed.data;
    // While casting, the speaker's state is the truth.
    if (state.cast || native.remote) return;
    // The queue went from something to nothing on the native side: closed
    // from the notification. The app's player goes too.
    const closed = nativeCount > 0 && native.count === 0;
    nativeCount = native.count;
    if (closed) {
      set({ queue: [], index: -1, playing: false, position: 0, duration: 0 });
      return;
    }
    const index = state.queue.findIndex((item) => String(itemId(item)) === native.id);
    const current = state.queue[index >= 0 ? index : native.index];
    set({
      index: index >= 0 ? index : native.index,
      playing: native.playing,
      position: native.position,
      duration: native.duration || (current ? itemAudio(current).duration : 0),
    });
    maybeCount(current, native.position, native.duration);
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
  const el = new Audio();
  audio = el;
  el.preload = "auto";
  // While a speaker plays, the local element's late events must not
  // overwrite the speaker's state (pausing it fires "pause" afterwards).
  el.addEventListener("play", () => !state.cast && set({ playing: true }));
  el.addEventListener("pause", () => !state.cast && set({ playing: false }));
  el.addEventListener("loadedmetadata", () => {
    if (!state.cast) set({ duration: Number.isFinite(el.duration) ? el.duration : 0 });
  });
  el.addEventListener("timeupdate", () => {
    if (state.cast) return;
    set({ position: el.currentTime });
    maybeCount(state.queue[state.index], el.currentTime, el.duration);
  });
  el.addEventListener("ended", () => {
    if (state.cast) return;
    const repeat = effectiveRepeat();
    if (repeat === "one") {
      el.currentTime = 0;
      void el.play();
      return;
    }
    const last = state.index >= state.queue.length - 1;
    if (last && repeat === "off") {
      // The radio may add more; it then goes on (appendToQueue).
      ranOut = true;
      set({ playing: false, position: 0 });
      return;
    }
    webLoad((state.index + 1) % state.queue.length, true);
  });
  setupMediaSession();
  return audio;
}

/** The web player reached the end of a queue that does not repeat. */
let ranOut = false;

function webLoad(index: number, autoplay: boolean) {
  const item = state.queue[index];
  if (!item) return;
  ranOut = false;
  const a = element();
  const audio = itemAudio(item);
  a.src = fileUrl(audio.fileId);
  set({ index, position: 0, duration: audio.duration || 0 });
  updateMediaMetadata(item);
  if (autoplay) void a.play().catch(() => set({ playing: false }));
}

// -------------------------------------------------------------------- cast

let castListening = false;

function listenCast() {
  if (castListening) return;
  castListening = true;
  void onEvent(CastStateSchema.nullable(), "cast://state", (event) => applyCast(event.payload));
}

function applyCast(cast: CastState | null) {
  if (!cast) {
    if (state.cast) {
      set({ cast: null, playing: false });
      if (isAndroid) void native("remote_stop");
    }
    return;
  }
  const index = state.queue.findIndex((item) => itemId(item) === cast.trackId);
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
  const trackIds = state.queue.map(itemId);
  if (trackIds.length === 0) throw new Error("Pick a song to play first.");
  // Hand over: the phone goes quiet, the speaker takes the queue.
  if (isAndroid) await native("pause");
  else audio?.pause();
  const cast = await api.castStart(device, trackIds, index, effectiveRepeat());
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

/** What Android's native player is handed for an item. */
function nativeItem(item: QueueItem) {
  const { title, artist } = itemTitle(item);
  return {
    id: String(itemId(item)),
    path: itemAudio(item).path,
    title,
    artist,
    artwork: item.track?.imagePath ?? null,
  };
}

/** Starts `queue` at `index` — on the speaker if casting, else on the phone. */
export function playQueue(
  queue: QueueItem[],
  index = 0,
  options?: { shuffle?: boolean; owner?: QueueOwner },
) {
  if (queue.length === 0) return;
  const shuffle = options?.shuffle ?? false;
  const ordered = shuffle ? shuffled(queue, index) : queue;
  const start = shuffle ? 0 : index;
  counted = null;
  set({ queue: ordered, shuffle, index: start, position: 0, error: null, owner: options?.owner ?? "library" });

  if (state.cast) {
    void startCast(state.cast.device, start).catch(fail);
  } else if (isAndroid) {
    void native("set_queue", {
      items: ordered.map(nativeItem),
      index: start,
      play: true,
      repeat: effectiveRepeat(),
    });
  } else {
    webLoad(start, true);
  }
}

/**
 * Adds to the end of the queue while it plays: the radio's programme
 * (RADIO-4). A queue that had run out goes on with them.
 */
// Spec: RADIO-4, RADIO-13
export function appendToQueue(items: QueueItem[]) {
  if (items.length === 0 || state.queue.length === 0) return;
  const first = state.queue.length;
  set({ queue: [...state.queue, ...items] });
  if (state.cast) {
    void api
      .castAppend(items.map(itemId))
      .then(applyCast)
      .catch(fail);
  } else if (isAndroid) {
    void native("add_items", { items: items.map(nativeItem) });
  } else if (ranOut) {
    webLoad(first, true);
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

/** Jumps to another song in the current queue. */
export function skipTo(index: number) {
  if (index < 0 || index >= state.queue.length) return;
  if (state.cast) playQueue(state.queue, index);
  else if (isAndroid) {
    set({ index, position: 0 });
    void native("skip_to", { index });
  } else webLoad(index, true);
}

// ------------------------------------------------------------ keep awake

let wantAwake = false;
let wakeLock: { release: () => Promise<void> } | null = null;

async function requestWebWakeLock() {
  if (!("wakeLock" in navigator) || wakeLock) return;
  try {
    wakeLock = await navigator.wakeLock.request("screen");
  } catch {
    /* not allowed right now (e.g. the page is hidden); retried on return */
  }
}

// The browser drops a wake lock when the page is hidden; take it back.
if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible" && wantAwake && !isAndroid) {
      wakeLock = null;
      void requestWebWakeLock();
    }
  });
}

// Who wants the screen on: the lyrics player, an open lesson… It stays on
// while anyone does, so one letting go doesn't switch it off for another.
const awakeHolders = new Set<string>();

/**
 * Keeps the screen from going to standby while `holder` wants it — following
 * the lyrics, studying a lesson with pen and paper. Android sets the window
 * flag natively; desktops use the browser's wake lock. Only ever applies
 * while the app is in front.
 */
export function keepScreenAwake(enabled: boolean, holder = "player") {
  if (enabled) awakeHolders.add(holder);
  else awakeHolders.delete(holder);
  applyAwake(awakeHolders.size > 0);
}

function applyAwake(enabled: boolean) {
  if (enabled === wantAwake) return;
  wantAwake = enabled;
  if (isAndroid) {
    void native("keep_awake", { enabled });
  } else if (enabled) {
    void requestWebWakeLock();
  } else {
    void wakeLock?.release().catch(() => {});
    wakeLock = null;
  }
}

/** Phones can hand a song to another app; desktops save it instead. */
export const canShare = isAndroid;

/** Opens the phone's share sheet with a take's MP3. */
export async function shareTrack(item: TakeItem) {
  const path = await api.songTrackShareFile(item.track.id);
  await invoke("plugin:songplayer|share_file", {
    path,
    mime: "audio/mpeg",
    title: item.song.title,
  });
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

/** Adopts the repeat mode stored in the database at startup. */
export function syncRepeatFromSettings(stored: "" | RepeatMode) {
  if (stored === "") {
    void api.updateSettings({ playerRepeat: state.repeat }).catch(() => {});
  } else if (stored !== state.repeat) {
    try {
      localStorage.setItem(REPEAT_KEY, stored);
    } catch {
      /* applies for this run */
    }
    set({ repeat: stored });
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
  void api.updateSettings({ playerRepeat: repeat }).catch(() => {});
  set({ repeat });
  if (state.cast) void api.castSetRepeat(effectiveRepeat()).catch(fail);
  else if (isAndroid) void native("set_repeat", { repeat: effectiveRepeat() });
}

export function stopPlayback() {
  if (state.cast) void castCommand("stop");
  if (isAndroid) void native("stop");
  audio?.pause();
  set({ queue: [], index: -1, playing: false, position: 0, duration: 0, cast: null, owner: "library" });
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
  const tracks = new Map<TrackId, TakeItem>();
  for (const song of songs) for (const track of song.tracks) tracks.set(track.id, { song, track });
  const kept = keptInLibrary(state, tracks);
  // The song playing was deleted: it stops (a speaker plays on by itself).
  if (state.index >= 0 && kept.index < 0 && !state.cast) {
    stopPlayback();
    return;
  }
  set(kept);
}

/**
 * The queue with each take brought up to date and deleted takes left out.
 * The radio's bridges are not in the library and stay (RADIO-6). Exported
 * for the tests.
 */
export function keptInLibrary(
  player: Pick<PlayerState, "queue" | "index">,
  tracks: Map<TrackId, TakeItem>,
): Pick<PlayerState, "queue" | "index"> {
  const current = player.queue[player.index];
  const queue = player.queue
    .map((item) => (item.bridge ? item : tracks.get(item.track.id)))
    .filter((item): item is QueueItem => Boolean(item));
  const index = current ? queue.findIndex((i) => itemId(i) === itemId(current)) : -1;
  return { queue, index };
}

/**
 * The take of a song that plays from the library: the one played most (the
 * learner's favourite, in practice), else the first. A song's other take is
 * never queued after it — it is the same song again.
 */
export function preferredTake(song: Song): SongTrack | null {
  let best: SongTrack | null = null;
  for (const track of song.tracks) {
    if (!best || track.playCount > best.playCount) best = track;
  }
  return best;
}

/** One take per finished song, in library order — what "play all" plays. */
export function libraryQueue(songs: Song[], chosen?: SongTrack): TakeItem[] {
  return songs
    .filter((song) => song.status === "ready")
    .map((song) => {
      const track = chosen && chosen.songId === song.id ? chosen : preferredTake(song);
      return track ? { song, track } : null;
    })
    .filter((item): item is TakeItem => item !== null);
}

/** Follows the player outside React (the radio does). */
export function subscribePlayer(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function playerState(): PlayerState {
  return state;
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
    const { title, artist } = itemTitle(item);
    const image = item.track?.imageFileId ?? null;
    navigator.mediaSession.metadata = new MediaMetadata({
      title,
      artist,
      album: "Polski",
      artwork: image !== null ? [{ src: fileUrl(image), sizes: "512x512" }] : [],
    });
  } catch {
    /* MediaMetadata missing in some webviews */
  }
}

/**
 * The position right now. Native players and speakers report about once a
 * second; between reports the song has moved on by the time elapsed.
 */
export function livePosition(player: PlayerState): number {
  if (!player.playing) return player.position;
  const elapsed = (performance.now() - player.positionAt) / 1000;
  const estimate = player.position + Math.min(Math.max(elapsed, 0), 2);
  return player.duration > 0 ? Math.min(estimate, player.duration) : estimate;
}

export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "0:00";
  const whole = Math.floor(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}
