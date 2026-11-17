import { DialogueToolbar, DialogueView } from "../components/DialogueView";
import { Stack, Text } from "../design";
import { readingAnswer } from "./shared";
import { defineExercise } from "./types";

/** Today's conversation, read through once before it is drilled. */
export const dialogueRead = defineExercise({
  kinds: ["dialogueRead"],
  reading: true,
  ...readingAnswer,
  render: ({ exercise, plan, revealAll, onToggleReveal }) => (
    <>
      <Stack gap={1}>
        <Text variant="eyebrow">Today's conversation</Text>
        <h1>{exercise.pl}</h1>
        <Text variant="small" tone="dim" as="p">
          {exercise.hint}
        </Text>
      </Stack>
      <DialogueToolbar lines={plan.dialogue} revealAll={revealAll} onToggleReveal={onToggleReveal} />
      <DialogueView lines={plan.dialogue} revealAll={revealAll} />
    </>
  ),
});
