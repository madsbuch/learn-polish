import type { Exercise } from "../lib/schemas";
import { Field } from "../design";
import { fileUrl } from "../lib/files";
import type { AnswerState, ExerciseProps } from "./types";

/** Reading cards: always ready, and the expected answer is what is recorded. */
export const readingAnswer = {
  given: (exercise: Exercise) => exercise.answer,
  canSubmit: () => true,
};

/** Multiple choice: the option tapped is the answer. */
export const pickedAnswer = {
  given: (_exercise: Exercise, state: AnswerState) => state.picked ?? "",
  canSubmit: (_exercise: Exercise, state: AnswerState) => (state.picked ?? "").trim().length > 0,
};

/** Typed answers: whatever is in the box, once it holds more than spaces. */
export const typedAnswer = {
  given: (_exercise: Exercise, state: AnswerState) => state.typed,
  canSubmit: (_exercise: Exercise, state: AnswerState) => state.typed.trim().length > 0,
};

/** How an option button looks: the learner's pick, then right/wrong once checked. */
function optionState({ exercise, picked, isChecked }: ExerciseProps, option: string) {
  if (!isChecked) return picked === option ? "picked" : "idle";
  if (option === exercise.answer) return "right";
  if (option === picked) return "wrong";
  return "idle";
}

/** The options of a multiple-choice card, as words (`two`: short ones, two to a row). */
export function Options(props: ExerciseProps & { two?: boolean }) {
  const { exercise, isChecked, setPicked, two = false } = props;
  return (
    <div className={two ? "options options--two" : "options"}>
      {exercise.options.map((option) => (
        <button
          key={option}
          className="option"
          data-state={optionState(props, option)}
          disabled={isChecked}
          onClick={() => setPicked(option)}
        >
          {option}
        </button>
      ))}
    </div>
  );
}

/** Picture lessons: four pictures to choose from. */
export function PictureOptions(props: ExerciseProps) {
  const { exercise, isChecked, setPicked } = props;
  return (
    <div className="picture-options">
      {exercise.options.map((option, i) => {
        const image = exercise.optionImages[i];
        return (
          <button
            key={option}
            className="picture-option"
            data-state={optionState(props, option)}
            disabled={isChecked}
            aria-label={isChecked ? option : `Picture ${i + 1}`}
            onClick={() => setPicked(option)}
          >
            {image != null ? <img src={fileUrl(image)} alt="" /> : null}
            {isChecked ? <span className="picture-option-word">{option}</span> : null}
          </button>
        );
      })}
    </div>
  );
}

/** The item's picture on the prompt card, when it has one. */
export function PictureCard({ exercise, fill = false }: { exercise: Exercise; fill?: boolean }) {
  if (exercise.image == null) return null;
  // `fill`: the picture is the card itself, as large as fits above the answers.
  return <img className={fill ? "picture-prompt" : "picture-card"} src={fileUrl(exercise.image)} alt="" />;
}

/** The box a typed answer goes in. Enter checks, or moves on once checked. */
export function TypedField({ typed, setTyped, isChecked, inputRef, onEnter }: ExerciseProps) {
  return (
    <Field label="Your answer" hint={'Polish letters are optional — "czesc" counts as "cześć".'}>
      <input
        ref={inputRef}
        value={typed}
        placeholder="Type here"
        autoCapitalize="none"
        autoCorrect="off"
        spellCheck={false}
        disabled={isChecked}
        onChange={(event) => setTyped(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            onEnter();
          }
        }}
      />
    </Field>
  );
}
