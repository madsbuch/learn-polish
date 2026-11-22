import type { ExerciseKind } from "../lib/types";
import { recogniseProduce, dialogueGap } from "./choice";
import { dialogueRead } from "./dialogueRead";
import { intro, sentenceIntro } from "./intro";
import { gapType } from "./gapType";
import { bankDictation, dictation, soundPair, wordDictation } from "./listen";
import { pictureIntro, picturePick, wordPick } from "./pictures";
import { dialogueSay, sayIt } from "./speak";
import { sceneExplore, sceneFind, sceneName, sceneType, sceneWhere } from "./scene";
import { gapFill, memory } from "./sets";
import { storyEnd, storyPanel, storyQuestion } from "./story";
import { typeWord } from "./type";
import type { ExerciseModule } from "./types";
import { wordOrder, wordOrderSet, wordOrderTwo } from "./wordOrder";

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
  wordOrderSet,
  wordOrderTwo,
  bankDictation,
  wordDictation,
  gapType,
  gapFill,
  memory,
  pictureIntro,
  wordPick,
  picturePick,
  dictation,
  soundPair,
  sceneExplore,
  sceneFind,
  sceneName,
  sceneType,
  sceneWhere,
  storyPanel,
  storyQuestion,
  storyEnd,
  sayIt,
  dialogueSay,
] as const;

/** The kinds no module claims. */
type Unclaimed = Exclude<ExerciseKind, (typeof MODULES)[number]["kinds"][number]>;

/**
 * Indexes the modules by kind. The argument only type-checks when every kind
 * is claimed, so a kind added to `ExerciseKind` without a module is a compile
 * error here that names it.
 */
function byKind(modules: typeof MODULES & ([Unclaimed] extends [never] ? unknown : { unclaimed: Unclaimed })) {
  const registry = new Map<ExerciseKind, ExerciseModule>();
  for (const module of modules) {
    for (const kind of module.kinds) registry.set(kind, module);
  }
  return registry;
}

const REGISTRY = byKind(MODULES);

/** The module that draws and scores an exercise kind (every kind has one: see `byKind`). */
export function exerciseModule(kind: ExerciseKind): ExerciseModule {
  const found = REGISTRY.get(kind);
  if (!found) throw new Error(`No exercise module for "${kind}".`);
  return found;
}
