import { useEffect, useMemo, useRef, useState } from "react";

import { WordChat } from "../components/WordChat";
import { WrongAnswerReview } from "../components/WrongAnswerReview";
import { CloseIcon } from "../components/icons";
import { Badge, Banner, Button, CountUp, EmptyState, IconButton, Progress, Text, useBump } from "../design";
import { api, errorMessage } from "../lib/api";
import { useBackHandler } from "../lib/back";
import { beginCard, endCard, flushLog, logEvent, noteInput, noteReveal, setPracticeContext, takeSignals } from "../lib/learnLog";
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
  const [index, setIndex] = useState(0);
  const [phase, setPhase] = useState<Phase>("answering");
  const [result, setResult] = useState<AnswerResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [sessionXp, setSessionXp] = useState(0);
  const [correctCount, setCorrectCount] = useState(0);
  // Typed answers one letter off: each counts as half right in the score.
  const [almostCount, setAlmostCount] = useState(0);
  // Every answer of this go, right or not, by card.
  const [results, setResults] = useState<Record<string, boolean>>({});

  // Per-exercise answer state.
  const [picked, setPicked] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const [built, setBuilt] = useState<number[]>([]);
  const [forms, setForms] = useState<Partial<Record<number, string>>>({});
  const [revealAll, setRevealAll] = useState(false);
  const [gapsFilled, setGapsFilled] = useState<(number | null)[]>([]);
  const [memoryTurns, setMemoryTurns] = useState<number | null>(null);
  // Bumped by a card that submits itself (onDone).
  const [done, setDone] = useState(0);

  // The chat about the current word, when open.
  const [talking, setTalking] = useState(false);
  // Speaking skipped for the rest of this go (a noisy place): spoken turns
  // are picked instead, and the Say it cards still to come are left out.
  const [speakingOff, setSpeakingOff] = useState(false);
  const [skipped, setSkipped] = useState<ReadonlySet<string>>(new Set());

  const bodyRef = useRef<HTMLDivElement>(null);
  // The XP badge swells as points come in.
  const xpRef = useBump<HTMLSpanElement>(sessionXp);
  // This go at the lesson, so the XP log keeps its answers together.
  const runKey = useRef(`${plan.lessonId}-${Date.now()}`);
  // The learning log: this go started, and where it was left if it ends early.
  useEffect(() => {
    setPracticeContext(plan.context);
    logEvent({ kind: "lesson_started", lessonId: plan.lessonId, run: runKey.current, details: { ofCards: plan.exercises.length } });
    return () => endCard();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const inputRef = useRef<HTMLInputElement>(null);

  const exercises = plan.exercises.filter((e) => !skipped.has(e.key));
  const card: Exercise | undefined = exercises[index];
  // A spoken turn with speaking off is the choice it was made from.
  const exercise: Exercise | undefined = useMemo(
    () => (card && speakingOff && card.kind === "dialogueSay" ? { ...card, kind: "dialogueGap" } : card),
    [card, speakingOff],
  );
  const total = exercises.length;
  /** Leaving before the end: where, for the learning log. */
  function quit() {
    logEvent({
      kind: "lesson_left",
      lessonId: plan.lessonId,
      run: runKey.current,
      details: { atCard: index + 1, ofCards: total, answered: Object.keys(results).length, correct: correctCount },
    });
    endCard();
    flushLog();
    onQuit();
  }
  useBackHandler(true, quit);
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
    setDone(0);
    setTalking(false);
    setError(null);
    bodyRef.current?.scrollTo({ top: 0 });
    if (module?.typed) {
      // A short delay lets the card render before the keyboard opens.
      const timer = setTimeout(() => inputRef.current?.focus(), 120);
      return () => clearTimeout(timer);
    }
    return undefined;
  }, [index, exercise?.key, exercise?.kind]);

  // The learning log's clock for this card (time to first input, to answer).
  const cardKey = exercise?.key;
  useEffect(() => {
    if (cardKey) beginCard(cardKey);
  }, [cardKey, index]);

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

  // Finding the last pair (or the thing in the picture) is the answer; no
  // separate Check tap needed. Run after the render that has the answer in.
  useEffect(() => {
    if (((module?.submitsItself && memoryTurns !== null) || done > 0) && phase === "answering") void check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [memoryTurns, done]);

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
        options: exercise.options,
        prompt: exercise.prompt,
        pl: exercise.pl,
        signals: takeSignals(),
        // Where it is practised from, and exactly which card: the log can
        // rebuild what was on screen.
        context: plan.context,
        card: {
          key: exercise.key,
          position: index + 1,
          ofCards: total,
          hint: exercise.hint,
          tokens: exercise.tokens,
          image: exercise.image,
          meaning: revealAll ? exercise.meaning : "",
        },
      });
      setResult(answer);
      setSessionXp((xp) => xp + answer.xpGained);
      const reading = exerciseModule(exercise.kind).reading;
      if (answer.correct && !reading) setCorrectCount((n) => n + 1);
      if (answer.almost && !reading) setAlmostCount((n) => n + 1);
      if (!reading) setResults((r) => ({ ...r, [exercise.key]: answer.correct }));
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
    await finish(exercises);
  }

  async function finish(played: Exercise[]) {
    setBusy(true);
    endCard();
    flushLog();
    try {
      const scored = played.filter((e) => !exerciseModule(e.kind).reading).length;
      onFinished(await api.lessonFinish(plan.lessonId, correctCount, almostCount, scored, plan.context), sessionXp);
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  }

  /** No more speaking this go: the Say it cards from here on are left out
   *  (unscored), and spoken turns become choices. */
  function skipSpeaking() {
    setSpeakingOff(true);
    const laterCards = exercises.slice(index).filter((e) => e.kind === "sayIt");
    for (const card of laterCards) {
      logEvent({
        kind: "card_skipped",
        itemKind: card.itemKind,
        itemId: card.itemId,
        lessonId: plan.lessonId,
        run: runKey.current,
        details: { reason: "speaking_off", exercise: card.kind },
      });
    }
    const later = laterCards.map((e) => e.key);
    if (later.length === 0) return;
    const left = exercises.filter((e) => !later.includes(e.key));
    // Nothing after it but Say it cards: the lesson is done.
    if (index >= left.length) void finish(left);
    else setSkipped(new Set([...skipped, ...later]));
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

  /** The foot button: check the answer, or move on once checked; a reading card is recorded and left. */
  async function next() {
    if (busy) return;
    if (isChecked && !reading) await advance();
    else if (reading) {
      if (await check()) await advance();
    } else if (canSubmit && !module?.submitsItself) await check();
  }

  const exerciseProps: ExerciseProps = {
    ...answerState,
    exercise,
    plan,
    isChecked,
    diff: result?.diff ?? [],
    // Every input is noted for the learning log: the first touch, picks changed.
    setPicked: (option) => {
      noteInput(option);
      setPicked(option);
    },
    setTyped: (text) => {
      noteInput();
      setTyped(text);
    },
    setBuilt: (next) => {
      noteInput();
      setBuilt(next);
    },
    setForms: (next) => {
      noteInput();
      setForms(next);
    },
    setGapsFilled: (next) => {
      noteInput();
      setGapsFilled(next);
    },
    onMemoryComplete: setMemoryTurns,
    onDone: () => setDone((n) => n + 1),
    onSkipSpeaking: skipSpeaking,
    revealAll,
    onToggleReveal: () => {
      if (!revealAll) noteReveal();
      setRevealAll((v) => !v);
    },
    inputRef,
    onEnter: () => {
      if (isChecked) void advance();
      else if (canSubmit) void check();
    },
    onNext: () => void next(),
    results,
    sessionXp,
  };

  return (
    <div className={module.wide ? "session session--wide" : "session"}>
      <div className="session-top">
        <IconButton icon={<CloseIcon />} label="Leave practice" onClick={quit} />
        <div className="grow">
          <Progress value={index} max={total} />
        </div>
        <span className="bump-target" ref={xpRef}>
          <Badge tone="gold">
            +<CountUp value={sessionXp} duration={400} /> XP
          </Badge>
        </span>
      </div>

      <div className={`session-body ${module.pageTurn ? "page-turn-in" : "exercise-in"}`} key={exercise.key} ref={bodyRef}>
        {error ? <Banner tone="error">{error}</Banner> : null}

        {module.render(exerciseProps)}

        {isChecked && result && !reading ? (
          <div className="verdict pop" data-correct={result.correct} data-almost={result.almost || undefined}>
            <Text weight="bold">
              {result.correct ? "Dobrze! ✓" : result.almost ? "Almost!" : "Not quite"}
              {result.xpGained > 0 ? `  +${result.xpGained} XP` : ""}
            </Text>
            {result.almost ? (
              <Text variant="small" tone="dim">
                One letter off — half XP, and half a point.
              </Text>
            ) : null}
            {module.verdictNote?.(exercise, answerState)}
            {exercise.explanation ? (
              <Text variant="small" tone="dim" selectable>
                {exercise.explanation}
              </Text>
            ) : null}
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
            // Reading cards have nothing to check: record them and move on.
            onClick={() => void next()}
          >
            {reading ? (module.checkLabel ?? "Got it") : (module.checkLabel ?? "Check")}
          </Button>
        )}
        <Text variant="caption" tone="faint" align="center" as="div">
          {index + 1} of {total} · {correctCount} correct{almostCount > 0 ? ` · ${almostCount} almost` : ""}
        </Text>
      </div>
    </div>
  );
}
