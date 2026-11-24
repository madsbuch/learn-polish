import { SentenceTiles, tileText } from "../components/SentenceTiles";
import { Card, Text } from "../design";
import type { Exercise, ExerciseKind } from "../lib/schemas";
import { defineExercise, type AnswerState, type ExerciseProps } from "./types";

/** The sentence as built so far: each tile as it reads now (its ending, if picked), joined by spaces. */
export function builtSentence(exercise: Exercise, { built, forms }: AnswerState) {
  return built.map((i) => tileText(exercise, forms, i)).join(" ");
}

/** The English, and the tiles to build its Polish from. */
function Tiles({ eyebrow, exercise, built, setBuilt, forms, setForms, isChecked }: ExerciseProps & { eyebrow: string }) {
  return (
    <>
      <Text variant="eyebrow">{eyebrow}</Text>
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
  );
}

/** A tiles card of one kind, with its own heading. */
function tiles<const K extends ExerciseKind>(kind: K, eyebrow: string) {
  return defineExercise({
    kinds: [kind],
    given: builtSentence,
    canSubmit: (exercise, state) => builtSentence(exercise, state).trim().length > 0,
    render: (props) => <Tiles {...props} eyebrow={eyebrow} />,
  });
}

/*
 * Build the Polish sentence from its words, as tiles: drag them into the
 * line (or tap), reorder them, and pick the ending of a word that needs
 * another form than its dictionary one (see components/SentenceTiles). Three
 * kinds, easiest first, as the lesson's level and the sentence's
 * familiarity allow: every word already in its form, each ending picked
 * from two, each from every form the sentence keeps.
 */
// Spec: DRILL-10
export const wordOrderSet = tiles("wordOrderSet", "Put the Polish in order");
// Spec: DRILL-10
export const wordOrderTwo = tiles("wordOrderTwo", "Build the Polish sentence");
// Spec: DRILL-10
export const wordOrder = tiles("wordOrder", "Build the Polish sentence");
