import { SlowButton, SpeakButton } from "../components/ui";
import { Inline, Text } from "../design";
import { PictureCard, PictureOptions, Options, pickedAnswer, readingAnswer } from "./shared";
import { defineExercise, type ExerciseProps } from "./types";

/*
 * Picture lessons: meet it (picture, sound, word); hear it and pick its
 * picture; read it and pick its picture; see the picture and pick the word;
 * type the word from the picture. The picture carries the meaning; the
 * English is shown with it when the word is new.
 */

/** A new word from its picture, with its sound and its English. */
function PictureIntroCard({ exercise }: ExerciseProps) {
  return (
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
  );
}

export const pictureIntro = defineExercise({
  kinds: ["pictureIntro"],
  reading: true,
  speakOnShow: true,
  ...readingAnswer,
  render: (props) => <PictureIntroCard {...props} />,
});

/** Read the word and pick its picture. */
export const wordPick = defineExercise({
  kinds: ["wordPick"],
  ...pickedAnswer,
  render: (props) => (
    <>
      <Text variant="eyebrow">Which picture is it?</Text>
      <div className="prompt-card">
        <Text variant="display" selectable>
          {props.exercise.prompt}
        </Text>
        <SpeakButton text={props.exercise.prompt} variant="tinted" />
      </div>
      <PictureOptions {...props} />
    </>
  ),
});

/** See the picture and pick the word. */
export const picturePick = defineExercise({
  kinds: ["picturePick"],
  ...pickedAnswer,
  render: (props) => (
    <>
      <Text variant="eyebrow">What is this?</Text>
      <PictureCard exercise={props.exercise} fill />
      <Options {...props} />
    </>
  ),
});
