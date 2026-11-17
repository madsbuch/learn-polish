import type { ExerciseKind } from "../lib/types";
import { recogniseProduce, dialogueGap } from "./choice";
import { dialogueRead } from "./dialogueRead";
import { intro, sentenceIntro } from "./intro";
import { pictureIntro, picturePick, wordPick } from "./pictures";
import { gapFill, memory } from "./sets";
import { typeWord } from "./type";
import type { ExerciseModule } from "./types";
import { wordOrder } from "./wordOrder";

export type { AnswerState, ExerciseModule, ExerciseProps } from "./types";

/** Every exercise module. Each kind must be claimed by exactly one. */
const MODULES = [
  dialogueRead,
  intro,
  sentenceIntro,
  recogniseProduce,
  dialogueGap,
  typeWord,
  wordOrder,
  gapFill,
  memory,
  pictureIntro,
  wordPick,
  picturePick,
] as const;

/** The kinds no module claims. */
type Unclaimed = Exclude<ExerciseKind, (typeof MODULES)[number]["kinds"][number]>;

/**
 * Indexes the modules by kind. The argument only type-checks when every kind
 * is claimed, so a kind added to `ExerciseKind` without a module is a compile
 * error here that names it.
 */
function byKind(modules: typeof MODULES & ([Unclaimed] extends [never] ? unknown : { unclaimed: Unclaimed })) {
  const registry: Partial<Record<ExerciseKind, ExerciseModule>> = {};
  for (const module of modules) {
    for (const kind of module.kinds) registry[kind] = module;
  }
  return registry as Record<ExerciseKind, ExerciseModule>;
}

/** The module that draws and scores each exercise kind. */
export const EXERCISES: Record<ExerciseKind, ExerciseModule> = byKind(MODULES);
