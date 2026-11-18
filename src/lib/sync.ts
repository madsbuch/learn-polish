// Sync between the learner's devices, as the screens see it (docs/SYNC.md):
// reloading when another device's data arrives, and saying what sync holds
// and does in plain words.

import { useEffect, useRef } from "react";

import { onSyncChanged } from "./events";
import type { SyncHeld } from "./types";

/** A burst of incoming batches reloads the screens once, this long after the last. */
const RELOAD_DEBOUNCE_MS = 1200;

const reloaders = new Set<() => void>();
let listening = false;
let timer: number | undefined;

function listen() {
  if (listening) return;
  listening = true;
  void onSyncChanged(() => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      for (const reload of reloaders) reload();
    }, RELOAD_DEBOUNCE_MS);
  }).catch(() => {
    // No Tauri (a browser preview): nothing ever arrives.
    listening = false;
  });
}

/**
 * Calls `reload` when another device's data has arrived (debounced: one call
 * for a whole sync). For screens that show data they loaded themselves. A
 * screen where the learner is typing or practising shouldn't use it: it
 * finishes on the data it has.
 */
export function useSyncReload(reload: () => void) {
  const latest = useRef(reload);
  useEffect(() => {
    latest.current = reload;
  });
  useEffect(() => {
    listen();
    const call = () => latest.current();
    reloaders.add(call);
    return () => {
      reloaders.delete(call);
    };
  }, []);
}

/** "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago", or a date. */
export function ago(iso: string, now = Date.now()): string {
  const at = Date.parse(iso);
  if (!Number.isFinite(at)) return "";
  const seconds = Math.max(0, Math.round((now - at) / 1000));
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  if (days === 1) return "yesterday";
  if (days < 7) return `${days} days ago`;
  return new Date(at).toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

/** How a table's rows are counted in words: one and many, or a phrase for any number. */
type Words = { one: string; many: string } | { phrase: string };

/**
 * The learner's tables in the order they matter to them. Tables missing here
 * are parts of these (a conversation's messages, a song's takes) and go
 * unsaid; an unknown one (a later schema) is "other data".
 */
const HELD_WORDS: [string, Words][] = [
  ["courses", { one: "course", many: "courses" }],
  ["lessons", { one: "lesson", many: "lessons" }],
  ["words", { one: "word", many: "words" }],
  ["sentences", { one: "sentence", many: "sentences" }],
  ["conversation_lines", { one: "dialogue line", many: "dialogue lines" }],
  ["songs", { one: "song", many: "songs" }],
  ["playlists", { one: "playlist", many: "playlists" }],
  ["tutor_conversations", { one: "tutor conversation", many: "tutor conversations" }],
  ["memories", { one: "memory", many: "memories" }],
  ["checkins", { one: "check-in", many: "check-ins" }],
  ["achievements", { one: "achievement", many: "achievements" }],
  ["reviews", { phrase: "your review history" }],
  ["xp_log", { phrase: "your XP history" }],
  ["day_log", { phrase: "your XP history" }],
  ["speech_clips", { phrase: "saved audio" }],
  ["ai_costs", { phrase: "your AI costs" }],
  ["bench_runs", { phrase: "benchmark results" }],
];

const UNSAID = new Set([
  "course_lessons",
  "tutor_messages",
  "checkin_messages",
  "song_tracks",
  "playlist_tracks",
  "word_examples",
  "wiki_sources",
  "files",
]);

/** "4 courses, 120 words and your XP history": what this device holds, in words. */
export function describeHeld(held: SyncHeld[]): string {
  const rows = new Map(held.map((entry) => [entry.table, entry.rows]));
  const parts: string[] = [];
  for (const [table, words] of HELD_WORDS) {
    const n = rows.get(table) ?? 0;
    if (n <= 0) continue;
    const part = "phrase" in words ? words.phrase : `${n.toLocaleString()} ${n === 1 ? words.one : words.many}`;
    if (!parts.includes(part)) parts.push(part);
  }
  const known = new Set(HELD_WORDS.map(([table]) => table));
  if (held.some((entry) => entry.rows > 0 && !known.has(entry.table) && !UNSAID.has(entry.table))) {
    parts.push("other data");
  }
  if (parts.length === 0) return held.length > 0 ? "some data of its own" : "nothing";
  if (parts.length === 1) return parts[0];
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

/** A sync error, as the learner can act on it. */
export function syncErrorText(error: string): string {
  const e = error.toLowerCase();
  if (e.includes("no answer") || e.includes("refused") || e.includes("unreachable") || e.includes("couldn't reach"))
    return "Couldn't reach the other device. Is Polski open on it, on the same Wi-Fi?";
  if (e.includes("different version")) return "The other device runs another version of Polski. Update both.";
  if (e.includes("expired or was already used")) return "That code has expired or was used already. Show a new one.";
  if (e.includes("not a polski pairing code") || e.includes("damaged")) return "That isn't a Polski code, or it was cut short.";
  if (e.includes("removed from this vault") || e.includes("no longer) in this vault"))
    return "This device was removed from your devices. Join again to sync.";
  if (e.includes("a sync is running")) return "A sync is running. Try again in a moment.";
  if (e.includes("local network")) return "This device isn't on a Wi-Fi network.";
  // Anything else: the core's own words, capitalised.
  return error.charAt(0).toUpperCase() + error.slice(1);
}
