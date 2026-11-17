import { Card, Text } from "../design";
import type { Exercise } from "../lib/types";
import { defineExercise, type AnswerState } from "./types";

/** The sentence as built so far, words joined by spaces. */
function builtSentence(exercise: Exercise, { built }: AnswerState) {
  return built.map((i) => exercise.tokens[i]).join(" ");
}

/**
 * Build the Polish sentence from shuffled words: tap a word to add it to the
 * answer, tap it in the answer to take it back.
 */
export const wordOrder = defineExercise({
  kinds: ["wordOrder"],
  given: builtSentence,
  canSubmit: (exercise, state) => builtSentence(exercise, state).trim().length > 0,
  render: ({ exercise, built, setBuilt, isChecked }) => (
    <>
      <Text variant="eyebrow">Build the Polish sentence</Text>
      <Card>
        <Text variant="lead" weight="medium" selectable>
          {exercise.prompt}
        </Text>
      </Card>
      <div className="answer-slot">
        {built.map((tokenIndex, position) => (
          <button
            key={`${tokenIndex}-${position}`}
            className="token"
            disabled={isChecked}
            onClick={() => setBuilt(built.filter((_, i) => i !== position))}
          >
            {exercise.tokens[tokenIndex]}
          </button>
        ))}
      </div>
      <div className="tokens">
        {exercise.tokens.map((token, tokenIndex) => (
          <button
            key={`${token}-${tokenIndex}`}
            className="token"
            data-used={built.includes(tokenIndex)}
            disabled={isChecked || built.includes(tokenIndex)}
            onClick={() => setBuilt([...built, tokenIndex])}
          >
            {token}
          </button>
        ))}
      </div>
    </>
  ),
});
