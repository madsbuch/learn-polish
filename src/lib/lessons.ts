import { lessonType } from "./lessonTypes";
import type { Lesson, Complexity, MasteryEntry } from "./types";

/** About how many minutes a lesson takes: its type's, by complexity (lesson_types::minutes). */
export function lessonMinutes(lesson: Pick<Lesson, "type" | "engine" | "complexity">): number {
  const index = lesson.complexity === "low" ? 0 : lesson.complexity === "medium" ? 1 : 2;
  return lessonType(lesson).minutes[index];
}

/** How long a lesson takes, for a badge. */
export function minutes(lesson: Pick<Lesson, "type" | "engine" | "complexity">): string {
  return `~${lessonMinutes(lesson)} min`;
}

export function complexityName(complexity: Complexity): string {
  return complexity === "low" ? "Quick" : complexity === "medium" ? "Medium" : "Deep";
}

export function complexityTone(complexity: Complexity): "mint" | "sky" | "crimson" {
  return complexity === "low" ? "mint" : complexity === "medium" ? "sky" : "crimson";
}

/** A lesson's type, by name, e.g. "Memory cards" (from the registry). */
export function kindName(lesson: Lesson): string {
  return lessonType(lesson).name;
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
/** The mark on a challenge (spec `lesson.challenge-label`). */
export const CHALLENGE_LABEL = "Challenge";
/** What a challenge is (spec `lesson.challenge-explained`). */
export const CHALLENGE_EXPLAINED =
  "A challenge is beyond its course's level: not needed to complete the course, only to master it.";
/** Where the points stop until a lesson is passed on two different days (lessons.rs `MASTERY_HELD_AT`). */
export const MASTERY_HELD_AT = 90;

/**
 * Whether a lesson is mastered: passed, with full mastery points, and — for
 * a type that asks it (writing) — done once without help. Mastery is earned by passing, never
 * by waiting, and once earned it is for good (lessons.rs `stamp_mastered`).
 */
export function mastered(lesson: Lesson): boolean {
  return lesson.mastered;
}

/** Where a lesson stands in its course: still to pass, being mastered, or mastered. */
export type Standing = "toPass" | "learning" | "mastered";

export function standing(lesson: Lesson): Standing {
  return !lesson.passed ? "toPass" : mastered(lesson) ? "mastered" : "learning";
}

/** What a lesson done with help (writing) still needs before it can be mastered, or null. */
export function writingStep(lesson: Lesson): string | null {
  if (!lessonType(lesson).unaidedToMaster || lesson.supportLevel >= 3) return null;
  return lesson.supportLevel === 0
    ? "write it without the word bank"
    : lesson.supportLevel === 1
      ? "write it without hints"
      : "pass it on your own";
}

/**
 * About how many more passes a lesson needs to be mastered, in words:
 * "3 more passes", "3–8 more passes" (spaced by its check-ins, or practising
 * now). Worked out in Rust from the lesson's own schedule (lessons.rs
 * `passes_left`).
 */
export function passesText(lesson: Lesson): string {
  const { now, spaced } = lesson.passesLeft;
  const low = Math.min(now, spaced);
  const high = Math.max(now, spaced);
  return low === high ? `${low} more ${low === 1 ? "pass" : "passes"}` : `${low}–${high} more passes`;
}

/** A lesson's standing in words, for a course's list. */
export function standingText(lesson: Lesson, today: number): string {
  if (!lesson.passed) return lesson.reps === 0 ? "Not tried yet" : "Needs another go";
  // Mastered is for good: it never comes back, so it has no next date.
  if (mastered(lesson)) return "Mastered";
  const due = dueLabel(lesson, today);
  const back = due === "due now" ? "due now" : `back ${due}`;
  if (lesson.mastery < MASTERY_FULL) {
    return `Mastery ${lesson.mastery} of ${MASTERY_FULL} · ${passesText(lesson)} · ${back}`;
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

/** Minutes as a rough duration: "about 40 min", "about 2 h". */
export function roughDuration(minutes: number): string {
  if (minutes < 60) return `about ${Math.max(5, Math.round(minutes / 5) * 5)} min`;
  const hours = minutes / 60;
  return `about ${hours < 3 ? Math.round(hours * 2) / 2 : Math.round(hours)} h`;
}

/** A mastery ledger row's change, short: "+19 (30 → 49)", "no change", with why when it was held back. */
export function masteryChange(entry: MasteryEntry): string {
  const change =
    entry.delta === 0
      ? `no change (${entry.after} of ${MASTERY_FULL})`
      : `${entry.delta > 0 ? "+" : "−"}${Math.abs(entry.delta)} (${entry.before} → ${entry.after})`;
  if (entry.reason === "capped") return `${change} — held at ${MASTERY_HELD_AT} until a pass on another day`;
  if (entry.reason === "fail_extra") return `${change} — extra practice never costs any`;
  if (entry.reason === "after_mastered") return "mastered, for good";
  return change;
}
