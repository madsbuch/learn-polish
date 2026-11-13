import type { CSSProperties } from "react";

import { BlockRow } from "../components/BlockRow";
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
import { minutes } from "../lib/blocks";
import type { Block, BlockOverview, Stats, TopicProgress } from "../lib/types";

function greeting(): string {
  const hour = new Date().getHours();
  if (hour < 11) return "Dzień dobry";
  if (hour < 18) return "Cześć";
  return "Dobry wieczór";
}

/** Rough minutes for a list of blocks, for the summary line. */
function estimate(blocks: Block[]): number {
  return blocks.reduce((sum, b) => sum + (b.complexity === "low" ? 2 : b.complexity === "medium" ? 7 : 20), 0);
}

/**
 * Everything to practise: what is due for a check-in, what to start next,
 * the study lessons, and the topics and programmes they come from.
 */
export function Exercises({
  overview,
  stats,
  topics,
  onOpenBlock,
  onOpenTopic,
  onOpenTutor,
  onOpenProgress,
  onOpenSearch,
}: {
  overview: BlockOverview;
  stats: Stats;
  topics: TopicProgress[];
  onOpenBlock: (block: Block) => void;
  onOpenTopic: (progress: TopicProgress) => void;
  onOpenTutor: () => void;
  onOpenProgress: () => void;
  onOpenSearch: () => void;
}) {
  const course = topics.filter((entry) => entry.topic.kind === "course");
  const programmes = topics.filter((entry) => entry.topic.kind === "programme");
  const goalPct = Math.min(100, Math.round((stats.dayXp / Math.max(1, stats.dailyGoal)) * 100));
  const upNext = overview.due[0] ?? overview.next[0] ?? null;
  const lessonsToStart = overview.lessons.filter((lesson) => lesson.dueDay === null);

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
              <Text variant="eyebrow">Today</Text>
              <Text variant="title" as="h2">
                {overview.due.length > 0
                  ? `${overview.due.length} check-in${overview.due.length === 1 ? "" : "s"} due`
                  : "All caught up"}
              </Text>
              <Text variant="small" tone="dim" as="div">
                {overview.due.length > 0
                  ? `About ${estimate(overview.due)} min`
                  : overview.doneToday > 0
                    ? `${overview.doneToday} block${overview.doneToday === 1 ? "" : "s"} done today`
                    : "Start something new below"}
              </Text>
            </div>
          </Inline>
          {upNext ? (
            <Stack gap={2}>
              <Button variant="inverse" size="lg" block onClick={() => onOpenBlock(upNext)}>
                {overview.due.length > 0 ? "Start the first check-in" : "Start something new"}
              </Button>
              <Text variant="caption" tone="dim" align="center" as="div">
                {upNext.title}
                {upNext.topicTitle ? ` · ${upNext.topicTitle}` : ""} · {minutes(upNext.complexity)}
              </Text>
            </Stack>
          ) : null}
        </Stack>
      </Hero>

      {overview.due.length > 0 ? (
        <Section title="Due now">
          <List>
            {overview.due.map((block) => (
              <BlockRow key={block.id} block={block} today={overview.today} showTopic onOpen={onOpenBlock} />
            ))}
          </List>
        </Section>
      ) : null}

      {overview.next.length > 0 ? (
        <Section title="Start next">
          <List>
            {overview.next.map((block) => (
              <BlockRow key={block.id} block={block} today={overview.today} showTopic onOpen={onOpenBlock} />
            ))}
          </List>
        </Section>
      ) : null}

      <Section title="Study">
        <List>
          {(lessonsToStart.length > 0 ? lessonsToStart : overview.lessons).slice(0, 4).map((block) => (
            <BlockRow key={block.id} block={block} today={overview.today} onOpen={onOpenBlock} />
          ))}
          <ListItem
            leading={<Avatar>✨</Avatar>}
            title="Ask the tutor for a lesson"
            subtitle="Any conjugation, grammar point or bit of culture"
            onClick={onOpenTutor}
          />
        </List>
      </Section>

      <List>
        <ListItem
          leading={<Avatar>📈</Avatar>}
          title="Your progress"
          subtitle={`${stats.wordsSeen} words · level ${stats.level}`}
          trailing={
            <Inline gap={1}>
              <Badge tone="crimson">🔥 {stats.currentStreak}</Badge>
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
              Ask the tutor for practice on anything — “colours”, “numbers”, “at the airport”.
            </EmptyState>
          </List>
        ) : (
          <TopicList entries={programmes} onOpen={onOpenTopic} />
        )}
      </Section>

      <Section title="Course">
        <TopicList entries={course} onOpen={onOpenTopic} />
      </Section>
    </Screen>
  );
}
