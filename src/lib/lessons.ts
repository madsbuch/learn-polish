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

/** The gap, in days, at which a lesson holds for mastering (courses.rs). */
export const MASTERED_GAP = 30;

/**
 * Whether a lesson holds the way mastering its course needs: passed, at a gap
 * of a month or more, and — for writing — done without help.
 */
export function holds(lesson: Lesson): boolean {
  return (
    lesson.passed && lesson.intervalDays >= MASTERED_GAP && (lesson.format !== "writing" || lesson.supportLevel >= 2)
  );
}

/** Where a lesson stands in its course: still to pass, passed, or holding. */
export type Standing = "toPass" | "passed" | "holding";

export function standing(lesson: Lesson): Standing {
  return !lesson.passed ? "toPass" : holds(lesson) ? "holding" : "passed";
}

/** What a writing lesson still needs before it can hold, or null. */
export function writingStep(lesson: Lesson): string | null {
  if (lesson.format !== "writing" || lesson.supportLevel >= 2) return null;
  return lesson.supportLevel === 0 ? "write it without the word bank" : "write it without hints";
}

/** A lesson's standing in words, for a course's list. */
export function standingText(lesson: Lesson, today: number): string {
  if (!lesson.passed) return lesson.reps === 0 ? "Not tried yet" : "Needs another go";
  const due = dueLabel(lesson, today);
  const back = due === "due now" ? "due now" : `back ${due}`;
  if (holds(lesson)) return `Holds · ${back}`;
  const step = writingStep(lesson);
  if (step) return `Next: ${step} · ${back}`;
  return `${Math.min(lesson.intervalDays, MASTERED_GAP)} of ${MASTERED_GAP} days apart · ${back}`;
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
