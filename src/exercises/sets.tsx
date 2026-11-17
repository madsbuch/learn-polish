import { GapFill, Memory } from "../components/SetExercises";
import { Text } from "../design";
import { defineExercise } from "./types";

/** Fill the gaps: a few sentences, each missing a word, and a word bank. */
export const gapFill = defineExercise({
  kinds: ["gapFill"],
  // The backend marks each gap; the answers travel as one "a | b | c" string.
  given: (exercise, { gapsFilled }) =>
    exercise.gaps
      .map((_, i) => {
        const filled = gapsFilled[i];
        return filled == null ? "" : exercise.options[filled];
      })
      .join(" | "),
  // Every gap needs a word before the set can be checked.
  canSubmit: (exercise, { gapsFilled }) => exercise.gaps.every((_, i) => gapsFilled[i] != null),
  render: ({ exercise, gapsFilled, setGapsFilled, isChecked }) => (
    <>
      <Text variant="eyebrow">Fill the gaps</Text>
      <GapFill exercise={exercise} filled={gapsFilled} onChange={setGapsFilled} checked={isChecked} />
    </>
  ),
});

/**
 * Memory: turn cards over two at a time to find the Polish/English pairs.
 * Finding the last pair is the answer, so it submits itself.
 */
export const memory = defineExercise({
  kinds: ["memory"],
  submitsItself: true,
  checkLabel: "Find every pair",
  given: (_exercise, { memoryTurns }) => (memoryTurns === null ? "" : "done"),
  canSubmit: (_exercise, { memoryTurns }) => memoryTurns !== null,
  render: ({ exercise, onMemoryComplete }) => (
    <>
      <Text variant="eyebrow">Memory · find the pairs</Text>
      <Memory exercise={exercise} onComplete={onMemoryComplete} />
    </>
  ),
  // Praise a quick finish: at most two turns more than there are pairs.
  verdictNote: (exercise, { memoryTurns }) =>
    memoryTurns !== null ? (
      <Text variant="small">
        All {exercise.pairs.length} pairs in {memoryTurns} turns
        {memoryTurns <= exercise.pairs.length + 2 ? " — a sharp memory!" : "."}
      </Text>
    ) : null,
});
