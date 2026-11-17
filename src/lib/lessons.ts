import type { Lesson, Complexity } from "./types";

/** How long a lesson takes, by complexity. */
export function minutes(complexity: Complexity): string {
  return complexity === "low" ? "1–2 min" : complexity === "medium" ? "5–10 min" : "20+ min";
}

export function complexityName(complexity: Complexity): string {
  return complexity === "low" ? "Quick" : complexity === "medium" ? "Medium" : "Deep";
}

export function complexityTone(complexity: Complexity): "mint" | "sky" | "crimson" {
  return complexity === "low" ? "mint" : complexity === "medium" ? "sky" : "crimson";
}

const FORMAT_NAMES: Record<string, string> = {
  words: "Words",
  memory: "Memory cards",
  typing: "Typing",
  sentences: "Sentences",
  gaps: "Fill the gaps",
  dialogue: "Conversation",
  song: "Listening",
  writing: "Writing",
};

/** "Study" or the drill's format, e.g. "Memory cards". */
export function kindName(lesson: Lesson): string {
  if (lesson.format === "song") return "Listening";
  if (lesson.format === "writing") return "Writing";
  return lesson.kind === "study" ? "Study" : (FORMAT_NAMES[lesson.format] ?? "Drill");
}

/** When the lesson is next due, in words: "due now", "in 3 days", "new". */
export function dueLabel(lesson: Lesson, today: number): string {
  if (lesson.dueDay === null) return "new";
  const days = lesson.dueDay - today;
  if (days <= 0) return "due now";
  if (days === 1) return "tomorrow";
  if (days < 14) return `in ${days} days`;
  if (days < 60) return `in ${Math.round(days / 7)} weeks`;
  return `in ${Math.round(days / 30)} months`;
}

/** The mastery points at which a lesson is mastered (lessons.rs). */
export const MASTERY_FULL = 100;

/**
 * Whether a lesson is mastered: passed, with full mastery points, and — for
 * writing — done without help. Mastery is earned by passing, never by
 * waiting (see `mastery_gain` in lessons.rs).
 */
export function mastered(lesson: Lesson): boolean {
  return (
    lesson.passed && lesson.mastery >= MASTERY_FULL && (lesson.format !== "writing" || lesson.supportLevel >= 2)
  );
}

/** Where a lesson stands in its course: still to pass, being mastered, or mastered. */
export type Standing = "toPass" | "learning" | "mastered";

export function standing(lesson: Lesson): Standing {
  return !lesson.passed ? "toPass" : mastered(lesson) ? "mastered" : "learning";
}

/** What a writing lesson still needs before it can be mastered, or null. */
export function writingStep(lesson: Lesson): string | null {
  if (lesson.format !== "writing" || lesson.supportLevel >= 2) return null;
  return lesson.supportLevel === 0 ? "write it without the word bank" : "write it without hints";
}

/**
 * About how many more passes a lesson needs to be mastered: practising now
 * (one sitting, 10 points a pass after the first) or spaced out by the
 * check-ins (about 35). Mirrors `mastery_gain` in lessons.rs.
 */
export function passesLeft(lesson: Lesson): { now: number; spaced: number } {
  const first = lesson.passed ? 0 : 1;
  const left = Math.max(0, MASTERY_FULL - Math.max(lesson.mastery, 0) - (first ? 30 : 0));
  return { now: first + Math.ceil(left / 10), spaced: first + Math.ceil(left / 35) };
}

/** A lesson's standing in words, for a course's list. */
export function standingText(lesson: Lesson, today: number): string {
  if (!lesson.passed) return lesson.reps === 0 ? "Not tried yet" : "Needs another go";
  const due = dueLabel(lesson, today);
  const back = due === "due now" ? "due now" : `back ${due}`;
  if (mastered(lesson)) return `Mastered · ${back}`;
  if (lesson.mastery < MASTERY_FULL) {
    const { now, spaced } = passesLeft(lesson);
    const passes = now === spaced ? `${now} more ${now === 1 ? "pass" : "passes"}` : `${spaced}–${now} more passes`;
    return `Mastery ${lesson.mastery} of ${MASTERY_FULL} · ${passes} · ${back}`;
  }
  return `Next: ${writingStep(lesson) ?? "pass it once more"} · ${back}`;
}

/** Day number (days since the epoch, local) to a short date. */
export function dayToDate(day: number): string {
  return new Date(day * 86_400_000).toLocaleDateString(undefined, {
    weekday: "short",
    day: "numeric",
    month: "short",
  });
}

export const PERIOD_CHOICES: { value: string; label: string }[] = [
  { value: "1", label: "Daily" },
  { value: "3", label: "3 days" },
  { value: "7", label: "Weekly" },
  { value: "14", label: "2 weeks" },
  { value: "30", label: "Monthly" },
];
