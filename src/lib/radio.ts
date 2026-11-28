/**
 * The radio, on the player's side (.spec/music/radio.json). Tuning in starts
 * a programme in the Rust core (radio.rs) and plays its first items; from
 * then on the player is kept a couple of items ahead, so the queue never runs
 * dry. Playing anything else, or stopping, leaves the radio.
 */
import { api } from "./api";
import { logCause } from "./log";
import {
  appendToQueue,
  playerState,
  playQueue,
  stopPlayback,
  subscribePlayer,
  type PlayerState,
  type QueueItem,
} from "./player";
import type { ProgrammeItem } from "./schemas";

/** How many items the player holds after the one playing. */
const KEEP = 2;
/** After the radio had nothing to give, wait this long before asking again. */
const EMPTY_PAUSE_MS = 15_000;

let on = false;
let asking = false;
let quietUntil = 0;
let lastPlaying: boolean | null = null;
let unsubscribe: (() => void) | null = null;

function toQueueItem(item: ProgrammeItem): QueueItem {
  return item.kind === "bridge" ? { bridge: item.bridge } : { song: item.song, track: item.track };
}

/** The radio is on in this run of the app. */
export function radioOn(): boolean {
  return on;
}

/**
 * Starts the radio: a new programme, played from its first items. Throws
 * when there is nothing to play at all yet.
 */
// Spec: RADIO-1, RADIO-13
export async function tuneIn(): Promise<void> {
  await api.radioStart();
  const first = await api.radioNext(KEEP);
  if (first.length === 0) {
    await api.radioStop();
    throw new Error("There is nothing to play yet. Make a song or a podcast first, or add the keys so the radio can.");
  }
  on = true;
  lastPlaying = null;
  quietUntil = 0;
  playQueue(first.map(toQueueItem), 0, { owner: "radio" });
  unsubscribe?.();
  unsubscribe = subscribePlayer(() => follow(playerState()));
}

/** Leaves the radio and stops what it was playing. */
export function tuneOut() {
  const wasPlaying = on && playerState().owner === "radio";
  leave();
  if (wasPlaying) stopPlayback();
}

function leave() {
  if (!on) return;
  on = false;
  unsubscribe?.();
  unsubscribe = null;
  void api.radioStop().catch((cause: unknown) => logCause("radio", cause));
}

/** Keeps the radio in step with the player. */
// Spec: RADIO-4, RADIO-11
function follow(player: PlayerState) {
  if (!on) return;
  // Something else is playing now, or nothing is.
  if (player.owner !== "radio" || player.queue.length === 0) {
    leave();
    return;
  }
  if (player.playing !== lastPlaying) {
    lastPlaying = player.playing;
    void api.radioPlaying(player.playing).catch((cause: unknown) => logCause("radio", cause));
  }
  const ahead = player.queue.length - player.index - 1;
  if (ahead >= KEEP || asking || Date.now() < quietUntil) return;
  asking = true;
  void api
    .radioNext(KEEP - ahead)
    .then((items) => {
      if (!on || playerState().owner !== "radio") return;
      if (items.length === 0) quietUntil = Date.now() + EMPTY_PAUSE_MS;
      appendToQueue(items.map(toQueueItem));
    })
    .catch((cause: unknown) => {
      logCause("radio", cause);
      quietUntil = Date.now() + EMPTY_PAUSE_MS;
    })
    .finally(() => {
      asking = false;
    });
}
