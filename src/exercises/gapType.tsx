import { Card, Stack, Text } from "../design";
import { TypedField, typedAnswer } from "./shared";
import { defineExercise } from "./types";

/**
 * One sentence with a word missing, the word typed: the English above it,
 * the sentence with its gap, and the box. Checked like a typed word.
 */
// Spec: DRILL-14
export const gapType = defineExercise({
  kinds: ["gapType"],
  typed: true,
  revealOnRight: true,
  ...typedAnswer,
  render: (props) => {
    const gap = props.exercise.gaps[0];
    return (
      <>
        <Text variant="eyebrow">Type the missing word</Text>
        <Card>
          <Stack gap={1}>
            <Text variant="small" tone="faint">
              {props.exercise.prompt}
            </Text>
            {gap ? (
              <div className="gap-line selectable">
                {gap.before}
                <span className="gap-slot" data-state={props.isChecked ? "filled" : "active"}>
                  {props.typed.trim() || "\u00a0"}
                </span>
                {gap.after}
              </div>
            ) : null}
          </Stack>
        </Card>
        <TypedField {...props} />
      </>
    );
  },
});
