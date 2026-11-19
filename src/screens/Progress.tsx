import { BarChart, type Bar } from "../components/Charts";
import { XpHistory } from "../components/XpHistory";
import {
  Button,
  Card,
  List,
  ListItem,
  CountUp,
  Inline,
  Progress as Meter,
  Screen,
  Section,
  Stack,
  Stat,
  StatGrid,
  Text,
  useStagger,
} from "../design";
import { achievementProgress, unlockedOnShort } from "../lib/achievements";
import { logEvent } from "../lib/learnLog";
import { dayLabel, REPORT_ASK } from "../lib/insights";
import { useMetOverview, type InsightPage } from "./Insights";
import type { Achievement, DayPoint, Stats } from "../lib/types";

/** Weeks in the practice grid. */
const WEEKS = 18;

/** Day numbers count from Thursday 1 Jan 1970; weeks start on Monday. */
function mondayOf(day: number): number {
  return Math.floor((day + 3) / 7) * 7 - 3;
}

/**
 * The last 18 weeks of practice: a column per week, Monday at the top,
 * this week on the right (its days still to come are left blank).
 */
function Heatmap({ stats }: { stats: Stats }) {
  const byDay = new Map(stats.history.map((entry) => [entry.day, entry]));
  const first = mondayOf(stats.day) - (WEEKS - 1) * 7;
  const cells = [];
  for (let day = first; day <= stats.day; day++) {
    const entry = byDay.get(day);
    // A done day is full; a frozen one shows the freeze that kept the run.
    const level = entry?.completed
      ? 3
      : !entry || entry.xp === 0
        ? 0
        : entry.xp >= 75
          ? 2
          : 1;
    cells.push(
      <i
        key={day}
        data-level={level}
        data-frozen={entry?.frozen || undefined}
      />,
    );
  }
  return (
    <Stack gap={3}>
      <div
        className="heat"
        role="img"
        aria-label={`Your practice over the last ${WEEKS} weeks`}
      >
        {cells}
      </div>
      <Inline gap={2} wrap>
        <Text variant="caption" tone="faint">
          Less
        </Text>
        <span className="heat heat--legend" aria-hidden="true">
          <i data-level={0} />
          <i data-level={1} />
          <i data-level={2} />
          <i data-level={3} />
        </span>
        <Text variant="caption" tone="faint">
          Day done
        </Text>
        <span className="heat heat--legend" aria-hidden="true">
          <i data-frozen />
        </span>
        <Text variant="caption" tone="faint">
          Freeze
        </Text>
      </Inline>
    </Stack>
  );
}

/** One line of the vocabulary card: a count, its total, and a bar. */
function Measure({
  label,
  value,
  max,
  tone,
  memory,
}: {
  label: string;
  value: number;
  max: number;
  tone?: "gold" | "mint";
  memory: string;
}) {
  return (
    <Stack gap={2}>
      <Inline justify="between">
        <Text variant="small">{label}</Text>
        <Text variant="small" tone="faint">
          <CountUp value={value} memory={memory} /> of {max}
        </Text>
      </Inline>
      <Meter value={value} max={max} tone={tone} memory={memory} />
    </Stack>
  );
}

/** An achievement as a tile: its badge, its name, and how far along it is. */
function AchievementTile({
  achievement,
  today,
  onOpen,
}: {
  achievement: Achievement;
  today: number;
  onOpen: () => void;
}) {
  const { unlocked, unlockedDay } = achievement;
  return (
    <button
      className="achievement"
      data-locked={!unlocked}
      onClick={onOpen}
      aria-label={`${achievement.title}: ${
        unlocked ? "unlocked" : achievementProgress(achievement)
      }. ${achievement.description}`}
    >
      <span className="achievement-icon" aria-hidden="true">
        {achievement.icon}
      </span>
      <span className="achievement-title">{achievement.title}</span>
      {unlocked ? (
        <span className="achievement-meta" data-done="true">
          ✓{" "}
          {unlockedDay !== null
            ? unlockedOnShort(unlockedDay, today)
            : "Earned"}
        </span>
      ) : (
        <span className="achievement-meta">
          <Meter
            value={achievement.current}
            max={achievement.target}
            size="sm"
            memory={`achievement:${achievement.code}`}
          />
          <span>
            {achievement.current}/{achievement.target}
          </span>
        </span>
      )}
    </button>
  );
}

/** "What you've met": words, structures and sounds, each opening its page. */
function recentBars(daily: DayPoint[], today: number): Bar[] {
  const by = new Map(daily.map((d) => [d.day, d]));
  return Array.from({ length: 30 }, (_, i) => {
    const day = today - 29 + i;
    const point = by.get(day);
    const share = point && point.attempts ? point.correct / point.attempts : 0;
    return {
      key: day,
      label: `${dayLabel(day, today)}: ${point ? `${Math.round(share * 100)}% of ${point.attempts}` : "no answers"}`,
      value: Math.round(share * 100),
      share,
      empty: !point,
      faint: Boolean(point && point.attempts < 3),
    };
  });
}

function Met({
  today,
  onOpen,
  onAskTutor,
}: {
  today: number;
  onOpen: (page: InsightPage) => void;
  onAskTutor: (ask: string) => void;
}) {
  const { data } = useMetOverview();
  const cards: {
    page: InsightPage;
    value: number;
    label: string;
    note: string;
    memory: string;
  }[] = [
    {
      page: { page: "words" },
      value: data?.wordsMet ?? 0,
      label: "Words",
      note: "Search, filter, the trickiest first",
      memory: "met:words",
    },
    {
      page: { page: "structures" },
      value: data?.structuresMet ?? 0,
      label: "Structures",
      note: "Cases, verb forms, agreement…",
      memory: "met:structures",
    },
    {
      page: { page: "sounds" },
      value: data?.soundsMet ?? 0,
      label: "Sounds",
      note: "Near pairs you tell apart",
      memory: "met:sounds",
    },
  ];
  return (
    <div className="met-board">
      <Section
        title="What you've met"
        info={
          <p>
            Every word, grammar structure and sound you have practised, with how
            well each is going and the mistakes you make on it.
          </p>
        }
      >
        <div className="met-cards">
          {cards.map((card) => (
            <Card
              key={card.label}
              onClick={() => onOpen(card.page)}
              className="met-card"
            >
              <b>
                <CountUp value={card.value} memory={card.memory} />
              </b>
              <Text weight="bold">{card.label}</Text>
              <Text variant="caption" tone="dim">
                {card.note}
              </Text>
            </Card>
          ))}
        </div>
        {data && data.attempts > 0 ? (
          <Card>
            <Stack gap={2}>
              <Text variant="small" tone="dim">
                Right answers, last 30 days
              </Text>
              <BarChart
                bars={recentBars(data.daily, today)}
                top="100%"
                start="30 days ago"
                end="Today"
                label="Share of answers right each day, the last 30 days"
              />
            </Stack>
          </Card>
        ) : null}
      </Section>
      <Section
        title="Insights"
        info={
          <p>
            What your answers say: where you stand, what is hardest, your
            mistakes by kind, two periods side by side, and how you practise.
          </p>
        }
      >
        <Stack gap={3}>
          <List>
            <ListItem
              title="Overview"
              subtitle={
                data && data.attempts
                  ? `${data.attempts} answers · ${Math.round(data.accuracy * 100)}% right`
                  : "Your answers at a glance"
              }
              onClick={() => onOpen({ page: "overview" })}
            />
            <ListItem
              title="Hardest"
              subtitle="What you miss most"
              onClick={() => onOpen({ page: "hard", kind: "word" })}
            />
            <ListItem
              title="Mistakes by kind"
              onClick={() => onOpen({ page: "mistakes", cls: null })}
            />
            <ListItem
              title="Compare two periods"
              subtitle="What improved, what slipped"
              onClick={() => onOpen({ page: "compare" })}
            />
            <ListItem
              title="Activity and habits"
              subtitle="When and how you practise"
              onClick={() => onOpen({ page: "activity" })}
            />
          </List>
          <Button
            icon={<span aria-hidden="true">✨</span>}
            onClick={() => {
              logEvent({ kind: "report_asked" });
              onAskTutor(REPORT_ASK);
            }}
          >
            Your learning report
          </Button>
        </Stack>
      </Section>
    </div>
  );
}

export function Progress({
  stats,
  onOpenDay,
  onOpenAchievement,
  onOpenInsights,
  onAskTutor,
  onBack,
}: {
  /** A "What you've met" or Insights page. */
  onOpenInsights: (page: InsightPage) => void;
  /** Starts a tutor conversation with the learner's request (the learning report). */
  onAskTutor: (ask: string) => void;
  stats: Stats;
  /** What earned a day's XP. */
  onOpenDay: (day: number) => void;
  /** An achievement's own page. */
  onOpenAchievement: (code: string) => void;
  onBack: () => void;
}) {
  const rise = useStagger();
  const earned = stats.achievements.filter((a) => a.unlocked).length;
  return (
    <Screen
      title="Progress"
      subtitle="Everything you have learned so far"
      onBack={onBack}
      nav={false}
      width="wide"
    >
      <div className="progress-board">
        <div className="progress-board__stats">
          <StatGrid>
            <Stat
              value={<CountUp value={stats.currentStreak} memory="streak" />}
              label="day streak"
            />
            <Stat
              value={
                <CountUp value={stats.longestStreak} memory="streak:best" />
              }
              label="best streak"
            />
            <Stat
              value={<CountUp value={stats.streakFreezes} memory="freezes" />}
              label="streak freezes"
            />
            <Stat
              value={<CountUp value={stats.level} memory="level" />}
              label={`level · ${stats.totalXp} XP`}
            />
          </StatGrid>
        </div>

        <div className="progress-board__met">
          <Met
            today={stats.day}
            onOpen={onOpenInsights}
            onAskTutor={onAskTutor}
          />
        </div>

        <div className="progress-board__achievements">
          <Section
            title={`Achievements · ${earned} of ${stats.achievements.length}`}
            info={
              <p>
                Badges for showing up and for the deeper work: streaks, words
                that stick, lessons passed in conversation. Tap one to see
                exactly what it takes and how close you are.
              </p>
            }
          >
            <div
              className={rise ? `achievement-grid ${rise}` : "achievement-grid"}
            >
              {stats.achievements.map((achievement) => (
                <AchievementTile
                  key={achievement.code}
                  achievement={achievement}
                  today={stats.day}
                  onOpen={() => onOpenAchievement(achievement.code)}
                />
              ))}
            </div>
          </Section>
        </div>

        <div className="progress-board__xp">
          <XpHistory today={stats.day} onOpenDay={onOpenDay} />
        </div>

        <div className="progress-board__heat">
          <Section
            title={`Last ${WEEKS} weeks`}
            info={
              <p>
                One square a day, a column a week (Monday at the top). The
                brighter, the more you practised; full colour is a day with all
                its lessons done. Blue is a missed day a streak freeze covered.
              </p>
            }
          >
            <Card>
              <Heatmap stats={stats} />
            </Card>
          </Section>
        </div>

        <div className="progress-board__vocab">
          <Section title="Vocabulary">
            <Card>
              <Stack gap={4}>
                <Measure
                  label="Words met"
                  value={stats.wordsSeen}
                  max={stats.wordsTotal}
                  memory="vocab:seen"
                />
                <Measure
                  label="Words that stick (3 weeks apart)"
                  value={stats.wordsMastered}
                  max={stats.wordsTotal}
                  tone="mint"
                  memory="vocab:mastered"
                />
                <Measure
                  label="Sentences practised"
                  value={stats.sentencesSeen}
                  max={stats.sentencesTotal}
                  tone="gold"
                  memory="vocab:sentences"
                />
              </Stack>
            </Card>
          </Section>
        </div>
      </div>
    </Screen>
  );
}
