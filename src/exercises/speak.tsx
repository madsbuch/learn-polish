import { useEffect } from "react";

import { SpeakPanel } from "../components/Speak";
import { SpeakButton } from "../components/ui";
import { Card, Stack, Text } from "../design";
import { speakQuietly } from "../lib/speech";
import { PictureCard } from "./shared";
import { defineExercise, type ExerciseProps } from "./types";

/*
 * Speaking (speaking.rs): say a sentence or word already met (Say it), or
 * say your line in a dialogue (a spoken turn) — the other speaker answers.
 * Go as often as you like; the best go is the answer (kept in `typed`), and
 * the first go heard right checks the card by itself.
 */

/** What both speaking cards submit: the best go, as heard. */
const spokenAnswer = {
  given: (_exercise: unknown, state: { typed: string }) => state.typed,
  canSubmit: (_exercise: unknown, state: { typed: string }) => state.typed.trim().length > 0,
};

function SayIt(props: ExerciseProps) {
  const { exercise, isChecked, setTyped, onDone, onSkipSpeaking } = props;
  return (
    <>
      <Text variant="eyebrow">Say it</Text>
      <div className="prompt-card">
        <PictureCard exercise={exercise} />
        <Text variant="display" selectable>
          {exercise.pl}
        </Text>
        <SpeakButton text={exercise.pl} variant="tinted" label="Hear it first" />
        <Text variant="caption" tone="faint">
          {exercise.meaning}
        </Text>
      </div>
      <SpeakPanel
        expected={exercise.answer}
        locked={isChecked}
        onBest={(best) => setTyped(best.heard)}
        onRight={() => onDone()}
        onSkip={onSkipSpeaking}
      />
    </>
  );
}

export const sayIt = defineExercise({
  kinds: ["sayIt"],
  ...spokenAnswer,
  checkLabel: "Done",
  revealOnRight: true,
  render: (props) => <SayIt {...props} />,
});

/** How long after the verdict's chime the other speaker answers. */
const REPLY_AFTER_MS = 650;

function DialogueSay(props: ExerciseProps) {
  const { exercise, isChecked, setTyped, onDone, onSkipSpeaking } = props;
  // The speaker of the line: the hint is "previous — this one".
  const speaker = exercise.hint.split(" — ").pop() ?? "";
  const gender = exercise.context.find((line) => line.speaker === speaker)?.gender ?? "";
  const reply = exercise.reply;
  useEffect(() => {
    if (!isChecked || !reply) return undefined;
    const timer = window.setTimeout(() => speakQuietly(reply.pl, reply.gender), REPLY_AFTER_MS);
    return () => window.clearTimeout(timer);
  }, [isChecked, reply]);
  return (
    <>
      <Text variant="eyebrow">Your turn: say it</Text>
      <Card>
        <Stack gap={2}>
          {exercise.context.map((line, i) => {
            const last = i === exercise.context.length - 1;
            return (
              <Stack key={`${line.ord}-${i}`} gap={0}>
                <Text variant="caption" tone="faint">
                  {line.speaker}
                </Text>
                <Text variant={last ? "lead" : "body"} weight={last ? "medium" : undefined} tone={last ? undefined : "dim"} selectable>
                  {line.pl}
                </Text>
              </Stack>
            );
          })}
          <div className="say-line">
            <Text variant="caption" tone="faint">
              {speaker ? `You, as ${speaker}` : "You"}
            </Text>
            <div className="say-line__text">
              <Text variant="lead" weight="bold" selectable>
                {exercise.answer}
              </Text>
              <SpeakButton text={exercise.answer} gender={gender} size="sm" label="Hear the line" />
            </div>
            <Text variant="caption" tone="dim">
              {exercise.meaning}
            </Text>
          </div>
          {isChecked && reply ? (
            <Stack gap={0} className="say-reply">
              <Text variant="caption" tone="faint">
                {reply.speaker}
              </Text>
              <div className="say-line__text">
                <Text variant="lead" weight="medium" selectable>
                  {reply.pl}
                </Text>
                <SpeakButton text={reply.pl} gender={reply.gender} size="sm" label="Hear the answer again" />
              </div>
            </Stack>
          ) : null}
        </Stack>
      </Card>
      <SpeakPanel
        expected={exercise.answer}
        gender={gender}
        locked={isChecked}
        onBest={(best) => setTyped(best.heard)}
        onRight={() => onDone()}
        onSkip={onSkipSpeaking}
      />
    </>
  );
}

export const dialogueSay = defineExercise({
  kinds: ["dialogueSay"],
  ...spokenAnswer,
  checkLabel: "Done",
  render: (props) => <DialogueSay {...props} />,
});
