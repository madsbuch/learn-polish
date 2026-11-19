import { useEffect, useRef, useState } from "react";

import { WordChat } from "../components/WordChat";
import { WrongAnswerReview } from "../components/WrongAnswerReview";
import { CloseIcon } from "../components/icons";
import { Badge, Banner, Button, CountUp, EmptyState, IconButton, Progress, Text, useBump } from "../design";
import { api, errorMessage } from "../lib/api";
import { useBackHandler } from "../lib/back";
import { speakQuietly } from "../lib/speech";
import type { AnswerResult, LessonFinished, Exercise, SessionPlan } from "../lib/types";
import { hapticRight, hapticWrong } from "../lib/haptics";
import { playCorrect, playWrong } from "../lib/sounds";
import { exerciseModule, type AnswerState, type ExerciseProps } from "../exercises";
import { WordIdSchema } from "../lib/types";

/*
 * The practice session: a shell that steps through the plan's exercises. Each
 * kind of exercise lives in its own module under src/exercises (how it is
 * drawn, what its answer is, whether it is only read); this screen keeps what
 * every card shares — the top bar, the verdict, the foot button, submitting
 * answers and moving on.
 */

type Phase = "answering" | "checked";

export function Session({
  plan,
  onFinished,
  onQuit,
}: {
  plan: SessionPlan;
  onFinished: (result: LessonFinished, sessionXp: number) => void;
  onQuit: () => void;
}) {
  useBackHandler(true, onQuit);
  const [index, setIndex] = useState(0);
  const [phase, setPhase] = useState<Phase>("answering");
  const [result, setResult] = useState<AnswerResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [sessionXp, setSessionXp] = useState(0);
  const [correctCount, setCorrectCount] = useState(0);

  // Per-exercise answer state.
  const [picked, setPicked] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const [built, setBuilt] = useState<number[]>([]);
  const [forms, setForms] = useState<Partial<Record<number, string>>>({});
  const [revealAll, setRevealAll] = useState(false);
  const [gapsFilled, setGapsFilled] = useState<(number | null)[]>([]);
  const [memoryTurns, setMemoryTurns] = useState<number | null>(null);

  // The chat about the current word, when open.
  const [talking, setTalking] = useState(false);

  const bodyRef = useRef<HTMLDivElement>(null);
  // The XP badge swells as points come in.
  const xpRef = useBump<HTMLSpanElement>(sessionXp);
  // This go at the lesson, so the XP log keeps its answers together.
  const runKey = useRef(`${plan.lessonId}-${Date.now()}`);
  const inputRef = useRef<HTMLInputElement>(null);

  const exercise: Exercise | undefined = plan.exercises[index];
  const total = plan.exercises.length;
  // The module that draws and scores this kind of exercise.
  const module = exercise ? exerciseModule(exercise.kind) : undefined;

  // Reset every per-card piece of state when we move on.
  useEffect(() => {
    setPhase("answering");
    setResult(null);
    setPicked(null);
    setTyped("");
    setBuilt([]);
    setForms({});
    setGapsFilled([]);
    setMemoryTurns(null);
    setTalking(false);
    setError(null);
    bodyRef.current?.scrollTo({ top: 0 });
    if (module?.typed) {
      // A short delay lets the card render before the keyboard opens.
      const timer = setTimeout(() => inputRef.current?.focus(), 120);
      return () => clearTimeout(timer);
    }
    return undefined;
  }, [index, exercise?.kind]);

  // Pronounce new material as it appears — hearing it first anchors the
  // spelling. Failures stay silent here; the 🔊 button is where they surface.
  useEffect(() => {
    if (exercise && exerciseModule(exercise.kind).speakOnShow) {
      speakQuietly(exercise.pl);
    }
  }, [exercise]);

  const answerState: AnswerState = { picked, typed, built, forms, gapsFilled, memoryTurns };
  const given = exercise && module ? module.given(exercise, answerState) : "";
  const canSubmit = exercise && module ? module.canSubmit(exercise, answerState) : false;

  // Finding the last pair is the answer; no separate Check tap needed.
  useEffect(() => {
    if (module?.submitsItself && memoryTurns !== null && phase === "answering") void check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [memoryTurns]);

  /** Submits the answer; true when it was recorded. */
  async function check(): Promise<boolean> {
    if (!exercise || busy) return false;
    setBusy(true);
    setError(null);
    try {
      const answer = await api.submitAnswer({
        kind: exercise.kind,
        itemKind: exercise.itemKind,
        itemId: exercise.itemId,
        isNew: exercise.isNew,
        expected: exercise.answer,
        given,
        extra: plan.extra,
        lessonId: plan.lessonId,
        run: runKey.current,
      });
      setResult(answer);
      setSessionXp((xp) => xp + answer.xpGained);
      const reading = exerciseModule(exercise.kind).reading;
      if (answer.correct && !reading) setCorrectCount((n) => n + 1);
      setPhase("checked");
      if (!reading) {
        if (answer.correct) {
          playCorrect();
          hapticRight();
        } else {
          playWrong();
          hapticWrong();
        }
      }
      if (exercise.pl && exercise.itemKind !== "dialogue" && exercise.itemKind !== "set") {
        // Let the chime ring before the word is read out — after a miss too,
        // so the right answer is heard, not just seen.
        const pl = exercise.pl;
        window.setTimeout(() => speakQuietly(pl), answer.correct ? 350 : 600);
      }
      return true;
    } catch (cause) {
      setError(errorMessage(cause));
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function advance() {
    if (index + 1 < total) {
      setIndex(index + 1);
      return;
    }
    setBusy(true);
    try {
      const scored = plan.exercises.filter((e) => !exerciseModule(e.kind).reading).length;
      onFinished(await api.lessonFinish(plan.lessonId, correctCount, scored), sessionXp);
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  }

  if (!exercise || !module) {
    return (
      <div className="center-screen">
        <EmptyState
          icon="🎉"
          title="Nothing to practise right now"
          action={
            <Button variant="primary" onClick={onQuit}>
              Back
            </Button>
          }
        />
      </div>
    );
  }

  const isChecked = phase === "checked";
  const reading = module.reading ?? false;

  const exerciseProps: ExerciseProps = {
    ...answerState,
    exercise,
    plan,
    isChecked,
    diff: result?.diff ?? [],
    setPicked,
    setTyped,
    setBuilt,
    setForms,
    setGapsFilled,
    onMemoryComplete: setMemoryTurns,
    revealAll,
    onToggleReveal: () => setRevealAll((v) => !v),
    inputRef,
    onEnter: () => {
      if (isChecked) void advance();
      else if (canSubmit) void check();
    },
  };

  return (
    <div className="session">
      <div className="session-top">
        <IconButton icon={<CloseIcon />} label="Leave practice" onClick={onQuit} />
        <div className="grow">
          <Progress value={index} max={total} />
        </div>
        <span className="bump-target" ref={xpRef}>
          <Badge tone="gold">
            +<CountUp value={sessionXp} duration={400} /> XP
          </Badge>
        </span>
      </div>

      <div className="session-body exercise-in" key={exercise.key} ref={bodyRef}>
        {error ? <Banner tone="error">{error}</Banner> : null}

        {module.render(exerciseProps)}

        {isChecked && result && !reading ? (
          <div className="verdict pop" data-correct={result.correct}>
            <Text weight="bold">
              {result.correct ? "Dobrze! ✓" : "Not quite"}
              {result.xpGained > 0 ? `  +${result.xpGained} XP` : ""}
            </Text>
            {module.verdictNote?.(exercise, answerState)}
            {/* Always show both sides of the pair, whichever direction was asked. */}
            {!result.correct && (exercise.itemKind === "word" || exercise.itemKind === "sentence") ? (
              <WrongAnswerReview
                pl={exercise.pl}
                meaning={exercise.meaning}
                wordId={exercise.itemKind === "word" ? (WordIdSchema.safeParse(exercise.itemId).data ?? null) : null}
              />
            ) : exercise.itemKind !== "set" && (!result.correct || module.revealOnRight) ? (
              <Text variant="small" selectable>
                {exercise.itemKind === "dialogue" ? exercise.answer : `${exercise.pl} — ${exercise.meaning}`}
              </Text>
            ) : null}
            {exercise.itemKind !== "dialogue" && exercise.itemKind !== "set" ? (
              <Button size="sm" icon={<span aria-hidden="true">✨</span>} onClick={() => setTalking(true)}>
                Talk about it
              </Button>
            ) : null}
            {talking ? (
              <WordChat
                itemKind={exercise.itemKind}
                itemId={exercise.itemId}
                title={exercise.pl}
                onClose={() => setTalking(false)}
              />
            ) : null}
          </div>
        ) : null}
      </div>

      <div className="session-foot">
        {isChecked && !reading ? (
          <Button variant="success" size="lg" wide loading={busy} onClick={advance}>
            {index + 1 < total ? "Continue" : "Finish"}
          </Button>
        ) : (
          <Button
            variant="primary"
            size="lg"
            wide
            loading={busy}
            disabled={!canSubmit || module.submitsItself}
            onClick={async () => {
              // Reading cards have nothing to check: record them and move on.
              if ((await check()) && reading) await advance();
            }}
          >
            {reading ? "Got it" : (module.checkLabel ?? "Check")}
          </Button>
        )}
        <Text variant="caption" tone="faint" align="center" as="div">
          {index + 1} of {total} · {correctCount} correct
        </Text>
      </div>
    </div>
  );
}
