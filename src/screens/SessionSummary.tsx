import {
  Banner,
  Avatar,
  Badge,
  Button,
  Confetti,
  CountUp,
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
import { dayToDate } from "../lib/lessons";
import type { LessonResult, SessionResult } from "../lib/types";

const OUTCOME = {
  pass: "Passed",
  partial:
    "Partly there — 85% right passes (in a short lesson, all but one); try again now, or it comes back at its usual pace",
  fail: "Not yet — 85% right passes (in a short lesson, all but one); try again now, or it comes back tomorrow",
} as const;

const percent = (n: number) => `${n}%`;

export function SessionSummary({
  result,
  sessionXp,
  outcome,
  levelUp,
  onDone,
}: {
  result: SessionResult;
  sessionXp: number;
  /** This lesson took the learner to a new level: a small party. */
  levelUp?: boolean;
  /** The lesson's verdict and when it checks in next. */
  outcome?: {
    title: string;
    result: LessonResult;
    nextDueDay: number | null;
    /** Extra practice: the schedule did not move. */
    extra?: boolean;
    /** False when nothing was scored: not graded at all. */
    graded?: boolean;
    /** What it did to the lesson's mastery points, in words (its ledger row). */
    mastery?: string;
    /** A drill's score; a check-in has none. */
    /** `almost`: one letter off, each half right. */
    score?: { correct: number; almost: number; total: number };
  };
  onDone: () => void;
}) {
  useBackHandler(true, onDone);
  const graded = outcome?.graded !== false;
  const passed = !outcome || (graded && outcome.result === "pass");
  useEffect(() => {
    if (passed) playComplete();
    // Once, as the summary appears.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const { stats, newlyUnlocked } = result;
  const score = outcome?.score;
  const scoreText = score
    ? ` — ${score.correct} of ${score.total} right${score.almost > 0 ? `, ${score.almost} almost` : ""}`
    : "";
  // The day is done: the goal reached, or everything due cleared.
  const goalMet = stats.completedToday;
  // The day done, or a new level: confetti, once, as the summary appears.
  const party = result.firstCompletionToday || Boolean(levelUp);

  return (
    <Screen nav={false} header={<div />} stagger>
      {party ? <Confetti fixed pieces={36} /> : null}
      <Stack gap={2} align="center">
        <Text variant="display" className={party ? "celebrate-rise" : "pop"}>
          {result.firstCompletionToday ? "🎉" : "👏"}
        </Text>
        <h1>{result.firstCompletionToday ? "Day done!" : "Nice work"}</h1>
        <Text tone="dim" align="center" as="p">
          {outcome && !graded
            ? `${outcome.title}: nothing was scored this time, so it isn't graded — no change to its mastery or its schedule.`
            : outcome?.extra
            ? `${outcome.title}${scoreText}: already passed today, so this is extra practice: half XP, and its schedule stays as it was.`
            : outcome
              ? `${outcome.title}${scoreText}: ${OUTCOME[outcome.result]}.`
              : result.firstCompletionToday
                ? "Today's streak is safe."
                : "Extra practice always counts."}
        </Text>
      </Stack>

      {(result.milestones ?? []).map((milestone) => (
        <Banner key={`${milestone.courseId}-${milestone.mastered}`} tone="ok">
          {milestone.mastered
            ? `🏆 You mastered “${milestone.title}” — every lesson in it. Close it on its page to keep it as a trophy.`
            : `🎉 You completed “${milestone.title}”! Next, master it: every pass adds mastery. Practise its lessons now, or let your check-ins bring them back — spaced out, it takes fewer passes.`}
        </Banner>
      ))}

      <Hero>
        <Stack gap={3} align="center">
          <Text variant="display">
            +<CountUp value={sessionXp} from={0} duration={900} />
          </Text>
          <Text weight="medium" tone="dim">
            XP this session
          </Text>
          {outcome?.mastery ? (
            <Text variant="small" tone="dim" align="center">
              Mastery: {outcome.mastery}
            </Text>
          ) : null}
          {outcome?.nextDueDay ? (
            <Text variant="small" tone="dim">
              Next check-in {dayToDate(outcome.nextDueDay)}
            </Text>
          ) : null}
          <Inline gap={2} justify="center">
            <Badge>
              🔥 <CountUp value={stats.currentStreak} memory="streak" /> day streak
            </Badge>
            {stats.streakFreezes > 0 ? <Badge>🧊 {stats.streakFreezes}</Badge> : null}
            {levelUp ? (
              <span className="level-up">
                <Badge>Level {stats.level}</Badge>
              </span>
            ) : (
              <Badge>Level {stats.level}</Badge>
            )}
          </Inline>
          <Text variant="small" tone="dim" align="center">
            {goalMet
              ? "🎉 Today's lessons are done — see you tomorrow."
              : `${stats.dueToday} of today's lesson${stats.dueToday === 1 ? "" : "s"} to go.`}
          </Text>
        </Stack>
      </Hero>

      <StatGrid>
        <Stat value={<CountUp value={stats.dayXp} memory="xp:today" />} label="XP today" />
        {score ? (
          <Stat
            value={<CountUp value={Math.round(((score.correct + score.almost / 2) / score.total) * 100)} from={0} format={percent} />}
            label="this lesson"
          />
        ) : (
          <Stat value={<CountUp value={stats.wordsSeen} memory="words:seen" />} label="words met" />
        )}
        <Stat
          value={<CountUp value={Math.round(stats.accuracy * 100)} memory="accuracy" format={percent} />}
          label="accuracy, all time"
        />
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

      <Button variant="primary" size="lg" wide onClick={onDone}>
        Done
      </Button>
    </Screen>
  );
}
