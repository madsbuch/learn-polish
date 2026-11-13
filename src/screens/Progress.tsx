import { TopicList } from "../components/TopicList";
import { Card, Inline, Progress as Meter, Screen, Section, Stack, Stat, StatGrid, Text } from "../design";
import type { Stats, TopicProgress } from "../lib/types";
/** 18 weeks of activity, oldest first, in Sunday-aligned columns. */
function Heatmap({ stats }: { stats: Stats }) {
  const byDay = new Map(stats.history.map((entry) => [entry.day, entry]));
  const cells = [];
  for (let offset = 125; offset >= 0; offset--) {
    const day = stats.day - offset;
    const entry = byDay.get(day);
    const level = !entry || entry.xp === 0 ? 0 : entry.xp >= stats.dailyGoal ? 3 : entry.xp >= stats.dailyGoal / 2 ? 2 : 1;
    cells.push(<i key={day} data-level={level} />);
  }
  return <div className="heat">{cells}</div>;
}

/** One line of the vocabulary card: a count, its total, and a bar. */
function Measure({
  label,
  value,
  max,
  tone,
}: {
  label: string;
  value: number;
  max: number;
  tone?: "gold" | "mint";
}) {
  return (
    <Stack gap={2}>
      <Inline justify="between">
        <Text variant="small">{label}</Text>
        <Text variant="small" tone="faint">
          {value} of {max}
        </Text>
      </Inline>
      <Meter value={value} max={max} tone={tone} />
    </Stack>
  );
}

export function Progress({
  stats,
  topics,
  onOpenTopic,
  onBack,
}: {
  stats: Stats;
  topics: TopicProgress[];
  onOpenTopic: (progress: TopicProgress) => void;
  onBack: () => void;
}) {
  return (
    <Screen title="Progress" subtitle="Everything you have learned so far" onBack={onBack} nav={false}>
      <StatGrid>
        <Stat value={stats.currentStreak} label="day streak" />
        <Stat value={stats.longestStreak} label="best streak" />
        <Stat value={stats.totalXp} label="total XP" />
        <Stat value={stats.level} label="level" />
      </StatGrid>

      <Section title="Last 18 weeks">
        <Card>
          <Heatmap stats={stats} />
        </Card>
      </Section>

      <Section title="Vocabulary">
        <Card>
          <Stack gap={4}>
            <Measure label="Words met" value={stats.wordsSeen} max={stats.wordsTotal} />
            <Measure label="Words mastered" value={stats.wordsMastered} max={stats.wordsTotal} tone="mint" />
            <Measure label="Sentences practised" value={stats.sentencesSeen} max={stats.sentencesTotal} tone="gold" />
          </Stack>
        </Card>
      </Section>

      <Section title="Topics">
        <TopicList entries={topics} onOpen={onOpenTopic} />
      </Section>

      <Section title="Achievements">
        <div className="achievement-grid">
          {stats.achievements.map((achievement) => (
            <div
              key={achievement.code}
              className="achievement"
              data-locked={!achievement.unlocked}
              title={achievement.description}
            >
              <span className="achievement-icon">{achievement.icon}</span>
              <Text variant="caption" weight="bold">
                {achievement.title}
              </Text>
              {!achievement.unlocked ? (
                <div className="full-width">
                  <Meter value={achievement.progress} max={1} size="sm" />
                </div>
              ) : null}
            </div>
          ))}
        </div>
      </Section>
    </Screen>
  );
}
