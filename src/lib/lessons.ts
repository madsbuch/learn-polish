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
