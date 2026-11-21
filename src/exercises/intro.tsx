import { SlowButton, SpeakButton } from "../components/ui";
import { Inline, Text } from "../design";
import { PictureCard, readingAnswer } from "./shared";
import { defineExercise } from "./types";

/** A new word: its picture if it has one, the word said aloud, its meaning. */
export const intro = defineExercise({
  kinds: ["intro"],
  reading: true,
  speakOnShow: true,
  ...readingAnswer,
  render: ({ exercise }) => (
    <>
      <Text variant="eyebrow">New word</Text>
      <div className="prompt-card">
        <PictureCard exercise={exercise} />
        <Text variant="display" selectable>
          {exercise.pl}
        </Text>
        <Inline gap={2} justify="center">
          <SpeakButton text={exercise.pl} variant="tinted" />
          <SlowButton text={exercise.pl} variant="tinted" />
        </Inline>
        <Text variant="lead" tone="dim" weight="medium">
          {exercise.meaning}
        </Text>
        <Text variant="caption" tone="faint">
          {exercise.hint}
        </Text>
      </div>
    </>
  ),
});

/** A new sentence, said aloud, with its English beneath. */
export const sentenceIntro = defineExercise({
  kinds: ["sentenceIntro"],
  reading: true,
  speakOnShow: true,
  ...readingAnswer,
  render: ({ exercise }) => (
    <>
      <Text variant="eyebrow">New sentence</Text>
      <div className="prompt-card">
        <Text variant="title" selectable>
          {exercise.pl}
        </Text>
        <Inline gap={2} justify="center">
          <SpeakButton text={exercise.pl} variant="tinted" />
          <SlowButton text={exercise.pl} variant="tinted" />
        </Inline>
        <Text tone="dim">{exercise.meaning}</Text>
      </div>
    </>
  ),
});
