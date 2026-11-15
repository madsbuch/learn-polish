import { useState, type CSSProperties } from "react";

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
 * Everything to practise, top to bottom: today (with the one thing to do
 * next), what is due for a check-in, and the courses every block lives in.
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
  // A done day (the goal reached, or everything due cleared) fills the ring.
  const goalPct = stats.completedToday
    ? 100
    : Math.min(100, Math.round((stats.dayXp / Math.max(1, stats.dailyGoal)) * 100));
  // Finished courses leave the list for their own shelf.
  const active = topics.filter((t) => t.status === "active");
  const finished = topics.filter((t) => t.status !== "active");
  const [showFinished, setShowFinished] = useState(false);
    // Today's list first; once it's done, more for a big day; else something new.
  const keepGoing = overview.due.length === 0 && overview.more.length > 0;
  const upNext = overview.due[0] ?? overview.more[0] ?? overview.next[0] ?? null;
  const balance =
    overview.behindMinutes > 0
      ? `About ${overview.behindMinutes} min more is due — it waits for another day, or keep going`
      : overview.aheadMinutes > 0
        ? `${overview.aheadMinutes} min ahead this week`
        : null;

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
      {topics.length > 0 ? <Hero>
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
              <Inline gap={2} wrap>
                <Text variant="eyebrow">{stats.completedToday ? "Today is done ✓" : "Today"}</Text>
                {stats.currentStreak > 0 ? <Badge>🔥 {stats.currentStreak}</Badge> : null}
                {stats.streakFreezes > 0 ? (
                  <Badge>
                    🧊 {stats.streakFreezes}
                  </Badge>
                ) : null}
              </Inline>
              <Text variant="title" as="h2">
                {overview.due.length > 0
                  ? `${overview.due.length} check-in${overview.due.length === 1 ? "" : "s"} today`
                  : overview.planTotal > 0
                    ? "Today's list is done"
                    : "All caught up"}
              </Text>
              <Text variant="small" tone="dim" as="div">
                {overview.due.length > 0
                  ? `About ${estimate(overview.due)} min${
                      overview.planDone > 0 ? ` · ${overview.planDone} of ${overview.planTotal} done` : ""
                    }`
                  : overview.doneToday > 0
                    ? `${overview.doneToday} block${overview.doneToday === 1 ? "" : "s"} done today`
                    : "Start something new below"}
              </Text>
              {balance ? (
                <Text variant="caption" tone="dim" as="div">
                  {balance}
                </Text>
              ) : null}
            </div>
          </Inline>
          {upNext ? (
            <Stack gap={2}>
              <Button variant="inverse" size="lg" block onClick={() => onOpenBlock(upNext)}>
                {overview.due.length > 0
                  ? "Start the first check-in"
                  : keepGoing
                    ? "Keep going"
                    : "Start something new"}
              </Button>
              <Text variant="caption" tone="dim" align="center" as="div">
                {upNext.title}
                {upNext.topicTitle ? ` · ${upNext.topicTitle}` : ""} · {minutes(upNext.complexity)}
              </Text>
            </Stack>
          ) : null}
        </Stack>
      </Hero> : null}

      {overview.due.length > 0 ? (
        <Section
          title="Today's check-ins"
          info={
            <>
              <p>
                The blocks most at risk of being forgotten, as many as fit today. Clear these and
                the day counts for your streak.
              </p>
              <p>
                How much goes on the list follows what you usually do, never less than the quiet-day
                minimum in Settings. Anything else that is due waits for another day without counting
                as missed.
              </p>
            </>
          }
        >
          <List>
            {overview.due.map((block) => (
              <BlockRow key={block.id} block={block} today={overview.today} showTopic onOpen={onOpenBlock} />
            ))}
          </List>
        </Section>
      ) : null}

      {keepGoing ? (
        <Section
          title="Keep going"
          info={
            <p>
              Today's list is done. These are next: the rest of what is due, then what comes due
              soonest. Doing one early is a real check-in — it moves out from the time that has
              passed, and comes off the day it was due.
            </p>
          }
        >
          <List>
            {overview.more.map((block) => (
              <BlockRow key={block.id} block={block} today={overview.today} showTopic onOpen={onOpenBlock} />
            ))}
          </List>
        </Section>
      ) : null}

      {topics.length > 0 || stats.totalXp > 0 ? <List>
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
      </List> : null}

      <Section
        title="Courses"
        info={
          <p>
            A course is a folder of blocks on one theme: drills on its words, sentences and
            dialogue, and lessons with a check-in. Ask the tutor for a new course on anything, or to
            add lessons and drills to one you have.
          </p>
        }
      >
        {topics.length === 0 ? (
          <List>
            <EmptyState
              icon="✨"
              title="Build your first Polish course"
              action={
                <Button variant="primary" size="sm" onClick={onOpenTutor}>
                  Create a course
                </Button>
              }
            >
              Tell the tutor what you want to learn — everyday conversation, life in Poland, or a topic you enjoy.
            </EmptyState>
          </List>
        ) : (
          <Stack gap={3}>
            <TopicList entries={active} onOpen={onOpenTopic} />
            <List>
              <ListItem
                leading={<Avatar>✨</Avatar>}
                title="New course or lesson"
                subtitle="Ask the tutor for anything — a theme, a conjugation, a grammar point"
                onClick={onOpenTutor}
              />
            </List>
          </Stack>
        )}
      </Section>

      {finished.length > 0 ? (
        <Section
          title="Completed"
          info={
            <p>
              Courses where every block is passed. Their blocks still come back in today's
              check-ins now and then; keep them up and a course becomes mastered 🏆.
            </p>
          }
        >
          {showFinished ? (
            <TopicList entries={finished} onOpen={onOpenTopic} />
          ) : (
            <List>
              <ListItem
                leading={<Avatar>✓</Avatar>}
                title={`${finished.length} completed course${finished.length === 1 ? "" : "s"}`}
                subtitle={
                  finished.filter((t) => t.status === "mastered").length > 0
                    ? `${finished.filter((t) => t.status === "mastered").length} mastered 🏆`
                    : "Tap to see them"
                }
                onClick={() => setShowFinished(true)}
              />
            </List>
          )}
        </Section>
      ) : null}
    </Screen>
  );
}
