import { api, type AnswerSignals, type LearningEventIn } from "./api";
import { describe, logError } from "./log";
import type { PracticeContext } from "./schemas";

/*
 * The learning log, from the screens' side (learnlog.rs keeps it).
 *
 * Events (a lesson opened or left early, a word's meaning tapped, "Talk
 * about it") are queued and sent in batches, one command and one short
 * transaction a batch, a few seconds apart and whenever the app goes to the
 * background — never one write per tap.
 *
 * Signals are how the card on screen is being answered: when it showed, the
 * first touch, picks changed, audio played (slow too), translations shown.
 * The practice screen starts a card, the pieces that see these things note
 * them here, and the answer takes them with it to `submit_answer`, where
 * they are written in the grade's own transaction.
 */

const queue: LearningEventIn[] = [];

/*
 * Where the learner is practising: the course page a lesson was opened
 * from, the daily five, a quick game… The screen that opens practice sets
 * it (a lesson's page, a drill, a game) and every event is logged with it,
 * so the log can say in which course and from where each thing happened.
 */
let where: PracticeContext | null = null;

export function setPracticeContext(context: PracticeContext | null): void {
  where = context;
}

export function practiceContext(): PracticeContext | null {
  return where;
}
let timer: number | undefined;
/** Sent this long after the first event of a batch. */
const BATCH_MS = 4000;
/** Sent at once from this many. */
const BATCH_MAX = 60;

export function logEvent(event: LearningEventIn): void {
  queue.push({
    ...event,
    at: event.at ?? Date.now(),
    courseId: event.courseId ?? where?.courseId ?? null,
    source: event.source ?? where?.source ?? "",
  });
  if (queue.length >= BATCH_MAX) {
    flushLog();
  } else if (timer === undefined) {
    timer = window.setTimeout(flushLog, BATCH_MS);
  }
}

/** Sends what is queued (the practice screen calls it as a drill ends). */
export function flushLog(): void {
  if (timer !== undefined) window.clearTimeout(timer);
  timer = undefined;
  if (queue.length === 0) return;
  const batch = queue.splice(0, 200);
  // Losing a few events is better than a stuck queue: a failed batch is
  // logged for Diagnostics and dropped.
  api
    .learningLog(batch)
    .catch((cause: unknown) => logError("learning log", describe(cause)));
  if (queue.length > 0) timer = window.setTimeout(flushLog, BATCH_MS);
}

if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") flushLog();
  });
}

// ------------------------------------------------------------ the card on screen

interface Card {
  key: string;
  shownAt: number;
  firstAt: number | null;
  tries: number;
  hints: number;
  plays: number;
  slowPlays: number;
  reveals: number;
  firstPick: string | null;
  lastPick: string | null;
}

let card: Card | null = null;

/** A card is on screen: its clock starts. */
export function beginCard(key: string): void {
  card = {
    key,
    shownAt: performance.now(),
    firstAt: null,
    tries: 0,
    hints: 0,
    plays: 0,
    slowPlays: 0,
    reveals: 0,
    firstPick: null,
    lastPick: null,
  };
}

/** No card on screen (the drill ended): audio from here on is an event. */
export function endCard(): void {
  card = null;
}

/** The learner put something in: a pick (its text), a key, a tile. */
export function noteInput(pick?: string): void {
  if (!card) return;
  card.firstAt ??= performance.now();
  if (pick === undefined) return;
  if (card.firstPick === null) card.firstPick = pick;
  else if (pick !== card.lastPick) card.tries += 1;
  card.lastPick = pick;
}

/** A typed answer changed after it was first started (retyped). */
export function noteRetype(): void {
  if (!card) return;
  card.firstAt ??= performance.now();
}

/** Polish was played: counted on the card, or an event of its own. */
export function noteAudio(text: string, slow: boolean): void {
  if (card) {
    card.plays += 1;
    if (slow) card.slowPlays += 1;
    return;
  }
  logEvent({
    kind: "audio_played",
    details: { word: text.slice(0, 80), slow },
  });
}

/** The English (or a translation) was shown. */
export function noteReveal(): void {
  if (card) card.reveals += 1;
}

export function noteHint(): void {
  if (card) card.hints += 1;
}

/** How the card was answered, for its answer. */
export function takeSignals(): AnswerSignals {
  if (!card) return {};
  const now = performance.now();
  const changed = card.firstPick !== null && card.lastPick !== card.firstPick;
  return {
    msFirst:
      card.firstAt === null
        ? undefined
        : Math.round(card.firstAt - card.shownAt),
    msAnswer: Math.round(now - card.shownAt),
    tries: card.tries,
    hints: card.hints,
    plays: card.plays,
    slowPlays: card.slowPlays,
    reveals: card.reveals,
    changed,
    firstPick: changed ? (card.firstPick ?? undefined) : undefined,
  };
}
