import { XpHistory } from "../components/XpHistory";
import {
  Card,
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
import type { Achievement, Stats } from "../lib/types";

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
    const level = entry?.completed ? 3 : !entry || entry.xp === 0 ? 0 : entry.xp >= 75 ? 2 : 1;
    cells.push(<i key={day} data-level={level} data-frozen={entry?.frozen || undefined} />);
  }
  return (
    <Stack gap={3}>
      <div className="heat" role="img" aria-label={`Your practice over the last ${WEEKS} weeks`}>
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
          ✓ {unlockedDay !== null ? unlockedOnShort(unlockedDay, today) : "Earned"}
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

export function Progress({
  stats,
  onOpenDay,
  onOpenAchievement,
  onBack,
}: {
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
    <Screen title="Progress" subtitle="Everything you have learned so far" onBack={onBack} nav={false} width="wide">
      <div className="progress-board">
        <div className="progress-board__stats">
          <StatGrid>
            <Stat value={<CountUp value={stats.currentStreak} memory="streak" />} label="day streak" />
            <Stat value={<CountUp value={stats.longestStreak} memory="streak:best" />} label="best streak" />
            <Stat value={<CountUp value={stats.streakFreezes} memory="freezes" />} label="streak freezes" />
            <Stat value={<CountUp value={stats.level} memory="level" />} label={`level · ${stats.totalXp} XP`} />
          </StatGrid>
        </div>

        <div className="progress-board__achievements">
          <Section
            title={`Achievements · ${earned} of ${stats.achievements.length}`}
            info={
              <p>
                Badges for showing up and for the deeper work: streaks, words that stick, lessons passed in
                conversation. Tap one to see exactly what it takes and how close you are.
              </p>
            }
          >
            <div className={rise ? `achievement-grid ${rise}` : "achievement-grid"}>
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
                One square a day, a column a week (Monday at the top). The brighter, the more you practised; full colour
                is a day with all its lessons done. Blue is a missed day a streak freeze covered.
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
                <Measure label="Words met" value={stats.wordsSeen} max={stats.wordsTotal} memory="vocab:seen" />
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
