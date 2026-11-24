import type { Achievement } from "./schemas";

/** "4 of 7 days": how far along, in what it counts (every unit takes an s). */
export function achievementProgress(achievement: Achievement): string {
  const { current, target, unit } = achievement;
  return `${current} of ${target} ${target === 1 ? unit : `${unit}s`}`;
}

/** What is still to do, in a short line: "3 days to go". */
export function achievementToGo(achievement: Achievement): string {
  const left = Math.max(0, achievement.target - achievement.current);
  return `${left} ${left === 1 ? achievement.unit : `${achievement.unit}s`} to go`;
}

/** A day number (days since the epoch, local) as a full date: "Tuesday 3 March 2026". */
export function unlockedOn(day: number): string {
  return new Date(day * 86_400_000).toLocaleDateString(undefined, {
    timeZone: "UTC",
    weekday: "long",
    day: "numeric",
    month: "long",
    year: "numeric",
  });
}

/** The same date on a tile: "3 Mar" this year, "Mar 2025" before. */
export function unlockedOnShort(day: number, today: number): string {
  const date = new Date(day * 86_400_000);
  const sameYear = date.getUTCFullYear() === new Date(today * 86_400_000).getUTCFullYear();
  return date.toLocaleDateString(
    undefined,
    sameYear
      ? { timeZone: "UTC", day: "numeric", month: "short" }
      : { timeZone: "UTC", month: "short", year: "numeric" },
  );
}
