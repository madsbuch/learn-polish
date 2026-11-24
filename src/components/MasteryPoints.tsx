import { useEffect, useState } from "react";

import { Badge, Info, List, ListEmpty, ListItem, Loading, Progress, Section, Sheet, Stack, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { dayToDate, MASTERY_FULL, MASTERY_HELD_AT, passesText, writingStep } from "../lib/lessons";
import type { Lesson, MasteryEntry, MasteryLog } from "../lib/schemas";

/** How mastery points are earned, in one short popover (lessons.rs `mastery_gain`, `credit_mastery`). */
export function MasteryRules() {
  return (
    <>
      <p>
        A pass earns 30 points the first time; another the same day, 10; one a day or more later, 16 rising to 35 at two
        weeks apart. An ace — every answer fully right — earns a quarter more.
      </p>
      <p>
        Short of a pass, a drill earns a share of what a pass would: nothing at half right or less, rising with every
        right answer to the full amount at 85%. A conversation partly there earns half. Nothing ever takes points
        away — a miss brings the lesson back sooner instead.
      </p>
      <p>
        The day a lesson is first passed it can reach {MASTERY_HELD_AT}; the last stretch takes a pass on another day.
        At {MASTERY_FULL} it is mastered, for good (writing also needs a pass without help).
      </p>
      <p>Points from before this log began show as one starting line.</p>
    </>
  );
}

function tone(entry: MasteryEntry): "gold" | "mint" | "crimson" | undefined {
  if (entry.reason === "mastered") return "gold";
  if (entry.delta > 0) return "mint";
  // Rows from before points only grew can still show a loss.
  if (entry.delta < 0) return "crimson";
  return undefined;
}

/** The ledger: every change to a lesson's points, newest first, with the running total. */
function MasteryLogSheet({ lesson, onClose }: { lesson: Lesson; onClose: () => void }) {
  const [log, setLog] = useState<MasteryLog | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.lessonMasteryLog(lesson.id).then(setLog, (cause: unknown) => setError(errorMessage(cause)));
  }, [lesson.id]);
  return (
    <Sheet
      title={
        <span className="with-info">
          Mastery points
          <Info title="How mastery works">
            <MasteryRules />
          </Info>
        </span>
      }
      onClose={onClose}
    >
      <Stack gap={3}>
        <Stack gap={2}>
          <Text weight="medium">
            {lesson.mastered ? `🏆 Mastered · ${lesson.mastery} of ${MASTERY_FULL}` : `${lesson.mastery} of ${MASTERY_FULL}`}
          </Text>
          <Progress value={lesson.mastery} max={MASTERY_FULL} tone={lesson.mastered ? "gold" : undefined} />
          {!lesson.mastered ? (
            <Text variant="small" tone="dim">
              {lesson.mastery >= MASTERY_FULL ? `Next: ${writingStep(lesson) ?? "pass it once more"}` : passesText(lesson)}
            </Text>
          ) : null}
        </Stack>
        {error ? <Text tone="dim">{error}</Text> : null}
        {log === null && !error ? <Loading /> : null}
        {log ? (
          <List>
            {log.entries.map((entry) => (
              <ListItem
                key={entry.id}
                title={entry.text}
                subtitle={dayToDate(entry.day)}
                trailing={<Badge tone={tone(entry)}>{entry.after}</Badge>}
              />
            ))}
            {log.beforeLog !== null ? (
              <ListItem
                title={`Before the log began: ${log.beforeLog} points`}
                subtitle="Earned before every change was written down"
                trailing={<Badge>{log.beforeLog}</Badge>}
              />
            ) : null}
            {log.entries.length === 0 && log.beforeLog === null ? (
              <ListEmpty>Nothing yet: each go at the lesson will show here, with what it earned and why.</ListEmpty>
            ) : null}
          </List>
        ) : null}
      </Stack>
    </Sheet>
  );
}

/** A lesson's mastery on its page: points, a bar and passes left; tap for the ledger. */
export function MasteryPoints({ lesson }: { lesson: Lesson }) {
  const [open, setOpen] = useState(false);
  const subtitle = lesson.mastered
    ? "Mastered, for good"
    : lesson.mastery >= MASTERY_FULL
      ? `Next: ${writingStep(lesson) ?? "pass it once more"}`
      : passesText(lesson);
  return (
    <Section title="Mastery" info={<MasteryRules />}>
      <List>
        <ListItem
          title={
            <Stack gap={2}>
              <span>{lesson.mastered ? `🏆 ${lesson.mastery} of ${MASTERY_FULL}` : `Mastery ${lesson.mastery} of ${MASTERY_FULL}`}</span>
              <Progress
                value={lesson.mastery}
                max={MASTERY_FULL}
                size="sm"
                tone={lesson.mastered ? "gold" : undefined}
                memory={`mastery:${lesson.id}`}
              />
            </Stack>
          }
          subtitle={subtitle}
          label="Mastery points: every change and why"
          onClick={() => setOpen(true)}
          chevron
          trailing={<Text variant="small" tone="dim">Points</Text>}
        />
      </List>
      {open ? <MasteryLogSheet lesson={lesson} onClose={() => setOpen(false)} /> : null}
    </Section>
  );
}
