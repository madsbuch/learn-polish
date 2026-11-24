/**
 * Where every page lives, and the one way of getting to it.
 *
 * What's on screen is the address's to say. An address is a tab, then each
 * page the open one was opened through, things named by their id:
 * `/crs_…/ls_…` is a lesson opened from its course, `/tutor/cv_…/ls_…` the
 * same lesson opened from a tutor conversation. Anything that shows a page —
 * the tab bar, a row, an id chip, a notification — asks `go` for its address.
 *
 * The browser history always holds the up-chain of the current address:
 * Exercises, then the tab, then each page above the open one, then any open
 * sheets. So back is always "up" — Android's button (the web view's history),
 * a page's ←, a summary's Done and a sheet's ✕ do the same thing however the
 * page was reached, and the app is left only from Exercises. `go` keeps that
 * shape: it walks the history back to where the old and new chains part,
 * replaces that entry, and pushes the rest. Sheets hold one entry each above
 * the page (`useBackLayer`) and have no address of their own.
 *
 * Kept hand-rolled: routes are trails, not a fixed table, and the history is
 * managed by hand (`whenSettled` waits for every back asked for before the
 * next push, as `history.go` lands a tick or two later).
 */
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { z } from "zod";

import { markBack } from "../design/motion";
import {
  ConversationIdSchema,
  CourseIdSchema,
  HardKindSchema,
  LastShownSchema,
  LessonIdSchema,
  parseJson,
  PlaylistIdSchema,
  QuickGameKindSchema,
  RawPresetSchema,
  SongIdSchema,
  SourceIdSchema,
  WordIdSchema,
  type ConversationId,
  type CourseId,
  type HardKind,
  type LessonId,
  type PlaylistId,
  type QuickGameKind,
  type RawPreset,
  type SongId,
  type SourceId,
  type WordId,
} from "./schemas";

// ------------------------------------------------------------ the addresses

/** The app's tabs (`nav.tabs`); Exercises is "/". */
export const TABS = ["exercises", "tutor", "music", "settings"] as const;
export type Tab = (typeof TABS)[number];

/** The Settings pages, each with the word its address uses. */
export const SettingsPageSchema = z.enum([
  "howTo",
  "practice",
  "memories",
  "ai",
  "sound",
  "songs",
  "appearance",
  "devices",
  "data",
]);
export type SettingsPage = z.infer<typeof SettingsPageSchema>;
const SETTINGS_WORD: Record<SettingsPage, string> = {
  howTo: "how-to",
  practice: "practice",
  memories: "memories",
  ai: "ai",
  sound: "sound",
  songs: "songs",
  appearance: "appearance",
  devices: "devices",
  data: "data",
};

/** A course page's sections; Lessons is the course's own address. */
export const CourseSectionSchema = z.enum(["lessons", "words", "sentences"]);
export type CourseSection = z.infer<typeof CourseSectionSchema>;

export type { RawPreset };

/** A "What you've met" or Insights page, under Progress. */
export type InsightPage =
  | { page: "words" }
  | { page: "word"; wordId: WordId }
  | { page: "structures" }
  | { page: "structure"; key: string }
  | { page: "sounds" }
  | { page: "overview" }
  | { page: "hard"; kind: HardKind }
  | { page: "mistakes"; cls: string | null }
  | { page: "compare" }
  | { page: "activity" }
  | { page: "item"; itemId: string }
  /** Every answer and event, filtered: a drill-down presets some filters. */
  | { page: "rawlog"; preset?: RawPreset; title?: string }
  | { page: "topics" }
  | { page: "topic"; key: string };

/** One page of an address. */
export type Segment =
  | { kind: "course"; id: CourseId; section: CourseSection }
  | { kind: "lesson"; id: LessonId }
  /** A Wikipedia source read in the app, under its lesson or song. */
  | { kind: "source"; id: SourceId }
  | { kind: "drill"; challenge: boolean }
  /** A conversation lesson's conversation (or a check-in held as one). */
  | { kind: "talk" }
  /** The summary after a drill or a conversation. */
  | { kind: "done" }
  | { kind: "song"; id: SongId }
  | { kind: "playlist"; id: PlaylistId }
  | { kind: "compose"; what: "song" | "podcast" }
  /** The full player. */
  | { kind: "playing" }
  /** A tutor conversation; "new" until its first reply names it. */
  | { kind: "chat"; id: ConversationId | "new" }
  | { kind: "settings"; page: SettingsPage }
  /** Downloaded audio, and a course in it. */
  | { kind: "audio" }
  | { kind: "audioCourse"; id: CourseId }
  | { kind: "search" }
  | { kind: "progress" }
  /** What earned a day's XP. */
  | { kind: "day"; day: number }
  | { kind: "achievement"; code: string }
  | { kind: "insight"; page: InsightPage }
  | { kind: "games" }
  | { kind: "game"; game: QuickGameKind };

export interface Address {
  tab: Tab;
  trail: Segment[];
}

/** A tab's own page. */
export function tabRoot(tab: Tab): Address {
  return { tab, trail: [] };
}

export const HOME: Address = tabRoot("exercises");

const TAB_WORDS: Record<Exclude<Tab, "exercises">, string> = { tutor: "tutor", music: "music", settings: "settings" };
const SECTION_WORDS = new Set(["words", "sentences"]);
const ISO_DAY = /^\d{4}-\d{2}-\d{2}$/;

function dayToIso(day: number): string {
  return new Date(day * 86_400_000).toISOString().slice(0, 10);
}

function isoToDay(text: string): number | null {
  if (!ISO_DAY.test(text)) return null;
  const at = Date.parse(`${text}T00:00:00Z`);
  return Number.isNaN(at) ? null : Math.round(at / 86_400_000);
}

function settingsPageOf(word: string | undefined): SettingsPage | null {
  for (const page of SettingsPageSchema.options) if (SETTINGS_WORD[page] === word) return page;
  return null;
}

function decode(token: string): string | null {
  try {
    return decodeURIComponent(token);
  } catch {
    return null;
  }
}

/** Whether settings pages are named by their bare word here (inside Settings). */
function settingsContext(tab: Tab, prev: Segment | null): boolean {
  return (tab === "settings" && prev === null) || prev?.kind === "settings";
}

function insightContext(prev: Segment | null): boolean {
  // A summary opens a missed word's or sentence's page above it.
  // Spec: SUM-7
  return prev?.kind === "progress" || prev?.kind === "insight" || prev?.kind === "done";
}

/** The path parts of one segment, given the segment before it. */
function formatSegment(seg: Segment, tab: Tab, prev: Segment | null): string[] {
  switch (seg.kind) {
    case "course":
      return seg.section === "lessons" ? [seg.id] : [seg.id, seg.section];
    case "lesson":
    case "source":
    case "song":
    case "playlist":
    case "audioCourse":
      return [seg.id];
    case "drill":
      return [seg.challenge ? "drill;challenge" : "drill"];
    case "talk":
    case "done":
    case "playing":
    case "audio":
    case "search":
    case "progress":
    case "games":
      return [seg.kind];
    case "compose":
      return ["new", seg.what];
    case "chat":
      return [seg.id];
    case "settings":
      return settingsContext(tab, prev) ? [SETTINGS_WORD[seg.page]] : ["settings", SETTINGS_WORD[seg.page]];
    case "day":
      return ["day", dayToIso(seg.day)];
    case "achievement":
      return ["achievements", encodeURIComponent(seg.code)];
    case "game":
      return [seg.game];
    case "insight":
      return formatInsight(seg.page);
  }
}

function formatInsight(page: InsightPage): string[] {
  switch (page.page) {
    case "word":
      return [page.wordId];
    case "structure":
      return ["structure", encodeURIComponent(page.key)];
    case "hard":
      return ["hard", page.kind];
    case "mistakes":
      return ["mistakes", page.cls === null ? "all" : encodeURIComponent(page.cls)];
    case "item":
      return ["item", encodeURIComponent(page.itemId)];
    case "topic":
      return ["topic", encodeURIComponent(page.key)];
    case "rawlog": {
      const params = [
        page.preset ? `f=${encodeURIComponent(JSON.stringify(page.preset))}` : null,
        page.title ? `t=${encodeURIComponent(page.title)}` : null,
      ].filter((part): part is string => part !== null);
      return [["log", ...params].join(";")];
    }
    default:
      return [page.page];
  }
}

/** An address as a path: `/tutor/cv_…/ls_…`. */
// Spec: SHELL-16
export function format(address: Address): string {
  const parts: string[] = address.tab === "exercises" ? [] : [TAB_WORDS[address.tab]];
  let prev: Segment | null = null;
  for (const seg of address.trail) {
    parts.push(...formatSegment(seg, address.tab, prev));
    prev = seg;
  }
  return `/${parts.join("/")}`;
}

interface Parsed {
  segment: Segment;
  /** The tokens it took. */
  used: number;
}

const INSIGHT_WORDS = ["words", "structures", "sounds", "overview", "compare", "activity", "topics"] as const;

function parseInsight(token: string, next: string | undefined): Parsed | null {
  const word = WordIdSchema.safeParse(token);
  if (word.success) return { segment: { kind: "insight", page: { page: "word", wordId: word.data } }, used: 1 };
  for (const page of INSIGHT_WORDS) if (token === page) return { segment: { kind: "insight", page: { page } }, used: 1 };
  if (token === "log" || token.startsWith("log;")) {
    let preset: RawPreset | undefined;
    let title: string | undefined;
    for (const part of token.split(";").slice(1)) {
      const value = decode(part.slice(2));
      if (value === null) return null;
      if (part.startsWith("f=")) {
        const parsed = parseJson(RawPresetSchema, value);
        if (!parsed.success) return null;
        preset = parsed.data;
      } else if (part.startsWith("t=")) title = value;
      else return null;
    }
    const page: InsightPage = { page: "rawlog" };
    if (preset) page.preset = preset;
    if (title !== undefined) page.title = title;
    return { segment: { kind: "insight", page }, used: 1 };
  }
  if (next === undefined) return null;
  const param = decode(next);
  if (param === null) return null;
  const one = (page: InsightPage): Parsed => ({ segment: { kind: "insight", page }, used: 2 });
  switch (token) {
    case "structure":
      return one({ page: "structure", key: param });
    case "hard": {
      const kind = HardKindSchema.safeParse(param);
      return kind.success ? one({ page: "hard", kind: kind.data }) : null;
    }
    case "mistakes":
      return one({ page: "mistakes", cls: param === "all" ? null : param });
    case "item":
      return one({ page: "item", itemId: param });
    case "topic":
      return one({ page: "topic", key: param });
    default:
      return null;
  }
}

/** One segment at `tokens[i]`, if the tokens there make one where it stands. */
function parseSegment(tokens: string[], i: number, tab: Tab, prev: Segment | null): Parsed | null {
  const token = tokens[i];
  const next = tokens[i + 1];
  const atRoot = prev === null;

  if (settingsContext(tab, prev)) {
    const page = settingsPageOf(token);
    if (page) return { segment: { kind: "settings", page }, used: 1 };
  }
  if (token === "settings") {
    const page = settingsPageOf(next);
    if (page) return { segment: { kind: "settings", page }, used: 2 };
  }
  if (insightContext(prev)) {
    const insight = parseInsight(token, next);
    if (insight) return insight;
  }

  const course = CourseIdSchema.safeParse(token);
  if (course.success) {
    if (prev?.kind === "audio") return { segment: { kind: "audioCourse", id: course.data }, used: 1 };
    const section = next !== undefined && SECTION_WORDS.has(next) ? CourseSectionSchema.safeParse(next) : null;
    return section?.success
      ? { segment: { kind: "course", id: course.data, section: section.data }, used: 2 }
      : { segment: { kind: "course", id: course.data, section: "lessons" }, used: 1 };
  }
  const lesson = LessonIdSchema.safeParse(token);
  if (lesson.success) return { segment: { kind: "lesson", id: lesson.data }, used: 1 };
  const source = SourceIdSchema.safeParse(token);
  if (source.success)
    return prev?.kind === "lesson" || prev?.kind === "song"
      ? { segment: { kind: "source", id: source.data }, used: 1 }
      : null;
  const song = SongIdSchema.safeParse(token);
  if (song.success) return { segment: { kind: "song", id: song.data }, used: 1 };
  const playlist = PlaylistIdSchema.safeParse(token);
  if (playlist.success)
    return tab === "music" && atRoot ? { segment: { kind: "playlist", id: playlist.data }, used: 1 } : null;
  const chat = ConversationIdSchema.safeParse(token);
  if (chat.success) return { segment: { kind: "chat", id: chat.data }, used: 1 };

  if (prev?.kind === "lesson") {
    if (token === "drill") return { segment: { kind: "drill", challenge: false }, used: 1 };
    if (token === "drill;challenge") return { segment: { kind: "drill", challenge: true }, used: 1 };
    if (token === "talk" || token === "done") return { segment: { kind: token }, used: 1 };
  }
  if (token === "new") {
    if (next === "song" || next === "podcast")
      return tab === "music" && atRoot ? { segment: { kind: "compose", what: next }, used: 2 } : null;
    return { segment: { kind: "chat", id: "new" }, used: 1 };
  }
  if (token === "playing") return tab === "music" && atRoot ? { segment: { kind: "playing" }, used: 1 } : null;
  if (token === "audio") return prev?.kind === "settings" ? { segment: { kind: "audio" }, used: 1 } : null;
  if (tab === "exercises" && atRoot && (token === "search" || token === "progress" || token === "games"))
    return { segment: { kind: token }, used: 1 };
  if (prev?.kind === "games") {
    const game = QuickGameKindSchema.safeParse(token);
    if (game.success) return { segment: { kind: "game", game: game.data }, used: 1 };
  }
  if (token === "day" && ((tab === "exercises" && atRoot) || prev?.kind === "progress")) {
    const day = next === undefined ? null : isoToDay(next);
    return day === null ? null : { segment: { kind: "day", day }, used: 2 };
  }
  if (token === "achievements" && prev?.kind === "progress" && next !== undefined) {
    const code = decode(next);
    return code ? { segment: { kind: "achievement", code }, used: 2 } : null;
  }
  return null;
}

/**
 * The address a path names, as far as it makes sense: an unknown or invalid
 * part ends it there, and `complete` says whether the whole path was read.
 */
export function parseLenient(path: string): { address: Address; complete: boolean } {
  const tokens = path.split("?")[0].split("/").filter((t) => t !== "");
  let tab: Tab = "exercises";
  let i = 0;
  if (tokens[0] === "tutor" || tokens[0] === "music" || tokens[0] === "settings") {
    tab = tokens[0];
    i = 1;
  }
  const trail: Segment[] = [];
  while (i < tokens.length) {
    const parsed = parseSegment(tokens, i, tab, trail[trail.length - 1] ?? null);
    if (!parsed) break;
    trail.push(parsed.segment);
    i += parsed.used;
  }
  return { address: { tab, trail }, complete: i === tokens.length };
}

/** The address a path names, or null when any of it isn't one. */
export function parse(path: string): Address | null {
  const { address, complete } = parseLenient(path);
  return complete ? address : null;
}

/** Whether an address is one the app can show as it stands. */
export function isValid(address: Address): boolean {
  return parse(format(address)) !== null && format(parse(format(address)) ?? HOME) === format(address);
}

/**
 * The page back goes to: the one this page was opened from, then the tab,
 * then Exercises; null on Exercises itself. A summary goes back past its
 * lesson, to where the lesson was started from.
 */
// Spec: SHELL-3
export function up(address: Address): Address | null {
  const { tab, trail } = address;
  if (trail.length === 0) return tab === "exercises" ? null : HOME;
  const last = trail[trail.length - 1];
  const keep = last.kind === "done" ? trail.length - 2 : trail.length - 1;
  return { tab, trail: trail.slice(0, Math.max(0, keep)) };
}

/** Every address back leads through to here: Exercises first, the address last. */
export function chain(address: Address): Address[] {
  const list = [address];
  for (let above = up(address); above; above = up(above)) list.unshift(above);
  return list;
}

/** What makes a segment the same page as another, for opening one that is already in the trail. */
function identity(seg: Segment): string | null {
  switch (seg.kind) {
    case "course":
    case "lesson":
    case "song":
    case "playlist":
    case "source":
      return seg.id;
    case "chat":
      return seg.id === "new" ? null : seg.id;
    case "settings":
      return `settings:${seg.page}`;
    case "insight":
      return formatInsight(seg.page).join("/");
    default:
      return null;
  }
}

/** The most pages an address holds (`nav.max-depth`). */
export const MAX_DEPTH = 8;

/**
 * `seg` opened from the page at `from`: on top of it, in the same tab. One
 * already in the trail is gone back to instead, and the oldest pages give way
 * past `MAX_DEPTH`.
 */
// Spec: SHELL-17, SHELL-25
export function openFrom(from: Address, seg: Segment): Address {
  const id = identity(seg);
  if (id !== null) {
    const at = from.trail.findIndex((s) => identity(s) === id);
    if (at !== -1) return { tab: from.tab, trail: from.trail.slice(0, at + 1) };
  }
  let trail = [...from.trail, seg];
  while (trail.length > MAX_DEPTH) trail = trail.slice(1);
  while (trail.length > 0 && !isValid({ tab: from.tab, trail })) trail = trail.slice(1);
  return { tab: from.tab, trail };
}

/** The same address with its last page swapped for `seg` (another section, another item of a list). */
export function swapLast(from: Address, seg: Segment): Address {
  return { tab: from.tab, trail: [...from.trail.slice(0, -1), seg] };
}

/** Pages not reopened when the app is: a drill, a summary, the player, a form. */
const TRANSIENT = new Set<Segment["kind"]>(["drill", "talk", "done", "playing", "compose"]);

function transient(seg: Segment): boolean {
  return TRANSIENT.has(seg.kind) || (seg.kind === "chat" && seg.id === "new");
}

/** The address to reopen the app on: the page under any drill, summary or player. */
export function forResume(address: Address): Address {
  const cut = address.trail.findIndex(transient);
  return cut === -1 ? address : { tab: address.tab, trail: address.trail.slice(0, cut) };
}

/** What a tab shows when its button is tapped again later: its own first page, if any. */
function tabPlace(address: Address): Address {
  const first = address.trail[0];
  if (address.tab === "exercises" || !first || transient(first)) return tabRoot(address.tab);
  return { tab: address.tab, trail: [first] };
}

/** How to get the history from chain `held` to chain `wanted`. */
export interface HistoryPlan {
  /** Entries to go back first. */
  back: number;
  /** The entry landed on, replaced with this; null to keep it. */
  replace: string | null;
  /** Then pushed, in order. */
  push: string[];
  /** Going to a page above: what's shown comes back rather than opens. */
  isReturn: boolean;
}

// Spec: SHELL-3, SHELL-16
export function planHistory(held: string[], wanted: string[]): HistoryPlan {
  let k = 0;
  while (k < held.length && k < wanted.length && held[k] === wanted[k]) k++;
  if (k === wanted.length) return { back: held.length - k, replace: null, push: [], isReturn: held.length > k };
  if (k === held.length) return { back: 0, replace: null, push: wanted.slice(k), isReturn: false };
  return { back: held.length - 1 - k, replace: wanted[k], push: wanted.slice(k + 1), isReturn: false };
}

// ------------------------------------------------------------ the history

const APP = "polski";
/** Where the address shown last is kept, for reopening the app on it. */
const LAST_KEY = `${APP}.nav.last`;
/** How long after leaving the app it reopens where it was (`nav.resume-within`). */
export const RESUME_WITHIN_MS = 30 * 60_000;

/** This page load: entries of an earlier one (before a reload) aren't ours. */
const SESSION = Math.random().toString(36).slice(2, 10);

const EntryStateSchema = z.object({
  session: z.string(),
  depth: z.number(),
  key: z.string(),
  sheet: z.boolean().optional(),
});
type EntryState = z.infer<typeof EntryStateSchema>;
const HistoryStateSchema = z.object({ [APP]: EntryStateSchema });

function readState(): EntryState | null {
  const parsed = HistoryStateSchema.safeParse(window.history.state);
  return parsed.success ? parsed.data[APP] : null;
}

function stateOf(entry: EntryState): Record<string, EntryState> {
  return { [APP]: entry };
}

interface Entry {
  path: string;
  key: string;
}

/** The pages in the history, Exercises first (sheets not counted). */
let entries: Entry[] = [];
let current: Address = typeof window === "undefined" ? HOME : parseLenient(window.location.pathname).address;
/** Where each tab was last left (SHELL-22). */
const lastInTab = new Map<Tab, Address>();
const subscribers = new Set<() => void>();
let started = false;

function newKey(): string {
  return Math.random().toString(36).slice(2, 10);
}

function show(address: Address): void {
  current = address;
  lastInTab.set(address.tab, tabPlace(address));
  try {
    localStorage.setItem(LAST_KEY, JSON.stringify({ path: format(address), at: Date.now() }));
  } catch {
    // No storage: the app reopens on Exercises.
  }
  for (const notify of subscribers) notify();
}

function subscribe(notify: () => void): () => void {
  subscribers.add(notify);
  return () => subscribers.delete(notify);
}

/** The address shown now. */
export function currentAddress(): Address {
  return current;
}

/** The address shown, following it as it changes. */
export function useAddress(): Address {
  return useSyncExternalStore(subscribe, () => current);
}

function topKey(): string {
  return entries[entries.length - 1]?.key ?? "";
}

/** The history entry of the page shown: what a page's remembered scroll and hand-offs are kept under. */
export function useEntryKey(): string {
  return useSyncExternalStore(subscribe, topKey);
}

// Sheets and guards: one history entry each above the page.

interface Layer {
  back: () => void;
  /** A sheet closes when a navigation passes it; a guard just goes. */
  sheet: boolean;
  pushed: boolean;
  cancelled: boolean;
}

const layers: Layer[] = [];
/** Backs asked of the browser that haven't landed yet. */
let pendingBacks = 0;
/** Held until they have. */
const queued: (() => void)[] = [];

function whenSettled(fn: () => void): void {
  if (pendingBacks === 0) fn();
  else queued.push(fn);
}

function backLanded(): void {
  pendingBacks = Math.max(0, pendingBacks - 1);
  if (pendingBacks === 0) for (const fn of queued.splice(0)) fn();
}

/** Goes back `steps` entries and resolves once that has landed. */
function traverse(steps: number): Promise<void> {
  return new Promise((resolve) => {
    if (steps <= 0) {
      resolve();
      return;
    }
    pendingBacks++;
    queued.push(resolve);
    // A traversal that never lands (nothing that far back) mustn't stall
    // every navigation after it.
    const landed = pendingBacks;
    window.setTimeout(() => {
      if (pendingBacks >= landed && queued.includes(resolve)) backLanded();
    }, 1500);
    window.history.go(-steps);
  });
}

/** Takes every open sheet and guard off the history (sheets close), then resolves. */
function clearLayers(): Promise<void> {
  return new Promise((resolve) =>
    whenSettled(() => {
      const open = layers.splice(0);
      for (const layer of open.reverse()) if (layer.sheet) layer.back();
      void traverse(open.length).then(resolve);
    }),
  );
}

/**
 * While `active`, one history entry above the page, so back (Android's
 * button, the mouse's, Cmd+[) runs `onBack` instead of leaving the page. A
 * sheet passes `sheet` (a navigation closes it on the way); a guard — a drill
 * asking before it is left — doesn't. Still active after a back, it takes an
 * entry again.
 */
export function useBackLayer(active: boolean, onBack: () => void, kind: "sheet" | "guard" = "sheet"): void {
  const backRef = useRef(onBack);
  backRef.current = onBack;
  const [round, setRound] = useState(0);
  useEffect(() => {
    if (!active) return;
    const layer: Layer = {
      back: () => {
        backRef.current();
        setRound((n) => n + 1);
      },
      sheet: kind === "sheet",
      pushed: false,
      cancelled: false,
    };
    whenSettled(() => {
      if (layer.cancelled) return;
      const below = readState();
      if (!below) return;
      layers.push(layer);
      layer.pushed = true;
      window.history.pushState(stateOf({ ...below, sheet: true }), "");
    });
    return () => {
      layer.cancelled = true;
      const at = layers.indexOf(layer);
      if (at !== -1) layers.splice(at, 1);
      // Only an entry still held is given back: a back that ran this layer
      // has used its entry already.
      if (layer.pushed && at !== -1) {
        pendingBacks++;
        window.history.back();
      }
    };
  }, [active, round, kind]);
}

function onPop(): void {
  if (pendingBacks > 0) {
    backLanded();
    return;
  }
  const layer = layers.pop();
  if (layer) {
    layer.back();
    return;
  }
  const state = readState();
  if (!state || state.session !== SESSION) {
    // Below this load's first entry: back from Exercises, past entries an
    // earlier load of the page left (a reload). Leave, as from Exercises.
    if (state && entries.length <= 1) {
      window.history.go(-(state.depth + 1));
      return;
    }
    rebuildFrom(window.location.pathname);
    return;
  }
  if (state.sheet) {
    // A sheet's entry whose sheet is gone: step past it.
    window.history.back();
    return;
  }
  const kept = entries.slice(0, state.depth + 1);
  const landed = kept[state.depth];
  if (!landed || landed.key !== state.key) {
    rebuildFrom(window.location.pathname);
    return;
  }
  markBack();
  entries = kept;
  show(parseLenient(landed.path).address);
}

function rebuildFrom(path: string): void {
  go(parseLenient(path).address, { rebuild: true });
}

function write(depth: number, entry: Entry, replace: boolean): void {
  const state = stateOf({ session: SESSION, depth, key: entry.key });
  if (replace) window.history.replaceState(state, "", entry.path);
  else window.history.pushState(state, "", entry.path);
}

export interface GoOptions {
  /** Lay the whole chain down again from the first entry (start, or a broken history). */
  rebuild?: boolean;
  /** Called with the new page's history entry, to hand it what an address can't carry. */
  handoff?: (entryKey: string) => void;
}

/** Navigations run one after another: a traversal lands before the next looks at where we are. */
let queue: Promise<void> = Promise.resolve();

/** Shows `to`, keeping the history its up-chain. */
export function go(to: Address, options: GoOptions = {}): void {
  queue = queue.then(() => goNow(to, options)).catch((cause) => console.warn("Navigation failed", cause));
}

async function goNow(to: Address, options: GoOptions): Promise<void> {
  const target = parse(format(to)) ?? parseLenient(format(to)).address;
  const wanted = chain(target).map(format);
  await clearLayers();
  if (options.rebuild || entries.length === 0) {
    entries = wanted.map((path) => ({ path, key: newKey() }));
    entries.forEach((entry, depth) => write(depth, entry, depth === 0));
  } else {
    const plan = planHistory(
      entries.map((e) => e.path),
      wanted,
    );
    if (plan.isReturn) markBack();
    await traverse(plan.back);
    entries = entries.slice(0, entries.length - plan.back);
    if (plan.replace !== null) {
      const entry = { path: plan.replace, key: newKey() };
      entries[entries.length - 1] = entry;
      write(entries.length - 1, entry, true);
    }
    for (const path of plan.push) {
      const entry = { path, key: newKey() };
      entries.push(entry);
      write(entries.length - 1, entry, false);
    }
  }
  options.handoff?.(topKey());
  show(target);
}

/** Back from the page shown: to the page above it (a page's ←, Done). */
export function goUp(): void {
  const above = up(current);
  if (above) go(above);
}

/** A page's ← and a summary's Done. */
export function useUp(): () => void {
  return goUp;
}

/** Opens `seg` from the page shown. */
export function open(seg: Segment, options?: GoOptions): void {
  go(openFrom(current, seg), options);
}

/** A tab button: where the tab was last left, or its top when it is the tab shown. */
// Spec: SHELL-22
export function goTab(tab: Tab): void {
  go(tab === current.tab ? tabRoot(tab) : (lastInTab.get(tab) ?? tabRoot(tab)));
}

/** Back, as a computer's back keys do it: nothing from Exercises. */
function back(): void {
  if (layers.length > 0 || entries.length > 1) window.history.back();
}

function lastShown(now: number): Address | null {
  try {
    const stored = localStorage.getItem(LAST_KEY);
    if (!stored) return null;
    const last = parseJson(LastShownSchema, stored);
    if (!last.success || now - last.data.at > RESUME_WITHIN_MS) return null;
    return forResume(parseLenient(last.data.path).address);
  } catch {
    return null;
  }
}

/**
 * Starts following the history, once the app has loaded: the address in the
 * URL is shown (a reload), or, opened fresh on Exercises within
 * `nav.resume-within` of leaving, the page that was open.
 */
// Spec: SHELL-19
export function startNavigation(): void {
  if (started) return;
  started = true;
  window.addEventListener("popstate", onPop);
  // Spec: SHELL-23
  window.addEventListener("keydown", (event) => {
    if (event.metaKey && event.key === "[") {
      event.preventDefault();
      back();
    }
  });
  window.addEventListener("mouseup", (event) => {
    if (event.button === 3) {
      event.preventDefault();
      back();
    }
  });
  const here = parseLenient(window.location.pathname).address;
  const resumed = here.tab === "exercises" && here.trail.length === 0 ? lastShown(Date.now()) : null;
  go(resumed ?? here, { rebuild: true });
}

/** Opens a path from outside the app's pages (a notification tapped). */
export function goPath(path: string): void {
  go(parseLenient(path).address);
}

// ------------------------------------------------------------ remembered per page

/** A value handed to the page a navigation opens (a summary's result, a message for the tutor). */
export function handoffSlot<T>(): {
  set: (entryKey: string, value: T) => void;
  get: (entryKey: string) => T | undefined;
  drop: (entryKey: string) => void;
} {
  const values = new Map<string, T>();
  return {
    set: (entryKey, value) => values.set(entryKey, value),
    get: (entryKey) => values.get(entryKey),
    drop: (entryKey) => values.delete(entryKey),
  };
}

const scrolls = new Map<string, number>();

/** Where a page was scrolled to, kept by its history entry, for coming back to it. */
export function rememberScroll(key: string, top: number): void {
  scrolls.set(key, top);
}

export function recallScroll(key: string): number | undefined {
  return scrolls.get(key);
}
