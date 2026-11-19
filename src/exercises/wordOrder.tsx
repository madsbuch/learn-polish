import { SentenceTiles, tileText } from "../components/SentenceTiles";
import { Card, Text } from "../design";
import type { Exercise } from "../lib/types";
import { defineExercise, type AnswerState } from "./types";

/** The sentence as built so far: each tile as it reads now (its ending, if picked), joined by spaces. */
function builtSentence(exercise: Exercise, { built, forms }: AnswerState) {
  return built.map((i) => tileText(exercise, forms, i)).join(" ");
}

/**
 * Build the Polish sentence from its words, as tiles: drag them into the
 * line (or tap), reorder them, and pick the ending of a word that needs
 * another form than its dictionary one (see components/SentenceTiles).
 */
export const wordOrder = defineExercise({
  kinds: ["wordOrder"],
  given: builtSentence,
  canSubmit: (exercise, state) => builtSentence(exercise, state).trim().length > 0,
  render: ({ exercise, built, setBuilt, forms, setForms, isChecked }) => (
    <>
      <Text variant="eyebrow">Build the Polish sentence</Text>
      <Card>
        <Text variant="lead" weight="medium" selectable>
          {exercise.prompt}
        </Text>
      </Card>
      <SentenceTiles
        exercise={exercise}
        built={built}
        forms={forms}
        checked={isChecked}
        onBuilt={setBuilt}
        onForms={setForms}
      />
    </>
  ),
});
