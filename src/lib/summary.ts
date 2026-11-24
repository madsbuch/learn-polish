import { dayToDate, MASTERY_FULL, MASTERY_HELD_AT } from "./lessons";
import type { Impact, MasteryEntry } from "./schemas";

/*
 * What the summary after a drill or a conversation says, worked out from
 * what the go handed it: the grade's word, its tone, the mastery line, when
 * the lesson comes back, and what the misses cost. Pure, so it is tested
 * apart from the screen (tests/summary.test.ts). The wording is the spec's
 * (`.spec/lessons/summary.json`, `summary.*`).
 */

export type Grade = "ace" | "pass" | "partial" | "fail";

/** How the summary feels: as much celebration as the grade earned. */
export type Tone = "mastered" | "ace" | "pass" | "partial" | "calm" | "plain";

/** Each grade's word on screen (spec `term.ace`, `term.pass`, `term.partial`, `term.fail`). */
export const GRADE_WORD: Record<Grade, string> = {
  ace: "Aced",
  pass: "Passed",
  partial: "Partly there",
  fail: "Not yet",
};

/** Spec `summary.*`. */
export const SUMMARY_TEXT = {
  notGraded: "Nothing was scored, so it isn't graded: its mastery and schedule stay as they were.",
  capped: `Held at ${MASTERY_HELD_AT} until a pass on another day`,
  afterMastered: "Mastered, for good",
  backOn: (date: string) => `Back on ${date}`,
  partialNext: (when: string) => `Nearly there. It comes back ${when}, or try again now.`,
  failNext: (when: string) => `Misses are how it sticks. It comes back ${when}.`,
  extraNext: "Extra practice: its schedule stays as it was.",
  impactDrill: (n: number, verb: string, change: string | null, actual: string) =>
    change === null ? `${n} more right would have ${verb} it.` : `${n} more right would have ${verb} it: ${change} instead of ${actual}.`,
  impactConversation: (change: string, actual: string) => `A pass would have been ${change} instead of ${actual}.`,
  missedSooner: "What you missed comes back for review sooner.",
  masteredLine: "Mastered for good. It won't come due again.",
  dayDone: "Today's lessons are done",
} as const;

/**
 * The tone: a lesson mastered by this go has its own moment; a fail is met
 * calmly; a go not graded is plain. No go loses points, so the grade alone
 * decides.
 */
// Spec: SUM-5, SUM-6, SUM-9
export function toneOf(grade: Grade | "", graded: boolean, masteredNow: boolean): Tone {
  if (masteredNow) return "mastered";
  if (!graded || grade === "") return "plain";
  if (grade === "fail") return "calm";
  return grade;
}

/** Whether the tone celebrates: confetti and the cheerful sound. */
// Spec: SUM-5, SUM-6
export function celebrates(tone: Tone): boolean {
  return tone === "mastered" || tone === "ace" || tone === "pass";
}

/** A change in points: "+38", or "no change" (points only grow: MAST-6). */
export function signed(delta: number): string {
  if (delta <= 0) return "no change";
  return `+${delta}`;
}

/** The go's own change beside a better one: "+5", or "0" when it earned none. */
function earned(delta: number): string {
  return delta > 0 ? `+${delta}` : "0";
}

/** A drill's score: "11 of 11 right", "7½ of 10 right" (an almost counts half). */
// Spec: SUM-2
export function scoreText(correct: number, almost: number, total: number): string {
  const whole = correct + Math.floor(almost / 2);
  return `${whole}${almost % 2 === 1 ? "½" : ""} of ${total} right`;
}

/** Why the ledger's row is what it is, when the reason needs saying. */
// Spec: SUM-3
export function masteryReason(entry: MasteryEntry): string | null {
  switch (entry.reason) {
    case "capped":
      return SUMMARY_TEXT.capped;
    case "after_mastered":
      return SUMMARY_TEXT.afterMastered;
    default:
      return null;
  }
}

/** "tomorrow", "today" or "on Sat, 28 Nov". */
function when(day: number, today: number): string {
  if (day <= today) return "today";
  if (day === today + 1) return "tomorrow";
  return `on ${dayToDate(day)}`;
}

/** The line that says when the lesson comes back, by the grade. */
// Spec: SUM-4
export function nextLine(grade: Grade, extra: boolean, nextDueDay: number | null, today: number, masteredNow: boolean): string | null {
  if (masteredNow) return null;
  if (extra) return SUMMARY_TEXT.extraNext;
  if (nextDueDay === null) return null;
  switch (grade) {
    case "partial":
      return SUMMARY_TEXT.partialNext(when(nextDueDay, today));
    case "fail":
      return SUMMARY_TEXT.failNext(when(nextDueDay, today));
    default:
      return SUMMARY_TEXT.backOn(dayToDate(nextDueDay));
  }
}

/**
 * What the misses kept from the go, in one line, from what the core worked
 * out by the rules that graded it (`impact`): the more right answers a
 * better grade took, and the mastery change it would have made instead of
 * the go's own ("0" when it earned none — no go loses points).
 */
// Spec: SUM-8
export function impactLine(impact: Impact | null, actualDelta: number | null): string | null {
  if (!impact) return null;
  const actual = Math.max(0, actualDelta ?? 0);
  const change = impact.delta === actual ? null : signed(impact.delta);
  if (impact.moreRight === null) {
    return change === null ? null : SUMMARY_TEXT.impactConversation(change, earned(actual));
  }
  return SUMMARY_TEXT.impactDrill(impact.moreRight, impact.grade === "ace" ? "aced" : "passed", change, earned(actual));
}

/** The bar's ends, 0–`mastery.full`. */
export function barEnds(entry: MasteryEntry): { from: number; to: number } {
  const clamp = (n: number) => Math.max(0, Math.min(MASTERY_FULL, n));
  return { from: clamp(entry.before), to: clamp(entry.after) };
}
