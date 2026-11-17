import { useEffect, useState } from "react";

import { Badge, Banner, Card, List, ListItem, Screen, Section, Spinner, Stack, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { dayToDate } from "../lib/lessons";
import type { XpDay, XpRun } from "../lib/types";

function time(at: number): string {
  return new Date(at * 1000).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/** One run, folded: its lesson and total; open, every answer and what it earned. */
function RunCard({ run }: { run: XpRun }) {
  const [open, setOpen] = useState(false);
  return (
    <List>
      <ListItem
        title={run.lessonTitle}
        subtitle={`${time(run.startedAt)}${run.courseTitle ? ` · ${run.courseTitle}` : ""} · ${run.entries.length} ${
          run.entries.length === 1 ? "entry" : "entries"
        }`}
        trailing={<Badge tone="gold">{`+${run.xp} XP`}</Badge>}
        chevron
        onClick={() => setOpen((shown) => !shown)}
      />
      {open
        ? run.entries.map((entry, i) => (
            <ListItem
              key={`${entry.at}-${i}`}
              selectable
              title={entry.label}
              subtitle={entry.note || undefined}
              trailing={
                <Text variant="small" weight="bold" tone={entry.xp > 0 ? undefined : "faint"}>
                  {entry.xp > 0 ? `+${entry.xp}` : "0"}
                </Text>
              }
            />
          ))
        : null}
    </List>
  );
}

/**
 * A day's XP: the total, and what earned it — each drill and conversation,
 * which opens to every answer and what it earned (or why it earned nothing).
 */
export function XpDayScreen({ day, onBack }: { day: number; onBack: () => void }) {
  const [data, setData] = useState<XpDay | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.xpDay(day).then(setData).catch((cause) => setError(errorMessage(cause)));
  }, [day]);

  return (
    <Screen title={dayToDate(day)} subtitle={data ? `${data.total} XP` : "XP"} onBack={onBack} nav={false}>
      {error ? <Banner tone="error">{error}</Banner> : null}
      {data === null && !error ? <Spinner /> : null}
      {data && !data.detailed ? (
        <Card>
          <Text tone="dim" as="p">
            {data.total > 0
              ? `${data.total} XP this day. What earned it is kept from the day the XP log started — this day has its total only.`
              : "No XP this day."}
          </Text>
        </Card>
      ) : null}
      {data && data.detailed ? (
        <Section
          title="What earned it"
          info={
            <p>
              Each go at a lesson, drill or conversation, in order. Open one to see every answer: right
              ones earn XP (harder exercises more, new items a little extra), mistakes and cards you
              only read earn nothing, and extra practice earns half.
            </p>
          }
        >
          <Stack gap={3}>
            {data.runs.map((run, i) => (
              <RunCard key={`${run.startedAt}-${i}`} run={run} />
            ))}
            <Text variant="small" tone="dim" align="center" as="div">{`${data.total} XP in all`}</Text>
          </Stack>
        </Section>
      ) : null}
    </Screen>
  );
}
