import { useEffect, useMemo, useRef, useState } from "react";

import { DialogueToolbar, DialogueView } from "../components/DialogueView";
import { WordChat } from "../components/WordChat";
import { GapFill, Memory } from "../components/SetExercises";
import { CloseIcon } from "../components/icons";
import { SpeakButton } from "../components/ui";
import {
  Badge,
  Banner,
  Button,
  Card,
  EmptyState,
  Field,
  IconButton,
  Progress,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { useBackHandler } from "../lib/back";
import { speakQuietly } from "../lib/speech";
import type { AnswerResult, BlockFinished, Exercise, SessionPlan } from "../lib/types";
import { playCorrect, playWrong } from "../lib/sounds";

/** Cards that only show something; they are not scored. */
const READING_KINDS = new Set(["dialogueRead", "intro", "sentenceIntro"]);

type Phase = "answering" | "checked";

export function Session({
  plan,
  onFinished,
  onQuit,
}: {
  plan: SessionPlan;
  onFinished: (result: BlockFinished, sessionXp: number) => void;
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
  const [revealAll, setRevealAll] = useState(false);
  const [gapsFilled, setGapsFilled] = useState<(number | null)[]>([]);
  const [memoryTurns, setMemoryTurns] = useState<number | null>(null);

  // The chat about the current word, when open.
  const [talking, setTalking] = useState(false);

  const bodyRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const exercise: Exercise | undefined = plan.exercises[index];
  const total = plan.exercises.length;

  // Reset every per-card piece of state when we move on.
  useEffect(() => {
    setPhase("answering");
    setResult(null);
    setPicked(null);
    setTyped("");
    setBuilt([]);
    setGapsFilled([]);
    setMemoryTurns(null);
    setTalking(false);
    setError(null);
    bodyRef.current?.scrollTo({ top: 0 });
    if (exercise?.kind === "type") {
      // A short delay lets the card render before the keyboard opens.
      const timer = setTimeout(() => inputRef.current?.focus(), 120);
      return () => clearTimeout(timer);
    }
    return undefined;
  }, [index, exercise?.kind]);

  // Pronounce new material as it appears — hearing it first anchors the
  // spelling. Failures stay silent here; the 🔊 button is where they surface.
  useEffect(() => {
    if (exercise && (exercise.kind === "intro" || exercise.kind === "sentenceIntro")) {
      speakQuietly(exercise.pl);
    }
  }, [exercise]);

  const given = useMemo(() => {
    if (!exercise) return "";
    switch (exercise.kind) {
      case "type":
        return typed;
      case "wordOrder":
        return built.map((i) => exercise.tokens[i]).join(" ");
      case "recognise":
      case "produce":
      case "dialogueGap":
        return picked ?? "";
      case "gapFill":
        return exercise.gaps
          .map((_, i) => (gapsFilled[i] == null ? "" : exercise.options[gapsFilled[i]!]))
          .join(" | ");
      case "memory":
        return memoryTurns === null ? "" : "done";
      default:
        return exercise.answer;
    }
  }, [exercise, typed, built, picked, gapsFilled, memoryTurns]);

  const canSubmit = exercise
    ? exercise.kind === "gapFill"
      ? exercise.gaps.every((_, i) => gapsFilled[i] != null)
      : exercise.kind === "intro" ||
        exercise.kind === "sentenceIntro" ||
        exercise.kind === "dialogueRead" ||
        given.trim().length > 0
    : false;

  // Finding the last pair is the answer; no separate Check tap needed.
  useEffect(() => {
    if (exercise?.kind === "memory" && memoryTurns !== null && phase === "answering") void check();
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
      });
      setResult(answer);
      setSessionXp((xp) => xp + answer.xpGained);
      if (answer.correct && !READING_KINDS.has(exercise.kind)) setCorrectCount((n) => n + 1);
      setPhase("checked");
      if (!READING_KINDS.has(exercise.kind)) {
        if (answer.correct) playCorrect();
        else playWrong();
      }
      if (answer.correct && exercise.pl && exercise.itemKind !== "dialogue") {
        // Let the chime ring before the word is read out.
        const pl = exercise.pl;
        window.setTimeout(() => speakQuietly(pl), 350);
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
      const scored = plan.exercises.filter((e) => !READING_KINDS.has(e.kind)).length;
      onFinished(await api.blockFinish(plan.blockId, correctCount, scored), sessionXp);
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  }


  if (!exercise) {
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
  const optionState = (option: string) => {
    if (!isChecked) return picked === option ? "picked" : "idle";
    if (option === exercise.answer) return "right";
    if (option === picked) return "wrong";
    return "idle";
  };

  const options = (
    <div className="options">
      {exercise.options.map((option) => (
        <button
          key={option}
          className="option"
          data-state={optionState(option)}
          disabled={isChecked}
          onClick={() => setPicked(option)}
        >
          {option}
        </button>
      ))}
    </div>
  );

  const reading = READING_KINDS.has(exercise.kind);

  return (
    <div className="session">
      <div className="session-top">
        <IconButton icon={<CloseIcon />} label="Leave practice" onClick={onQuit} />
        <div className="grow">
          <Progress value={index} max={total} />
        </div>
        <Badge tone="gold">+{sessionXp} XP</Badge>
      </div>

      <div className="session-body fade-in" key={exercise.key} ref={bodyRef}>
        {error ? <Banner tone="error">{error}</Banner> : null}

        {exercise.kind === "dialogueRead" ? (
          <>
            <Stack gap={1}>
              <Text variant="eyebrow">Today's conversation</Text>
              <h1>{exercise.pl}</h1>
              <Text variant="small" tone="dim" as="p">
                {exercise.hint}
              </Text>
            </Stack>
            <DialogueToolbar
              lines={plan.dialogue}
              revealAll={revealAll}
              onToggleReveal={() => setRevealAll((v) => !v)}
            />
            <DialogueView lines={plan.dialogue} revealAll={revealAll} />
          </>
        ) : null}

        {exercise.kind === "intro" ? (
          <>
            <Text variant="eyebrow">New word</Text>
            <div className="prompt-card">
              <Text variant="display" selectable>
                {exercise.pl}
              </Text>
              <SpeakButton text={exercise.pl} variant="tinted" />
              <Text variant="lead" tone="dim" weight="medium">
                {exercise.meaning}
              </Text>
              <Text variant="caption" tone="faint">
                {exercise.hint}
              </Text>
            </div>
          </>
        ) : null}

        {exercise.kind === "sentenceIntro" ? (
          <>
            <Text variant="eyebrow">New sentence</Text>
            <div className="prompt-card">
              <Text variant="title" selectable>
                {exercise.pl}
              </Text>
              <SpeakButton text={exercise.pl} variant="tinted" />
              <Text tone="dim">{exercise.meaning}</Text>
            </div>
          </>
        ) : null}

        {exercise.kind === "recognise" || exercise.kind === "produce" ? (
          <>
            <Text variant="eyebrow">
              {exercise.kind === "recognise" ? "What does this mean?" : "How do you say this?"}
            </Text>
            <div className="prompt-card">
              <Text variant="display" selectable>
                {exercise.prompt}
              </Text>
              {exercise.kind === "recognise" ? <SpeakButton text={exercise.prompt} variant="tinted" /> : null}
              <Text variant="caption" tone="faint">
                {exercise.hint}
              </Text>
            </div>
            {options}
          </>
        ) : null}

        {exercise.kind === "dialogueGap" ? (
          <>
            <Text variant="eyebrow">What comes next?</Text>
            <Card>
              <Stack gap={1}>
                <Text variant="caption" tone="faint">
                  {exercise.hint}
                </Text>
                <Text variant="lead" weight="medium" selectable>
                  {exercise.prompt}
                </Text>
              </Stack>
            </Card>
            {options}
          </>
        ) : null}

        {exercise.kind === "type" ? (
          <>
            <Text variant="eyebrow">Write it in Polish</Text>
            <div className="prompt-card">
              <Text variant="display">{exercise.prompt}</Text>
              <Text variant="caption" tone="faint">
                {exercise.hint}
              </Text>
            </div>
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
                    if (isChecked) void advance();
                    else if (canSubmit) void check();
                  }
                }}
              />
            </Field>
          </>
        ) : null}

        {exercise.kind === "wordOrder" ? (
          <>
            <Text variant="eyebrow">Build the Polish sentence</Text>
            <Card>
              <Text variant="lead" weight="medium" selectable>
                {exercise.prompt}
              </Text>
            </Card>
            <div className="answer-slot">
              {built.map((tokenIndex, position) => (
                <button
                  key={`${tokenIndex}-${position}`}
                  className="token"
                  disabled={isChecked}
                  onClick={() => setBuilt(built.filter((_, i) => i !== position))}
                >
                  {exercise.tokens[tokenIndex]}
                </button>
              ))}
            </div>
            <div className="tokens">
              {exercise.tokens.map((token, tokenIndex) => (
                <button
                  key={`${token}-${tokenIndex}`}
                  className="token"
                  data-used={built.includes(tokenIndex)}
                  disabled={isChecked || built.includes(tokenIndex)}
                  onClick={() => setBuilt([...built, tokenIndex])}
                >
                  {token}
                </button>
              ))}
            </div>
          </>
        ) : null}

        {exercise.kind === "gapFill" ? (
          <>
            <Text variant="eyebrow">Fill the gaps</Text>
            <GapFill exercise={exercise} filled={gapsFilled} onChange={setGapsFilled} checked={isChecked} />
          </>
        ) : null}

        {exercise.kind === "memory" ? (
          <>
            <Text variant="eyebrow">Memory · find the pairs</Text>
            <Memory exercise={exercise} onComplete={setMemoryTurns} />
          </>
        ) : null}

        {isChecked && result && !reading ? (
          <div className="verdict pop" data-correct={result.correct}>
            <Text weight="bold">
              {result.correct ? "Dobrze! ✓" : "Not quite"}
              {result.xpGained > 0 ? `  +${result.xpGained} XP` : ""}
            </Text>
            {exercise.kind === "memory" && memoryTurns !== null ? (
              <Text variant="small">
                All {exercise.pairs.length} pairs in {memoryTurns} turns
                {memoryTurns <= exercise.pairs.length + 2 ? " — a sharp memory!" : "."}
              </Text>
            ) : null}
            {/* Always show both sides of the pair, whichever direction was asked. */}
            {exercise.itemKind !== "set" && (!result.correct || exercise.kind === "type") ? (
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
          <Button variant="success" size="lg" block loading={busy} onClick={advance}>
            {index + 1 < total ? "Continue" : "Finish"}
          </Button>
        ) : (
          <Button
            variant="primary"
            size="lg"
            block
            loading={busy}
            disabled={!canSubmit || exercise.kind === "memory"}
            onClick={async () => {
              // Reading cards have nothing to check: record them and move on.
              if ((await check()) && reading) await advance();
            }}
          >
            {reading ? "Got it" : exercise.kind === "memory" ? "Find every pair" : "Check"}
          </Button>
        )}
        <Text variant="caption" tone="faint" align="center" as="div">
          {index + 1} of {total} · {correctCount} correct
        </Text>
      </div>
    </div>
  );
}
