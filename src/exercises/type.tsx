import { Text } from "../design";
import { TypedField, typedAnswer } from "./shared";
import { defineExercise } from "./types";

/** The English, and the Polish to write from memory. */
export const typeWord = defineExercise({
  kinds: ["type"],
  typed: true,
  // The verdict shows the word and its meaning even after a right answer.
  revealOnRight: true,
  ...typedAnswer,
  render: (props) => (
    <>
      <Text variant="eyebrow">Write it in Polish</Text>
      <div className="prompt-card">
        <Text variant="display">{props.exercise.prompt}</Text>
        <Text variant="caption" tone="faint">
          {props.exercise.hint}
        </Text>
      </div>
      <TypedField {...props} />
    </>
  ),
});
