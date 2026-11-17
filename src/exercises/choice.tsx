import { SpeakButton } from "../components/ui";
import { Card, Stack, Text } from "../design";
import { Options, pickedAnswer } from "./shared";
import { defineExercise } from "./types";

/**
 * Pick the translation. "recognise" shows the Polish (with its sound) and asks
 * for the English; "produce" goes the other way.
 */
export const recogniseProduce = defineExercise({
  kinds: ["recognise", "produce"],
  ...pickedAnswer,
  render: (props) => {
    const { exercise } = props;
    const recognise = exercise.kind === "recognise";
    return (
      <>
        <Text variant="eyebrow">{recognise ? "What does this mean?" : "How do you say this?"}</Text>
        <div className="prompt-card">
          <Text variant="display" selectable>
            {exercise.prompt}
          </Text>
          {recognise ? <SpeakButton text={exercise.prompt} variant="tinted" /> : null}
          <Text variant="caption" tone="faint">
            {exercise.hint}
          </Text>
        </div>
        <Options {...props} />
      </>
    );
  },
});

/** A line of today's conversation, and a choice of what is said next. */
export const dialogueGap = defineExercise({
  kinds: ["dialogueGap"],
  ...pickedAnswer,
  render: (props) => (
    <>
      <Text variant="eyebrow">What comes next?</Text>
      <Card>
        <Stack gap={1}>
          <Text variant="caption" tone="faint">
            {props.exercise.hint}
          </Text>
          <Text variant="lead" weight="medium" selectable>
            {props.exercise.prompt}
          </Text>
        </Stack>
      </Card>
      <Options {...props} />
    </>
  ),
});
