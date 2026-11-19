import { useEffect } from "react";

import { DictationDiff, ListenButtons, useSayOnce } from "../components/Listen";
import { SpeakButton } from "../components/ui";
import { Stack, Text } from "../design";
import { speakQuietly } from "../lib/speech";
import { pickedAnswer, TypedField, typedAnswer } from "./shared";
import { defineExercise, type ExerciseProps } from "./types";

/*
 * The ear: write down a sentence as it is said (dictation), and tell two
 * words apart that differ by one sound (sound pairs).
 */

/** Dictation: heard, not seen, until it is checked; then word by word. */
function Dictation(props: ExerciseProps) {
  const { exercise, isChecked, diff } = props;
  // A dialogue line is said in its speaker's voice.
  const line = exercise.context[0];
  const gender = line?.gender ?? "";
  useSayOnce(exercise.pl, gender, exercise.key);
  return (
    <>
      <Text variant="eyebrow">Write what you hear</Text>
      <div className="prompt-card listen-card">
        {line ? (
          <Text variant="caption" tone="faint">
            {line.speaker} says
          </Text>
        ) : null}
        <ListenButtons text={exercise.pl} gender={gender} />
      </div>
      {isChecked && diff.length > 0 ? <DictationDiff words={diff} /> : <TypedField {...props} />}
    </>
  );
}

export const dictation = defineExercise({
  kinds: ["dictation"],
  typed: true,
  // The sentence and its meaning, under the comparison, even when right.
  revealOnRight: true,
  ...typedAnswer,
  render: (props) => <Dictation {...props} />,
});

/** How long after the verdict's chime the other word of the pair is said. */
const OTHER_AFTER_MS = 700;

/**
 * Sound pairs: one of two words is said; which was it? Once answered, the
 * other is said too, so the two are heard side by side, and either can be
 * played again.
 */
function SoundPair(props: ExerciseProps) {
  const { exercise, isChecked, picked, setPicked } = props;
  const other = exercise.options.find((option) => option !== exercise.answer) ?? "";
  useEffect(() => {
    if (!isChecked || !other) return undefined;
    const timer = window.setTimeout(() => speakQuietly(other), OTHER_AFTER_MS);
    return () => window.clearTimeout(timer);
  }, [isChecked, other]);
  const state = (option: string) =>
    !isChecked
      ? picked === option
        ? "picked"
        : "idle"
      : option === exercise.answer
        ? "right"
        : option === picked
          ? "wrong"
          : "idle";
  return (
    <>
      <Text variant="eyebrow">Which did you hear?</Text>
      <div className="prompt-card listen-card">
        <SpeakButton text={exercise.answer} label="Play it again" variant="tinted" />
        <Text variant="caption" tone="faint">
          Listen for {exercise.prompt}
        </Text>
      </div>
      <div className="pair-options">
        {exercise.pairs.map((pair) => (
          <Stack key={pair.pl} gap={1} align="center">
            <button
              className="option pair-option"
              data-state={state(pair.pl)}
              disabled={isChecked}
              onClick={() => setPicked(pair.pl)}
            >
              <span className="pair-option__pl">{pair.pl}</span>
              <span className="pair-option__en">{pair.en}</span>
            </button>
            {isChecked ? <SpeakButton text={pair.pl} size="sm" label={`Hear ${pair.pl}`} /> : null}
          </Stack>
        ))}
      </div>
    </>
  );
}

export const soundPair = defineExercise({
  kinds: ["soundPair"],
  speakOnShow: true,
  ...pickedAnswer,
  render: (props) => <SoundPair {...props} />,
  verdictNote: (exercise) => {
    const said = exercise.pairs.find((p) => p.pl === exercise.answer);
    const other = exercise.pairs.find((p) => p.pl !== exercise.answer);
    if (!said || !other) return null;
    return (
      <Text variant="small" selectable>
        It was <strong>{said.pl}</strong> ({said.en}) — now hear <strong>{other.pl}</strong> ({other.en}).
      </Text>
    );
  },
});
