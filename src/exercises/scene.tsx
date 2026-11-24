import { useEffect, useState } from "react";

import { SceneView, type Miss, type RingState } from "../components/Scene";
import { SpeakButton } from "../components/ui";
import { Inline, Text } from "../design";
import { hapticTick, hapticWrong } from "../lib/haptics";
import { speakQuietly } from "../lib/speech";
import type { SceneCard, SceneSpot } from "../lib/schemas";
import { Options, pickedAnswer, readingAnswer, TypedField, typedAnswer } from "./shared";
import { defineExercise, type ExerciseProps } from "./types";

/*
 * Scene lessons: one picture full of things. Explore it (tap a thing, hear
 * its name, see it); find a thing by its Polish name; name the thing that
 * lights up; say where things are. The picture is the card's centre on every
 * one of them, as large as the window allows (`wide`).
 */

/** Misses before the thing to find is shown. */
const MISSES_ALLOWED = 2;

/** A label: the Polish, and its English quieter beside it. */
function Label({ spot, english = true }: { spot: SceneSpot; english?: boolean }) {
  return (
    <>
      {spot.pl}
      {english ? <small>{spot.en}</small> : null}
    </>
  );
}

/** The card's scene; every scene card has one (scene lessons fall back to word cards without). */
function sceneOf(props: ExerciseProps): SceneCard | null {
  return props.exercise.scene;
}

/** Look around: tap a thing to hear it and see its name. Every thing found once is counted. */
function Explore(props: ExerciseProps) {
  const card = sceneOf(props);
  const [picked, setPicked] = useState<SceneSpot | null>(null);
  const [heard, setHeard] = useState<string[]>([]);
  if (!card) return null;
  const all = card.spots.length;
  return (
    <>
      <div className="scene-progress">
        <Text variant="eyebrow">Explore the scene</Text>
        <Text variant="caption" tone={heard.length === all ? "brand" : "faint"}>
          {heard.length === all ? "All found ✓" : `${heard.length} of ${all}`}
        </Text>
      </div>
      <SceneView
        card={card}
        fit="explore"
        description={props.exercise.prompt}
        ring={(spot) => (picked?.wordId === spot.wordId ? "picked" : heard.includes(spot.wordId) ? null : "faint")}
        label={(spot) => (picked?.wordId === spot.wordId ? <Label spot={spot} /> : null)}
        onTap={(spot) => {
          if (!spot) {
            setPicked(null);
            return;
          }
          hapticTick();
          speakQuietly(spot.pl);
          setPicked(spot);
          setHeard((list) => (list.includes(spot.wordId) ? list : [...list, spot.wordId]));
        }}
      />
      <Text variant="small" tone="dim" align="center" as="p">
        Tap each thing to hear its name.
      </Text>
    </>
  );
}

export const sceneExplore = defineExercise({
  kinds: ["sceneExplore"],
  reading: true,
  wide: true,
  ...readingAnswer,
  render: (props) => <Explore {...props} />,
});

/**
 * "Gdzie jest zegar?" — tap it. A miss shakes where it landed (the thing
 * there, or the spot); the second shows the thing, and the card is answered.
 */
function Find(props: ExerciseProps) {
  const { exercise, isChecked, picked, setPicked, onDone } = props;
  const card = sceneOf(props);
  const [missed, setMissed] = useState<string[]>([]);
  const [miss, setMiss] = useState<Miss | null>(null);
  const [wrong, setWrong] = useState<string | null>(null);
  // The question is heard as the card lands.
  useEffect(() => {
    const timer = window.setTimeout(() => speakQuietly(exercise.prompt), 250);
    return () => window.clearTimeout(timer);
  }, [exercise.key, exercise.prompt]);
  if (!card) return null;
  const target = exercise.itemId;
  const gaveUp = isChecked && picked !== exercise.answer;

  function ring(spot: SceneSpot): RingState | null {
    if (spot.wordId === target && isChecked) return gaveUp ? "shown" : "right";
    if (spot.wordId === wrong) return "wrong";
    return null;
  }

  return (
    <>
      <Text variant="eyebrow">Find it in the picture</Text>
      <Inline gap={2} justify="center">
        <Text variant="headline" selectable>
          {exercise.prompt}
        </Text>
        <SpeakButton text={exercise.prompt} variant="tinted" />
      </Inline>
      <SceneView
        card={card}
        fit="find"
        ring={ring}
        label={(spot) =>
          isChecked && spot.wordId === target ? (
            <Label spot={spot} english={false} />
          ) : spot.wordId === wrong && missed.includes(spot.wordId) ? (
            <Label spot={spot} english={false} />
          ) : null
        }
        miss={miss}
        disabled={isChecked}
        onTap={(spot, x, y) => {
          if (spot?.wordId === target) {
            setPicked(exercise.answer);
            onDone();
            return;
          }
          const misses = missed.length + 1;
          // A thing missed is named as it shakes, so the miss teaches too.
          if (spot) {
            setWrong(spot.wordId);
            speakQuietly(spot.pl);
          } else {
            setWrong(null);
            setMiss({ x, y, key: Date.now() });
          }
          setMissed([...missed, spot?.wordId ?? `nothing-${misses}`]);
          if (misses >= MISSES_ALLOWED) {
            setPicked(spot?.pl ?? "—");
            onDone();
          } else {
            hapticWrong();
          }
        }}
      />
      {!isChecked && missed.length > 0 ? (
        <Text variant="small" tone="dim" align="center" as="p">
          Not that one — one more try.
        </Text>
      ) : null}
    </>
  );
}

export const sceneFind = defineExercise({
  kinds: ["sceneFind"],
  submitsItself: true,
  wide: true,
  checkLabel: "Tap it in the picture",
  ...pickedAnswer,
  render: (props) => <Find {...props} />,
});

/** A thing lights up: what is it? Picked from the scene's words, or typed. */
function Name(props: ExerciseProps) {
  const { exercise, isChecked } = props;
  const card = sceneOf(props);
  if (!card) return null;
  const typed = exercise.kind === "sceneType";
  return (
    <>
      <Inline gap={2} justify="between">
        <Text variant="eyebrow">What is this?</Text>
        {exercise.hint ? (
          <Text variant="caption" tone="faint">
            {exercise.hint}
          </Text>
        ) : null}
      </Inline>
      <SceneView
        card={card}
        fit={typed ? "find" : "choose"}
        ring={(spot) => (spot.lit ? "lit" : null)}
        label={(spot) => (isChecked && spot.lit ? <Label spot={spot} /> : null)}
      />
      {typed ? <TypedField {...props} /> : <Options {...props} two />}
    </>
  );
}

export const sceneName = defineExercise({
  kinds: ["sceneName"],
  wide: true,
  ...pickedAnswer,
  render: (props) => <Name {...props} />,
});

export const sceneType = defineExercise({
  kinds: ["sceneType"],
  typed: true,
  wide: true,
  ...typedAnswer,
  render: (props) => <Name {...props} />,
});

/** "Kot jest pod stołem": true or false, or the place to fill in. Both things are pointed out. */
function Where(props: ExerciseProps) {
  const { exercise, isChecked } = props;
  const card = sceneOf(props);
  if (!card) return null;
  const sentence = !exercise.prompt.includes("…");
  return (
    <>
      <Text variant="eyebrow">{exercise.hint || "Where is it?"}</Text>
      <SceneView
        card={card}
        fit="where"
        ring={(spot) => (spot.lit ? "lit" : null)}
        label={(spot) => (isChecked && spot.lit ? <Label spot={spot} english={false} /> : null)}
      />
      <Inline gap={2} justify="center">
        <Text variant="lead" weight="bold" selectable>
          {exercise.prompt}
        </Text>
        {sentence ? <SpeakButton text={exercise.prompt} size="sm" /> : null}
      </Inline>
      <Options {...props} two />
    </>
  );
}

export const sceneWhere = defineExercise({
  kinds: ["sceneWhere"],
  wide: true,
  ...pickedAnswer,
  render: (props) => <Where {...props} />,
  // The true sentence, whichever was asked, to hear and read.
  verdictNote: (exercise) => (
    <Inline gap={2}>
      <Text variant="small" selectable>
        {exercise.pl} — {exercise.meaning}
      </Text>
      <SpeakButton text={exercise.pl} size="sm" />
    </Inline>
  ),
});
