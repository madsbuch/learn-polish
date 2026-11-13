import type { CSSProperties } from "react";

import { Logo, SearchIcon } from "../components/icons";
import { TopicList } from "../components/TopicList";
import {
  Avatar,
  Badge,
  Button,
  EmptyState,
  Hero,
  IconButton,
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  Stack,
  Text,
} from "../design";
import type { SessionPlan, Stats, TopicProgress } from "../lib/types";

function greeting(): string {
  const hour = new Date().getHours();
  if (hour < 11) return "Dzień dobry";
  if (hour < 18) return "Cześć";
  return "Dobry wieczór";
}

/**
 * Everything to practise in one place: today's check-in on top, then your
 * programmes and the course. Progress and the word search open from here.
 */
export function Exercises({
  plan,
  stats,
  topics,
  focusTopicId,
  onStart,
  onOpenTopic,
  onOpenTutor,
  onOpenProgress,
  onOpenSearch,
  onBackToCourse,
}: {
  plan: SessionPlan;
  stats: Stats;
  topics: TopicProgress[];
  focusTopicId: number | null;
  onStart: () => void;
  onOpenTopic: (progress: TopicProgress) => void;
  onOpenTutor: () => void;
  onOpenProgress: () => void;
  onOpenSearch: () => void;
  /** Ends the daily focus on a programme. */
  onBackToCourse: () => void;
}) {
  const course = topics.filter((entry) => entry.topic.kind === "course");
  const programmes = topics.filter((entry) => entry.topic.kind === "programme");
  const programme = plan.topic.kind === "programme";
  const goalPct = Math.min(100, Math.round((stats.dayXp / Math.max(1, stats.dailyGoal)) * 100));
  const items = plan.newWords + plan.reviewWords + plan.newSentences + plan.reviewSentences;
  const minutes = Math.max(3, Math.round(items * 0.4));
  const contents = [
    plan.dialogue.length > 0 ? "a conversation" : null,
    plan.newWords > 0 ? `${plan.newWords} new words` : null,
    plan.reviewWords > 0 ? `${plan.reviewWords} to review` : null,
    plan.newSentences + plan.reviewSentences > 0
      ? `${plan.newSentences + plan.reviewSentences} sentences`
      : null,
  ].filter(Boolean);

  return (
    <Screen
      header={
        <header className="screen-header">
          <Logo size={40} />
          <div className="screen-header__titles">
            <Text variant="eyebrow">{greeting()}</Text>
            <h1>Exercises</h1>
          </div>
          <IconButton icon={<SearchIcon />} label="Search words and sentences" onClick={onOpenSearch} />
        </header>
      }
    >
      <Hero>
        <Stack gap={4}>
          <Inline gap={4}>
            <div
              className="goal-ring"
              style={{ "--value": goalPct } as CSSProperties}
              role="img"
              aria-label={`${stats.dayXp} of ${stats.dailyGoal} XP today`}
            >
              <div>
                <Text variant="title" weight="bold">
                  {stats.dayXp}
                </Text>
                <Text variant="caption" tone="dim">
                  / {stats.dailyGoal} XP
                </Text>
              </div>
            </div>
            <div className="grow">
              <Text variant="eyebrow">
                {programme ? "Your programme" : `Topic ${plan.topic.ord} of ${course.length}`}
              </Text>
              <Text variant="title" as="h2">
                {plan.topic.titlePl}
              </Text>
              <Text variant="small" tone="dim" as="div">
                {plan.topic.title}
              </Text>
            </div>
          </Inline>
          {plan.topic.summary ? (
            <Text variant="small" tone="dim" as="p" clamp={2}>
              {plan.topic.summary}
            </Text>
          ) : null}
          <Stack gap={2}>
            <Button variant="inverse" size="lg" block onClick={onStart}>
              {stats.completedToday ? "Practise again" : "Start today's check-in"}
            </Button>
            <Text variant="caption" tone="dim" align="center" as="div">
              {stats.completedToday
                ? "✓ Done for today — your streak is safe"
                : `About ${minutes} min · ${contents.join(" · ")}`}
            </Text>
          </Stack>
          {programme ? (
            <Button variant="plain" size="sm" onClick={onBackToCourse}>
              Back to the course
            </Button>
          ) : null}
        </Stack>
      </Hero>

      <List>
        <ListItem
          leading={<Avatar>📈</Avatar>}
          title="Your progress"
          subtitle={`${stats.wordsSeen} words · ${stats.dueToday} due`}
          trailing={
            <Inline gap={1}>
              <Badge tone="crimson">🔥 {stats.currentStreak}</Badge>
              <Badge tone="gold">Lv {stats.level}</Badge>
            </Inline>
          }
          chevron
          onClick={onOpenProgress}
        />
      </List>

      <Section title="Your programmes">
        {programmes.length === 0 ? (
          <List>
            <EmptyState
              icon="✨"
              title="Make your own"
              action={
                <Button variant="primary" size="sm" onClick={onOpenTutor}>
                  Ask the tutor
                </Button>
              }
            >
              Ask the tutor for a programme on anything — “colours”, “numbers”, “at the
              airport”.
            </EmptyState>
          </List>
        ) : (
          <TopicList entries={programmes} focusTopicId={focusTopicId} onOpen={onOpenTopic} />
        )}
      </Section>

      <Section title="Course">
        <TopicList entries={course} focusTopicId={focusTopicId} onOpen={onOpenTopic} />
      </Section>
    </Screen>
  );
}
