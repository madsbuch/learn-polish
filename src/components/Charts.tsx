import type { ReactNode } from "react";

import { Inline, Progress, Stack, Text } from "../design";

/*
 * The insights pages' charts, in the app's one chart language (Progress →
 * XP): a single series in the brand colour, thin bars rounded at the top
 * and anchored to the baseline, recessive ends, every bar carrying its
 * value as a tooltip and for screen readers. One series each, so no legend:
 * the section title names it. Bars rise once as they appear (an entrance:
 * nothing stays transformed).
 */

export interface Bar {
  key: string | number;
  /** What it covers and its value, e.g. "Mon 3 Nov: 80% of 12" (its tooltip). */
  label: string;
  value: number;
  /** The bar's height as a share of the tallest (0–1). */
  share: number;
  /** A bar with nothing behind it shows as a stub. */
  empty?: boolean;
  /** A lower-key bar (e.g. few answers behind a percentage). */
  faint?: boolean;
}

/** Vertical bars with their two ends labelled. */
export function BarChart({
  bars,
  top,
  start,
  end,
  label,
  small,
  onOpen,
}: {
  bars: Bar[];
  /** The top of the scale, e.g. "100%". */
  top?: string;
  start?: string;
  end?: string;
  /** For screen readers: what the chart shows. */
  label: string;
  small?: boolean;
  onOpen?: (bar: Bar) => void;
}) {
  return (
    <div className="mini-chart" data-small={small || undefined}>
      {top ? (
        <Text variant="caption" tone="faint" as="div">
          {top}
        </Text>
      ) : null}
      <div className="mini-bars" role="img" aria-label={label}>
        {bars.map((bar, i) => {
          const body = (
            <i
              style={{
                "--share": Math.max(0, Math.min(1, bar.share)),
                "--i": i,
              }}
              data-empty={bar.empty || undefined}
              data-faint={bar.faint || undefined}
            />
          );
          return onOpen ? (
            <button
              key={bar.key}
              className="mini-bar"
              title={bar.label}
              aria-label={bar.label}
              onClick={() => onOpen(bar)}
            >
              {body}
            </button>
          ) : (
            <span
              key={bar.key}
              className="mini-bar"
              title={bar.label}
            >
              {body}
            </span>
          );
        })}
      </div>
      {start || end ? (
        <div className="mini-chart-ends">
          <Text variant="caption" tone="faint">
            {start}
          </Text>
          <Text variant="caption" tone="faint">
            {end}
          </Text>
        </div>
      ) : null}
    </div>
  );
}

/** A labelled horizontal measure: a name, its count, and a bar. */
export function Measure({
  label,
  value,
  max,
  shown,
  memory,
  tone,
  onClick,
}: {
  label: ReactNode;
  value: number;
  max: number;
  /** What is written at the right (defaults to the value). */
  shown?: ReactNode;
  memory?: string;
  tone?: "gold" | "mint";
  onClick?: () => void;
}) {
  const body = (
    <Stack gap={1}>
      <Inline justify="between" gap={2}>
        <Text variant="small">{label}</Text>
        <Text variant="small" tone="faint">
          {shown ?? value}
        </Text>
      </Inline>
      <Progress value={value} max={max} size="sm" tone={tone} memory={memory} />
    </Stack>
  );
  return onClick ? (
    <button className="measure-row" onClick={onClick}>
      {body}
    </button>
  ) : (
    <div className="measure-row">{body}</div>
  );
}
