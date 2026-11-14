import {
  Avatar,
  Badge,
  Button,
  Hero,
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  Stack,
  Stat,
  StatGrid,
  Text,
} from "../design";
import { useEffect } from "react";

import { useBackHandler } from "../lib/back";
import { playComplete } from "../lib/sounds";
import { dayToDate } from "../lib/blocks";
import type { BlockResult, SessionResult } from "../lib/types";

const OUTCOME = {
  pass: "Passed",
  partial: "Partly there — it comes back at its usual pace",
  fail: "Not yet — it comes back tomorrow",
} as const;

export function SessionSummary({
  result,
  sessionXp,
  outcome,
  onDone,
}: {
  result: SessionResult;
  sessionXp: number;
  /** The block's verdict and when it checks in next. */
  outcome?: {
    title: string;
    result: BlockResult;
    nextDueDay: number | null;
    /** Extra practice: the schedule did not move. */
    extra?: boolean;
  };
  onDone: () => void;
}) {
  useBackHandler(true, onDone);
  const passed = !outcome || outcome.result === "pass";
  useEffect(() => {
    if (passed) playComplete();
    // Once, as the summary appears.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const { stats, newlyUnlocked } = result;
  // The day is done: the goal reached, or everything due cleared.
  const goalMet = stats.completedToday;

  return (
    <Screen nav={false} header={<div />}>
      <Stack gap={2} align="center">
        <Text variant="display" className="pop">
          {result.firstCompletionToday ? "🎉" : "👏"}
        </Text>
        <h1>{result.firstCompletionToday ? "Day done!" : "Nice work"}</h1>
        <Text tone="dim" align="center" as="p">
          {outcome?.extra
            ? `${outcome.title}: extra practice — half XP, and its schedule stays as it was.`
            : outcome
            ? `${outcome.title}: ${OUTCOME[outcome.result]}.`
            : result.firstCompletionToday
              ? "Today's streak is safe."
              : "Extra practice always counts."}
        </Text>
      </Stack>

      <Hero>
        <Stack gap={3} align="center">
          <Text variant="display">+{sessionXp}</Text>
          <Text weight="medium" tone="dim">
            XP this session
          </Text>
          {outcome?.nextDueDay ? (
            <Text variant="small" tone="dim">
              Next check-in {dayToDate(outcome.nextDueDay)}
            </Text>
          ) : null}
          <Inline gap={2} justify="center">
            <Badge>🔥 {stats.currentStreak} day streak</Badge>
            {stats.streakFreezes > 0 ? <Badge>🧊 {stats.streakFreezes}</Badge> : null}
            <Badge>Level {stats.level}</Badge>
          </Inline>
          {goalMet ? null : (
            <Text variant="small" tone="dim" align="center">
              {Math.max(0, stats.dailyGoal - stats.dayXp)} XP to today's goal — or clear what is due.
            </Text>
          )}
        </Stack>
      </Hero>

      <StatGrid>
        <Stat value={stats.dayXp} label="XP today" />
        <Stat value={stats.wordsSeen} label="words met" />
        <Stat value={`${Math.round(stats.accuracy * 100)}%`} label="accuracy" />
      </StatGrid>

      {newlyUnlocked.length > 0 ? (
        <Section title={`New achievement${newlyUnlocked.length > 1 ? "s" : ""}`}>
          <List>
            {newlyUnlocked.map((achievement) => (
              <ListItem
                key={achievement.code}
                leading={<Avatar>{achievement.icon}</Avatar>}
                title={achievement.title}
                subtitle={achievement.description}
              />
            ))}
          </List>
        </Section>
      ) : null}

      <Button variant="primary" size="lg" block onClick={onDone}>
        Done
      </Button>
    </Screen>
  );
}
