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
  outcome?: { title: string; result: BlockResult; nextDueDay: number | null };
  onDone: () => void;
}) {
  const { stats, newlyUnlocked } = result;
  const goalMet = stats.dayXp >= stats.dailyGoal;

  return (
    <Screen nav={false} header={<div />}>
      <Stack gap={2} align="center">
        <Text variant="display" className="pop">
          {goalMet ? "🎉" : "👏"}
        </Text>
        <h1>{goalMet ? "Goal reached!" : "Nice work"}</h1>
        <Text tone="dim" align="center" as="p">
          {outcome
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
            <Badge>Level {stats.level}</Badge>
          </Inline>
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
