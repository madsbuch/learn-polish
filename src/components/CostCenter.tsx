import { useEffect, useState } from "react";

import {
  Badge,
  Banner,
  List,
  ListEmpty,
  ListItem,
  Progress,
  Section,
  Segmented,
  Spinner,
  Stack,
  Stat,
  StatGrid,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { dayToDate } from "../lib/blocks";
import type { CostSummary, FeatureCost } from "../lib/types";

const FEATURES: Record<string, string> = {
  tutor: "Tutor",
  examiner: "Check-ins",
  songwriter: "Song lyrics",
  songs: "Song audio",
  explain: "Explanations",
  speech: "Speech",
  "key test": "Key test",
};

const WINDOWS: { value: string; label: string }[] = [
  { value: "7", label: "7 days" },
  { value: "30", label: "30 days" },
  { value: "90", label: "90 days" },
];

function featureName(feature: string): string {
  return FEATURES[feature] ?? feature.charAt(0).toUpperCase() + feature.slice(1);
}

/** Dollars, with enough precision that a cent-sized day is not "$0.00". */
function usd(amount: number): string {
  if (amount === 0) return "$0";
  if (amount < 0.001) return "<$0.001";
  if (amount < 1) return `$${amount.toFixed(3)}`;
  return `$${amount.toFixed(2)}`;
}

function unpriced(cost: FeatureCost): boolean {
  return cost.calls > 0 && cost.calls === cost.unpriced;
}

function uses(cost: FeatureCost): string {
  const noun = cost.feature === "speech" ? "clip" : cost.feature === "songs" ? "song" : "request";
  const count = `${cost.calls} ${noun}${cost.calls === 1 ? "" : "s"}`;
  if (cost.unpriced === 0) return count;
  return unpriced(cost) ? `${count} · price not reported` : `${count} · ${cost.unpriced} unpriced`;
}

/**
 * What the AI features cost: today, over a window, per feature, and day by
 * day with each day's features. Prices are what the providers report.
 */
export function CostCenter() {
  const [range, setRange] = useState("30");
  const [summary, setSummary] = useState<CostSummary | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .costsSummary(Number(range))
      .then((next) => {
        if (live) setSummary(next);
      })
      .catch((cause) => {
        if (live) setError(errorMessage(cause));
      });
    return () => {
      live = false;
    };
  }, [range]);

  const top = summary ? Math.max(0, ...summary.features.map((f) => f.costUsd)) : 0;

  return (
    <>
      <Section
        title="AI costs"
        info={
          <>
            <p>
              Every request to an AI service is logged with the feature that made it. Chat requests
              — the tutor, check-ins, song lyrics and explanations — are priced by OpenRouter
              itself.
            </p>
            <p>
              Song audio is priced from the kie.ai credits Suno used, at about $0.005 a credit.
              OpenRouter does not report a price for speech, so speech shows how many clips were
              made. Replaying downloaded audio is free.
            </p>
            <p>Costs survive a progress reset and are part of the database export.</p>
          </>
        }
      >
        {error ? <Banner tone="error">{error}</Banner> : null}
        {!summary ? (
          error ? null : <Spinner />
        ) : (
          <Stack gap={3}>
            <StatGrid>
              <Stat value={usd(summary.todayUsd)} label="today" />
              <Stat value={usd(summary.totalUsd)} label={`${summary.days} days`} />
              <Stat value={usd(summary.allTimeUsd)} label="all time" />
            </StatGrid>
            <Segmented value={range} options={WINDOWS} onChange={setRange} />
            <List>
              {summary.features.length === 0 ? (
                <ListEmpty>No AI use in the last {summary.days} days.</ListEmpty>
              ) : (
                summary.features.map((cost) => (
                  <ListItem
                    key={cost.feature}
                    title={featureName(cost.feature)}
                    subtitle={
                      <Stack gap={1}>
                        {uses(cost)}
                        {top > 0 && cost.costUsd > 0 ? (
                          <Progress value={cost.costUsd} max={top} size="sm" />
                        ) : null}
                      </Stack>
                    }
                    trailing={
                      <Badge tone={cost.costUsd > 0 ? "brand" : "neutral"}>
                        {unpriced(cost) ? "—" : usd(cost.costUsd)}
                      </Badge>
                    }
                  />
                ))
              )}
            </List>
          </Stack>
        )}
      </Section>

      {summary && summary.perDay.length > 0 ? (
        <Section title="AI costs per day">
          <List>
            {summary.perDay.map((day) => (
              <ListItem
                key={day.day}
                title={day.day === summary.today ? "Today" : dayToDate(day.day)}
                subtitle={day.features
                  .map((cost) =>
                    unpriced(cost)
                      ? `${featureName(cost.feature)} ×${cost.calls}`
                      : `${featureName(cost.feature)} ${usd(cost.costUsd)}`,
                  )
                  .join(" · ")}
                trailing={<Badge>{usd(day.costUsd)}</Badge>}
              />
            ))}
          </List>
        </Section>
      ) : null}
    </>
  );
}
