import { useEffect } from "react";

import {
  Avatar,
  Badge,
  Banner,
  Button,
  Card,
  Confetti,
  CountUp,
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  Stack,
  Text,
} from "../design";
import { MASTERY_FULL } from "../lib/lessons";
import { playComplete } from "../lib/sounds";
import {
  barEnds,
  celebrates,
  GRADE_WORD,
  impactLine,
  masteryReason,
  nextLine,
  scoreText,
  signed,
  SUMMARY_TEXT,
  toneOf,
  type Grade,
  type Tone,
} from "../lib/summary";
import type { Impact, MasteryEntry, SessionResult } from "../lib/types";

/** A card missed in a drill, or answered almost right. */
export interface Miss {
  key: string;
  prompt: string;
  given: string;
  expected: string;
  almost: boolean;
  /** A word's (wd_…) or sentence's (sn_…) id opens its page. */
  itemId: string;
}

/** What a finished drill or conversation hands its summary. */
export interface SummaryData {
  result: SessionResult;
  /** This go's XP. */
  xp: number;
  /** This go earned a streak freeze. */
  freezeEarned: boolean;
  title: string;
  /** False when nothing was scored. */
  graded: boolean;
  grade: Grade | "";
  extra: boolean;
  /** A drill's score; a conversation has none. */
  score?: { correct: number; almost: number; total: number };
  /** The go's row in the mastery ledger. */
  mastery: MasteryEntry | null;
  masteredNow: boolean;
  impact: Impact | null;
  nextDueDay: number | null;
  /** A drill's misses. */
  misses: Miss[];
  /** A conversation's: what the verdict says to go over. */
  toReview: string[];
}

const EMOJI: Partial<Record<Tone, string>> = { ace: "🌟", pass: "👏", partial: "💪" };

/**
 * The screen after a drill or a conversation: the grade first, what the go
 * did to the lesson's mastery points, what was missed and what it cost,
 * then the longer term in one muted row.
 */
// Spec: SUM-1
export function SessionSummary({
  data,
  onOpenItem,
  onDone,
}: {
  data: SummaryData;
  /** A missed word's or sentence's page. */
  onOpenItem: (itemId: string) => void;
  onDone: () => void;
}) {
  const { result, mastery, score } = data;
  const delta = mastery?.delta ?? 0;
  const tone = toneOf(data.grade, data.graded, delta, data.masteredNow);
  const party = celebrates(tone);
  useEffect(() => {
    // Spec: SUM-6
    if (party) playComplete();
    // Once, as the summary appears.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const grade = data.grade === "" ? null : data.grade;
  const next = grade ? nextLine(grade, data.extra, data.nextDueDay, result.stats.day, data.masteredNow) : null;
  const impact = impactLine(data.impact, mastery?.delta ?? null);
  const missed = data.misses.length > 0 || data.toReview.length > 0;

  return (
    <Screen nav={false} header={<div />} stagger>
      {/* Spec: SUM-5, SUM-9 */}
      {tone === "mastered" ? (
        <Confetti fixed pieces={72} rounds={2} />
      ) : tone === "ace" ? (
        <Confetti fixed pieces={56} rounds={2} />
      ) : tone === "pass" ? (
        <Confetti fixed pieces={28} />
      ) : null}

      {tone === "mastered" ? (
        <MasteredMoment title={data.title} grade={grade} score={score} />
      ) : (
        <GradeHeadline tone={tone} grade={grade} title={data.title} extra={data.extra} score={score} />
      )}

      {/* Spec: SUM-3, SUM-4 */}
      {!data.graded ? (
        <Text tone="dim" align="center" as="p">
          {SUMMARY_TEXT.notGraded}
        </Text>
      ) : mastery ? (
        <MasteryCard entry={mastery} tone={tone} next={next} />
      ) : next ? (
        <Text variant="small" tone="dim" align="center" as="p">
          {next}
        </Text>
      ) : null}

      {/* Spec: SUM-7, SUM-8 */}
      {missed ? (
        <Section title="What you missed">
          <Stack gap={3}>
            {impact || data.misses.length > 0 ? (
              <Stack gap={1}>
                {impact ? (
                  <Text variant="small" weight="medium" as="p">
                    {impact}
                  </Text>
                ) : null}
                {data.misses.length > 0 ? (
                  <Text variant="small" tone="dim" as="p">
                    {SUMMARY_TEXT.missedSooner}
                  </Text>
                ) : null}
              </Stack>
            ) : null}
            <List>
              {data.misses.map((miss) => (
                <ListItem
                  key={miss.key}
                  title={miss.prompt}
                  subtitle={
                    <span className="summary-miss">
                      {miss.given ? <s className="summary-miss__given">{miss.given}</s> : null}
                      <span className="summary-miss__right">{miss.expected}</span>
                    </span>
                  }
                  trailing={miss.almost ? <Badge tone="gold">almost</Badge> : undefined}
                  onClick={/^(wd|sn)_/.test(miss.itemId) ? () => onOpenItem(miss.itemId) : undefined}
                />
              ))}
              {data.toReview.map((point) => (
                <ListItem key={point} title={point} />
              ))}
            </List>
          </Stack>
        </Section>
      ) : null}

      {(result.milestones ?? []).map((milestone) => (
        <Banner key={`${milestone.courseId}-${milestone.mastered}`} tone="ok">
          {milestone.mastered
            ? `🏆 You mastered “${milestone.title}” — every lesson in it. Close it on its page to keep it as a trophy.`
            : `🎉 You completed “${milestone.title}”! Next, master it: every pass adds mastery. Practise its lessons now, or let your check-ins bring them back — spaced out, it takes fewer passes.`}
        </Banner>
      ))}

      {result.newlyUnlocked.length > 0 ? (
        <Section title={`New achievement${result.newlyUnlocked.length > 1 ? "s" : ""}`}>
          <List>
            {result.newlyUnlocked.map((achievement) => (
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

      <LongTerm data={data} />

      <Button variant="primary" size="lg" wide onClick={onDone}>
        Done
      </Button>
    </Screen>
  );
}

/** The grade, as the headline: its word, the lesson, the score. */
// Spec: SUM-2
function GradeHeadline({
  tone,
  grade,
  title,
  extra,
  score,
}: {
  tone: Tone;
  grade: Grade | null;
  title: string;
  extra: boolean;
  score?: SummaryData["score"];
}) {
  const emoji = EMOJI[tone];
  return (
    <Stack gap={2} align="center">
      {emoji ? (
        <Text variant="display" className={tone === "ace" ? "celebrate-rise" : "pop"}>
          <span aria-hidden="true">{emoji}</span>
        </Text>
      ) : null}
      <h1 className="summary-grade" data-tone={tone}>
        {grade ? GRADE_WORD[grade] : "Not graded"}
      </h1>
      <Text tone="dim" align="center" as="p">
        {title}
        {extra ? " · extra practice" : ""}
      </Text>
      {score && score.total > 0 ? (
        <Text weight="medium" align="center" as="p">
          {scoreText(score.correct, score.almost, score.total)}
          {score.almost > 0 ? ` · ${score.almost} almost` : ""}
        </Text>
      ) : null}
    </Stack>
  );
}

/** A lesson mastered by this go: its own moment, in place of the pass's. */
// Spec: SUM-9
function MasteredMoment({ title, grade, score }: { title: string; grade: Grade | null; score?: SummaryData["score"] }) {
  return (
    <div className="summary-mastered">
      <div className="summary-mastered__burst" aria-hidden="true" />
      <div className="summary-mastered__trophy" aria-hidden="true">
        🏆
      </div>
      <Stack gap={2} align="center">
        <span className="summary-mastered__badge">
          <Badge tone="gold">🏆 Mastered</Badge>
        </span>
        <h1 className="summary-grade" data-tone="mastered">
          {title}
        </h1>
        <Text align="center" as="p">
          {SUMMARY_TEXT.masteredLine}
        </Text>
        {grade ? (
          <Text variant="small" tone="dim" align="center" as="p">
            {GRADE_WORD[grade]}
            {score && score.total > 0 ? ` · ${scoreText(score.correct, score.almost, score.total)}` : ""}
          </Text>
        ) : null}
      </Stack>
    </div>
  );
}

/** The lesson's mastery points: a bar from before the go to after it, the change, and why. */
// Spec: SUM-3, SUM-4
function MasteryCard({ entry, tone, next }: { entry: MasteryEntry; tone: Tone; next: string | null }) {
  const { from, to } = barEnds(entry);
  const reason = masteryReason(entry);
  const counts = tone !== "calm" && tone !== "plain" && entry.delta > 0;
  return (
    <Card className="summary-mastery">
      <Stack gap={2}>
        <Inline justify="between">
          <Text variant="small" tone="dim">
            Mastery points
          </Text>
          <span className="summary-mastery__delta" data-sign={Math.sign(entry.delta)}>
            {counts ? (
              <>
                +<CountUp value={entry.delta} from={0} duration={900} />
              </>
            ) : (
              signed(entry.delta)
            )}
          </span>
        </Inline>
        <div
          className="mastery-bar"
          data-tone={tone === "mastered" ? "gold" : undefined}
          role="progressbar"
          aria-valuenow={to}
          aria-valuemax={MASTERY_FULL}
          aria-label="Mastery points"
        >
          {from > to ? <span className="mastery-bar__lost" style={{ "--pct": from }} /> : null}
          <span className="mastery-bar__fill" style={{ "--pct": to, "--from": from }} />
        </div>
        <Text variant="small" tone="dim">
          {entry.before === entry.after ? `${entry.after}` : `${entry.before} → ${entry.after}`} of {MASTERY_FULL}
          {reason ? ` · ${reason}` : ""}
        </Text>
        {next ? (
          <Text variant="small" tone="dim">
            {next}
          </Text>
        ) : null}
      </Stack>
    </Card>
  );
}

/** The longer term, brief and muted: XP, the streak, and the day when this go finished it. */
// Spec: SUM-10, SUM-11
function LongTerm({ data }: { data: SummaryData }) {
  const { stats, firstCompletionToday } = data.result;
  const bits = [
    `+${data.xp} XP · ${stats.dayXp} today`,
    `${stats.currentStreak}-day streak`,
    data.freezeEarned ? "a streak freeze earned" : null,
    firstCompletionToday ? SUMMARY_TEXT.dayDone : null,
  ].filter((bit): bit is string => bit !== null);
  return (
    <Text variant="small" tone="faint" align="center" as="p" className="summary-longterm">
      {bits.join(" · ")}
    </Text>
  );
}
