import { useSyncExternalStore } from "react";

import { dayToDate } from "./lessons";
import type { BadgeTone } from "../design";
import type { CourseId, Filter, Strength } from "./types";

/*
 * What the Progress → "What you've met" and "Insights" pages share: the
 * period and course they look at (one choice for every page, kept while the
 * learner moves between them), and how numbers and dates read.
 */

export type Period = "week" | "month" | "quarter" | "year" | "all";

export const PERIODS: { value: Period; label: string; days: number | null }[] =
  [
    { value: "week", label: "7 days", days: 7 },
    { value: "month", label: "30 days", days: 30 },
    { value: "quarter", label: "3 months", days: 91 },
    { value: "year", label: "A year", days: 365 },
    { value: "all", label: "All time", days: null },
  ];

interface Shared {
  period: Period;
  courseId: CourseId | null;
}

let shared: Shared = { period: "month", courseId: null };
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function setInsightFilter(next: Partial<Shared>): void {
  shared = { ...shared, ...next };
  for (const listener of listeners) listener();
}

/** The period and course every insights page is showing. */
export function useInsightFilter(): Shared {
  return useSyncExternalStore(subscribe, () => shared);
}

/** A period ending today as the filter the commands take. */
export function filterFor(
  period: Period,
  courseId: CourseId | null,
  today: number,
): Filter {
  const days = PERIODS.find((p) => p.value === period)?.days ?? null;
  return {
    fromDay: days === null ? null : today - days + 1,
    toDay: null,
    courseId,
  };
}

export const ALL_TIME: Filter = { fromDay: null, toDay: null, courseId: null };

export function pct(x: number): string {
  return `${Math.round(x * 100)}%`;
}

/** "Mon 3 Nov", or "Today" / "Yesterday". */
export function dayLabel(day: number, today: number): string {
  if (day === today) return "Today";
  if (day === today - 1) return "Yesterday";
  return dayToDate(day);
}

/** When an answer was given: its day and time. */
export function whenLabel(at: number, today: number): string {
  const day = Math.floor(
    (at - new Date().getTimezoneOffset() * 60_000) / 86_400_000,
  );
  const time = new Date(at).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
  return `${dayLabel(day, today)}, ${time}`;
}

export const STRENGTHS: { value: Strength; label: string; tone: BadgeTone }[] =
  [
    { value: "tricky", label: "Tricky", tone: "crimson" },
    { value: "learning", label: "Learning", tone: "gold" },
    { value: "solid", label: "Solid", tone: "mint" },
    { value: "new", label: "New", tone: "neutral" },
  ];

export function strengthOf(value: Strength) {
  return STRENGTHS.find((s) => s.value === value) ?? STRENGTHS[3];
}

/** Parts of speech as the words list filters them. */
export const POS_LABELS: Record<string, string> = {
  "n-m": "Noun",
  "n-f": "Noun",
  "n-n": "Noun",
  "n-pl": "Noun",
  v: "Verb",
  adj: "Adjective",
  adv: "Adverb",
  num: "Number",
  pron: "Pronoun",
  prep: "Preposition",
  conj: "Conjunction",
  part: "Particle",
  interj: "Interjection",
  phr: "Phrase",
};

export function posGroup(pos: string): string {
  return POS_LABELS[pos] ?? "Other";
}

/** What "Your learning report" asks the tutor: the learner's own request. */
export const REPORT_ASK =
  "Could you look at my answer history and give me a learning report: what I find hard (words, grammar, sounds, kinds of mistakes, with examples of what I actually got wrong), what is improving or slipping, and what I'm strong at?";

/** What the Structures page's button asks the tutor (PRINCIPLES #4: a button, no backfill). */
export const TAG_ASK =
  "Tag the grammar in my courses: list the words and sentences without grammar tags (list_untagged) and give each its tags with tag_grammar, a course at a time, until none are left. Change nothing else.";

/** What the Structures page's second button asks (PRINCIPLES #4: a button, no backfill). */
export const ANNOTATE_ASK =
  "Annotate the sentences in my courses word by word: list the sentences without tokens (list_untagged with missing: tokens) and annotate each with annotate_sentences — dictionary form, part of speech, case, number, gender, person, tense, aspect, mood and the reason for each form — a course at a time, until none are left. Change nothing else.";

/** What the Topics page's button asks. */
export const TOPICS_ASK =
  "Tag the topics in my courses: list the lessons, words and sentences without topics (list_untagged with missing: topics) and give each its topics with tag_topics, a course at a time, until none are left. Change nothing else.";

/** Every kind of mistake (mistakes.rs CLASSES), for the raw log's filter. */
export const MISTAKE_CLASSES: { value: string; label: string }[] = [
  { value: "diacritics", label: "Polish letters" },
  { value: "ending", label: "Endings" },
  { value: "word_order", label: "Word order" },
  { value: "sound_contrast", label: "Sounds" },
  { value: "preposition", label: "Prepositions" },
  { value: "typo", label: "Typos" },
  { value: "lookalike", label: "Look-alike words" },
  { value: "partial", label: "Some words wrong" },
  { value: "unknown_word", label: "Didn't know it" },
];

/** How practice was reached (learnlog.rs SOURCES), as the pages say it. */
export const SOURCES: { value: string; label: string }[] = [
  { value: "course", label: "From its course" },
  { value: "daily", label: "Today's lessons" },
  { value: "other", label: "Other lessons" },
  { value: "game", label: "Quick games" },
  { value: "tutor", label: "From the tutor" },
  { value: "music", label: "From Music" },
  { value: "link", label: "From a link" },
  { value: "none", label: "Not recorded" },
];

/** The exercise kinds the raw log filters by (analytics.rs exercise_label). */
export const EXERCISES: { value: string; label: string }[] = [
  { value: "recognise", label: "Pick the meaning" },
  { value: "produce", label: "Pick the Polish" },
  { value: "type", label: "Write it" },
  { value: "wordOrder", label: "Build the sentence" },
  { value: "dialogueGap", label: "What comes next" },
  { value: "gapFill", label: "Fill the gaps" },
  { value: "picturePick", label: "Pictures" },
  { value: "dictation", label: "Dictation" },
  { value: "soundPair", label: "Sound pair" },
  { value: "sceneWhere", label: "Where is it?" },
  { value: "storyQuestion", label: "Story question" },
  { value: "sayIt", label: "Say it" },
  { value: "dialogueSay", label: "Say your line" },
  { value: "checkin", label: "Check-in" },
  { value: "checkinMistake", label: "Check-in slip" },
  { value: "game:dictation", label: "Dictation game" },
  { value: "game:sounds", label: "Sound pairs game" },
  { value: "game:say", label: "Say it game" },
];

/** The ways a word's answers group on its page (contexts.rs GROUPS). */
export const CONTEXT_GROUPS: { value: string; label: string }[] = [
  { value: "role", label: "Alone or in context" },
  { value: "reason", label: "Why that form" },
  { value: "case", label: "Case" },
  { value: "form", label: "Form" },
  { value: "course", label: "Course" },
  { value: "topic", label: "Topic" },
  { value: "exercise", label: "Exercise" },
  { value: "source", label: "Reached from" },
];

/** A word-by-word result as the pages show it. */
export const WORD_STATUS: Record<string, string> = {
  r: "right",
  a: "Polish letters",
  w: "wrong",
  e: "wrong ending",
  m: "missing",
  x: "extra",
  o: "out of order",
};
