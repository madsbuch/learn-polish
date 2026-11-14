import type { Block, Complexity } from "./types";

/** How long a block takes, by complexity. */
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
export function kindName(block: Block): string {
  if (block.format === "song") return "Listening";
  if (block.format === "writing") return "Writing";
  return block.kind === "study" ? "Study" : (FORMAT_NAMES[block.format] ?? "Drill");
}

/** When the block is next due, in words: "due now", "in 3 days", "new". */
export function dueLabel(block: Block, today: number): string {
  if (block.dueDay === null) return "new";
  const days = block.dueDay - today;
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
