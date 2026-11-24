import { Badge, Card, CountUp, Inline, Progress as Meter, Screen, Section, Stack, Text } from "../design";
import { achievementToGo, unlockedOn } from "../lib/achievements";
import type { Achievement } from "../lib/schemas";

/**
 * One achievement's own page: its badge, what it is about, exactly what
 * earns it, and how close the learner is — or when it was earned.
 */
export function AchievementScreen({ achievement, onBack }: { achievement: Achievement; onBack: () => void }) {
  const { unlocked, unlockedDay, current, target, unit } = achievement;
  const units = target === 1 ? unit : `${unit}s`;
  const pct = Math.round((current / Math.max(1, target)) * 100);
  return (
    <Screen title={achievement.title} onBack={onBack} nav={false}>
      <Card>
        <Stack gap={3} align="center">
          <span className="achievement-badge" data-locked={!unlocked} aria-hidden="true">
            {achievement.icon}
          </span>
          <Text variant="headline" as="h2" align="center">
            {achievement.title}
          </Text>
          <Badge tone={unlocked ? "mint" : "neutral"}>{unlocked ? "Unlocked" : "Not yet unlocked"}</Badge>
          <Text tone="dim" align="center" as="p">
            {achievement.description}
          </Text>
        </Stack>
      </Card>

      <Section title="How to get it">
        <Card>
          <Text as="p">{achievement.requirement}</Text>
        </Card>
      </Section>

      <Section title={unlocked ? "Earned" : "Your progress"}>
        <Card>
          <Stack gap={3}>
            <Inline justify="between" wrap>
              <Text variant="lead" weight="bold">
                <CountUp value={current} from={0} /> of {target} {units}
              </Text>
              <Text variant="small" tone="faint">
                {unlocked ? "Done" : `${pct}% · ${achievementToGo(achievement)}`}
              </Text>
            </Inline>
            <Meter value={current} max={target} tone={unlocked ? "mint" : undefined} />
            {unlocked ? (
              <Text variant="small" tone="dim" as="div">
                {unlockedDay !== null ? `Unlocked on ${unlockedOn(unlockedDay)}.` : "Unlocked."} Nicely done!
              </Text>
            ) : (
              <Text variant="small" tone="dim" as="div">
                {current === 0 ? "Not started yet — your first one counts." : "Keep going: every one counts."}
              </Text>
            )}
          </Stack>
        </Card>
      </Section>
    </Screen>
  );
}
