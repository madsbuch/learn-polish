import { useEffect, useMemo, useState } from "react";

import { Card, Section, Segmented, Stack, Text } from "../design";
import { api } from "../lib/api";
import { dayToDate } from "../lib/lessons";
import type { XpHistoryDay } from "../lib/types";

type Range = "days" | "weeks";

/** A bar: what it covers, its XP, and the day it opens (a week opens none). */
interface Bar {
  key: number;
  label: string;
  xp: number;
  day: number | null;
}

/** Day numbers count from Thursday 1 Jan 1970; weeks start on Monday. */
function weekOf(day: number): number {
  return Math.floor((day + 3) / 7);
}

/**
 * XP over time: one bar per day for the last 30 days, or per week for the
 * last 26. A day's bar opens what earned it. One series, so no legend: the
 * title names it, and every bar carries its value for screen readers.
 */
export function XpHistory({ today, onOpenDay }: { today: number; onOpenDay: (day: number) => void }) {
  const [history, setHistory] = useState<XpHistoryDay[] | null>(null);
  const [range, setRange] = useState<Range>("days");
  useEffect(() => {
    api
      .xpHistory(26 * 7)
      .then(setHistory)
      .catch(() => setHistory([]));
  }, []);

  const bars = useMemo<Bar[]>(() => {
    const byDay = new Map((history ?? []).map((d) => [d.day, d.xp]));
    if (range === "days") {
      return Array.from({ length: 30 }, (_, i) => {
        const day = today - 29 + i;
        return { key: day, label: dayToDate(day), xp: byDay.get(day) ?? 0, day };
      });
    }
    const last = weekOf(today);
    return Array.from({ length: 26 }, (_, i) => {
      const week = last - 25 + i;
      const monday = week * 7 - 3;
      let xp = 0;
      for (let d = monday; d < monday + 7; d++) xp += byDay.get(d) ?? 0;
      return { key: week, label: `Week of ${dayToDate(monday)}`, xp, day: null };
    });
  }, [history, range, today]);

  const days = history ?? [];
  const total = days.reduce((sum, d) => sum + d.xp, 0);
  const practised = days.filter((d) => d.xp > 0);
  const best = practised.reduce<XpHistoryDay | null>((top, d) => (top === null || d.xp > top.xp ? d : top), null);
  const average = practised.length ? Math.round(total / practised.length) : 0;
  const max = Math.max(1, ...bars.map((b) => b.xp));

  return (
    <Section
      title="XP"
      info={<p>XP earned each day (or week). Tap a day to see exactly what earned it.</p>}
    >
      <Card>
        <Stack gap={3}>
          <Segmented
            value={range}
            onChange={(value) => setRange(value as Range)}
            options={[
              { value: "days", label: "30 days" },
              { value: "weeks", label: "6 months, by week" },
            ]}
          />
          <Text variant="small" tone="dim" as="div">
            {history === null
              ? "Loading…"
              : `${total} XP in the last 6 months · best day ${best ? `${best.xp} (${dayToDate(best.day)})` : "—"} · ${average} on an average day you practised`}
          </Text>
          <div className="xp-chart">
            <Text variant="caption" tone="faint" as="div">{`${max} XP`}</Text>
            <div className="xp-bars" data-range={range}>
              {bars.map((bar) => (
                <button
                  key={bar.key}
                  className="xp-bar"
                  disabled={bar.day === null}
                  title={`${bar.label}: ${bar.xp} XP`}
                  aria-label={`${bar.label}: ${bar.xp} XP`}
                  onClick={() => bar.day !== null && onOpenDay(bar.day)}
                >
                  <i style={{ height: `${(bar.xp / max) * 100}%` }} data-empty={bar.xp === 0 || undefined} />
                </button>
              ))}
            </div>
            <div className="xp-chart-ends">
              <Text variant="caption" tone="faint">
                {bars[0]?.label}
              </Text>
              <Text variant="caption" tone="faint">
                {range === "days" ? "Today" : "This week"}
              </Text>
            </div>
          </div>
        </Stack>
      </Card>
    </Section>
  );
}
