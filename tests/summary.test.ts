// What the summary after a drill or a conversation says (src/lib/summary.ts):
// pure parts, run with `bun test`.
import { describe, expect, test } from "bun:test";

import { celebrates, impactLine, masteryReason, nextLine, scoreText, toneOf } from "../src/lib/summary";
import { MasteryEntryIdSchema, type Impact, type MasteryEntry } from "../src/lib/schemas";

const impact = (grade: "pass" | "ace", moreRight: number | null, delta: number, reason = "pass_same_day"): Impact => ({
  grade,
  moreRight,
  delta,
  after: 0,
  reason,
});

const row = (reason: string, delta = 0): MasteryEntry => ({
  id: MasteryEntryIdSchema.parse(`mst_0${"0".repeat(25)}`),
  at: 0,
  day: 0,
  delta,
  before: 40,
  after: 40 + delta,
  grade: "pass",
  correct: null,
  almost: null,
  total: null,
  daysSince: null,
  extra: false,
  via: "drill",
  reason,
  text: "",
});

describe("tone", () => {
  // Spec: SUM-5, SUM-6, SUM-9
  test("celebrates as much as the grade earned, and never a fail", () => {
    expect(toneOf("ace", true, false)).toBe("ace");
    expect(toneOf("pass", true, false)).toBe("pass");
    expect(toneOf("partial", true, false)).toBe("partial");
    expect(toneOf("fail", true, false)).toBe("calm");
    expect(toneOf("pass", true, true)).toBe("mastered");
    expect(toneOf("", false, false)).toBe("plain");
    expect([celebrates("ace"), celebrates("pass"), celebrates("mastered")]).toEqual([true, true, true]);
    expect([celebrates("partial"), celebrates("calm"), celebrates("plain")]).toEqual([false, false, false]);
  });
});

describe("the grade and the mastery", () => {
  // Spec: SUM-2
  test("a score counts an almost as half", () => {
    expect(scoreText(11, 0, 11)).toBe("11 of 11 right");
    expect(scoreText(7, 1, 10)).toBe("7½ of 10 right");
    expect(scoreText(6, 2, 10)).toBe("7 of 10 right");
  });

  // Spec: SUM-3
  test("the ledger's reason is said when it held the points back", () => {
    expect(masteryReason(row("capped"))).toBe("Held at 90 until a pass on another day");
    expect(masteryReason(row("after_mastered"))).toBe("Mastered, for good");
    expect(masteryReason(row("fail"))).toBeNull();
    expect(masteryReason(row("pass_spaced", 16))).toBeNull();
  });

  // Spec: SUM-4
  test("when it comes back follows the grade", () => {
    expect(nextLine("fail", false, 101, 100, false)).toBe("Misses are how it sticks. It comes back tomorrow.");
    expect(nextLine("partial", false, 101, 100, false)).toBe("Nearly there. It comes back tomorrow, or try again now.");
    expect(nextLine("pass", false, 106, 100, false)).toMatch(/^Back on /);
    expect(nextLine("pass", true, 106, 100, false)).toBe("Extra practice: its schedule stays as it was.");
    expect(nextLine("pass", false, null, 100, true)).toBeNull();
  });
});

describe("what the misses kept from the go", () => {
  // Spec: SUM-8
  test("the more right answers a better grade took, and its mastery change", () => {
    expect(impactLine(impact("pass", 2, 30, "first_pass"), 17)).toBe("2 more right would have passed it: +30 instead of +17.");
    expect(impactLine(impact("ace", 1, 38, "first_pass"), 30)).toBe("1 more right would have aced it: +38 instead of +30.");
    // A fail earns nothing below half right, and never loses: "0", not a minus.
    expect(impactLine(impact("pass", 6, 16, "pass_spaced"), 0)).toBe("6 more right would have passed it: +16 instead of 0.");
    expect(impactLine(impact("pass", 7, 10), 0)).toBe("7 more right would have passed it: +10 instead of 0.");
    // Capped: the points would have been the same.
    expect(impactLine(impact("pass", 1, 0, "capped"), 0)).toBe("1 more right would have passed it.");
    expect(impactLine(impact("pass", null, 30, "first_pass"), 0)).toBe("A pass would have been +30 instead of 0.");
    expect(impactLine(null, 10)).toBeNull();
  });
});
