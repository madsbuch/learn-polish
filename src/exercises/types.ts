import type { ReactNode, RefObject } from "react";

import type { DiffWord, Exercise, ExerciseKind, SessionPlan } from "../lib/types";

/**
 * Everything a learner can have put in so far on one card. The session owns
 * this state (it resets it between cards and submits it); each exercise kind
 * reads only the parts it uses.
 */
export interface AnswerState {
  /** Multiple choice: the option tapped. */
  picked: string | null;
  /** Typed answers: the text in the box. */
  typed: string;
  /** Word order: the indices into `exercise.tokens`, in the order placed. */
  built: number[];
  /** Word order: the ending picked on a tile, by its index into `exercise.tokens`. */
  forms: Partial<Record<number, string>>;
  /** Fill the gaps: for each gap, the index of the bank word in it, or null. */
  gapsFilled: (number | null)[];
  /** Memory: the turns it took to find every pair, once they are all found. */
  memoryTurns: number | null;
}

/** What the session hands an exercise's body when it draws it. */
export interface ExerciseProps extends AnswerState {
  exercise: Exercise;
  plan: SessionPlan;
  /** The answer has been submitted: options lock and show right/wrong. */
  isChecked: boolean;
  /** Once checked, a dictation word by word (empty for other kinds). */
  diff: DiffWord[];
  setPicked: (option: string) => void;
  setTyped: (text: string) => void;
  setBuilt: (built: number[]) => void;
  setForms: (forms: Partial<Record<number, string>>) => void;
  setGapsFilled: (filled: (number | null)[]) => void;
  /** Memory: called once, with the turns taken, when the last pair is found. */
  onMemoryComplete: (turns: number) => void;
  /** A card that submits itself (finding a thing in a scene): the answer is
   *  in, check it — after the state set alongside this call has landed. */
  onDone: () => void;
  /** Today's conversation: whether every line's English is showing. */
  revealAll: boolean;
  onToggleReveal: () => void;
  /** The text box of a typed exercise, so the session can focus it. */
  inputRef: RefObject<HTMLInputElement | null>;
  /** Enter in the text box: check the answer, or move on once checked. */
  onEnter: () => void;
}

/**
 * One kind of exercise (or a few that share a body): how it is drawn, what
 * counts as its answer, and how the session should treat it. The session is a
 * shell around these; adding a kind means adding a module, not more branches.
 */
export interface ExerciseModule<K extends ExerciseKind = ExerciseKind> {
  /** The exercise kinds this module draws. */
  kinds: readonly K[];
  /** Only shows something — nothing to check. The button reads "Got it", the
   *  card is recorded and the session moves straight on; it is not scored. */
  reading?: boolean;
  /** The answer is typed: the keyboard opens when the card appears. */
  typed?: boolean;
  /** New material: say `exercise.pl` aloud when the card appears. */
  speakOnShow?: boolean;
  /** The answer is complete the moment the learner finishes (memory's last
   *  pair, a thing found), so it submits itself and the foot button stays
   *  disabled. */
  submitsItself?: boolean;
  /** Wants the room: a scene, as large as the window allows (the session
   *  widens past the reading width on a big screen). */
  wide?: boolean;
  /** Show "pl — meaning" in the verdict even after a right answer. */
  revealOnRight?: boolean;
  /** The foot button's label in place of "Check". */
  checkLabel?: string;
  /** The card's body, between the error banner and the verdict. */
  render: (props: ExerciseProps) => ReactNode;
  /** The answer as submitted to the backend. */
  given: (exercise: Exercise, state: AnswerState) => string;
  /** Whether there is enough of an answer to check. */
  canSubmit: (exercise: Exercise, state: AnswerState) => boolean;
  /** An extra line in the verdict, under "Dobrze!" / "Not quite". */
  verdictNote?: (exercise: Exercise, state: AnswerState) => ReactNode;
}

/**
 * Declares a module, keeping its `kinds` as literal types so the registry can
 * tell at compile time whether every kind is covered.
 */
export function defineExercise<const K extends ExerciseKind>(module: ExerciseModule<K>): ExerciseModule<K> {
  return module;
}
