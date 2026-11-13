import { useEffect, useMemo, useRef, useState } from "react";

import { DialogueToolbar, DialogueView } from "../components/DialogueView";
import { Markdown } from "../components/Markdown";
import { Bar, Banner, SpeakButton, Spinner } from "../components/ui";
import { api, errorMessage } from "../lib/api";
import { speakQuietly } from "../lib/speech";
import { useAiStream } from "../lib/useAiStream";
import type { AnswerResult, Exercise, SessionPlan, SessionResult } from "../lib/types";

type Phase = "answering" | "checked";

export function Session({
  plan,
  onFinished,
  onQuit,
}: {
  plan: SessionPlan;
  onFinished: (result: SessionResult, sessionXp: number) => void;
  onQuit: () => void;
}) {
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

  // The explanation streams in, so it can be read while it is still arriving.
  const explanation = useAiStream();

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
    explanation.reset();
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
      default:
        return exercise.answer;
    }
  }, [exercise, typed, built, picked]);

  const canSubmit = exercise
    ? exercise.kind === "intro" ||
      exercise.kind === "sentenceIntro" ||
      exercise.kind === "dialogueRead" ||
      given.trim().length > 0
    : false;

  async function check() {
    if (!exercise || busy) return;
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
      });
      setResult(answer);
      setSessionXp((xp) => xp + answer.xpGained);
      if (answer.correct) setCorrectCount((n) => n + 1);
      setPhase("checked");
      if (answer.correct && exercise.itemKind !== "dialogue") speakQuietly(exercise.pl);
    } catch (cause) {
      setError(errorMessage(cause));
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
      onFinished(await api.completeSession(plan.topic.id), sessionXp);
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  }

  function explain() {
    if (!exercise || explanation.streaming) return;
    void explanation.run((streamId) =>
      api.aiExplainStream(exercise.itemKind, exercise.itemId, streamId),
    );
  }

  if (!exercise) {
    return (
      <div className="center-screen">
        <div>
          <p>Nothing to practise right now.</p>
          <button className="btn primary" onClick={onQuit}>
            Back
          </button>
        </div>
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

  return (
    <div className="session">
      <div className="session-top">
        <button className="icon-btn" onClick={onQuit} aria-label="Leave practice">
          ✕
        </button>
        <div className="grow">
          <Bar value={index} max={total} />
        </div>
        <span className="chip gold">+{sessionXp} XP</span>
      </div>

      <div className="session-body" ref={bodyRef}>
        {error ? <Banner kind="error">{error}</Banner> : null}

        {exercise.kind === "dialogueRead" ? (
          <div className="fade-in">
            <div className="eyebrow">Today's conversation</div>
            <h1>{exercise.pl}</h1>
            <p className="small">{exercise.hint}</p>
            <DialogueToolbar
              lines={plan.dialogue}
              revealAll={revealAll}
              onToggleReveal={() => setRevealAll((v) => !v)}
            />
            <DialogueView lines={plan.dialogue} revealAll={revealAll} />
          </div>
        ) : null}

        {exercise.kind === "intro" ? (
          <div className="fade-in">
            <div className="eyebrow">New word</div>
            <div className="prompt-card">
              <div className="prompt-word selectable">{exercise.pl}</div>
              <div className="row" style={{ justifyContent: "center", marginBottom: 12 }}>
                <SpeakButton text={exercise.pl} />
              </div>
              <div className="muted" style={{ fontSize: 17, fontWeight: 600 }}>
                {exercise.meaning}
              </div>
              <div className="tiny faint" style={{ marginTop: 8 }}>
                {exercise.hint}
              </div>
            </div>
          </div>
        ) : null}

        {exercise.kind === "sentenceIntro" ? (
          <div className="fade-in">
            <div className="eyebrow">New sentence</div>
            <div className="prompt-card">
              <div className="prompt-sentence selectable">{exercise.pl}</div>
              <div className="row" style={{ justifyContent: "center", margin: "12px 0" }}>
                <SpeakButton text={exercise.pl} />
              </div>
              <div className="muted">{exercise.meaning}</div>
            </div>
          </div>
        ) : null}

        {exercise.kind === "recognise" || exercise.kind === "produce" ? (
          <div className="fade-in">
            <div className="eyebrow">
              {exercise.kind === "recognise" ? "What does this mean?" : "How do you say this?"}
            </div>
            <div className="prompt-card">
              <div className="prompt-word selectable">{exercise.prompt}</div>
              {exercise.kind === "recognise" ? (
                <div className="row" style={{ justifyContent: "center", marginTop: 10 }}>
                  <SpeakButton text={exercise.prompt} />
                </div>
              ) : null}
              <div className="tiny faint" style={{ marginTop: 8 }}>
                {exercise.hint}
              </div>
            </div>
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
          </div>
        ) : null}

        {exercise.kind === "dialogueGap" ? (
          <div className="fade-in">
            <div className="eyebrow">What comes next?</div>
            <div className="card">
              <div className="tiny faint" style={{ marginBottom: 6 }}>
                {exercise.hint}
              </div>
              <div className="selectable" style={{ fontSize: 17, fontWeight: 600 }}>
                {exercise.prompt}
              </div>
            </div>
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
          </div>
        ) : null}

        {exercise.kind === "type" ? (
          <div className="fade-in">
            <div className="eyebrow">Write it in Polish</div>
            <div className="prompt-card">
              <div className="prompt-word">{exercise.prompt}</div>
              <div className="tiny faint" style={{ marginTop: 8 }}>
                {exercise.hint}
              </div>
            </div>
            <input
              ref={inputRef}
              value={typed}
              placeholder="Type here…"
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
            <p className="tiny faint" style={{ marginTop: 8 }}>
              Polish letters are optional — "czesc" counts as "cześć".
            </p>
          </div>
        ) : null}

        {exercise.kind === "wordOrder" ? (
          <div className="fade-in">
            <div className="eyebrow">Build the Polish sentence</div>
            <div className="card">
              <div className="selectable" style={{ fontSize: 17, fontWeight: 600 }}>
                {exercise.prompt}
              </div>
            </div>
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
                  className={`token${built.includes(tokenIndex) ? " used" : ""}`}
                  disabled={isChecked}
                  onClick={() => setBuilt([...built, tokenIndex])}
                >
                  {token}
                </button>
              ))}
            </div>
          </div>
        ) : null}

        {isChecked && result ? (
          <div className={`verdict pop ${result.correct ? "right" : "wrong"}`}>
            <b>
              {result.correct ? "Dobrze! ✓" : "Not quite"}
              {result.xpGained > 0 ? `  +${result.xpGained} XP` : ""}
            </b>
            {/* Always show both sides of the pair, whichever direction was asked. */}
            {!result.correct || exercise.kind === "type" ? (
              <div className="small selectable">
                {exercise.itemKind === "dialogue"
                  ? exercise.answer
                  : `${exercise.pl} — ${exercise.meaning}`}
              </div>
            ) : null}
            {exercise.itemKind !== "dialogue" && !explanation.text && !explanation.streaming ? (
              <button className="btn ghost small" style={{ marginTop: 10 }} onClick={explain}>
                ✨ Explain with AI
              </button>
            ) : null}
            {explanation.streaming && !explanation.text ? (
              <div className="row small faint" style={{ marginTop: 12 }}>
                <Spinner /> Thinking…
              </div>
            ) : null}
            {explanation.text ? (
              <div style={{ marginTop: 12 }}>
                <Markdown text={explanation.text} />
                {explanation.streaming ? <span className="caret" /> : null}
              </div>
            ) : null}
            {explanation.error ? (
              <div className="small" style={{ marginTop: 10, color: "var(--bad)" }}>
                {explanation.error}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>

      <div className="session-foot">
        {isChecked ? (
          <button className="btn success block" disabled={busy} onClick={advance}>
            {busy ? <Spinner /> : index + 1 < total ? "Continue" : "Finish session"}
          </button>
        ) : (
          <button className="btn primary block" disabled={!canSubmit || busy} onClick={check}>
            {busy ? (
              <Spinner />
            ) : exercise.kind === "dialogueRead" ||
              exercise.kind === "intro" ||
              exercise.kind === "sentenceIntro" ? (
              "Got it"
            ) : (
              "Check"
            )}
          </button>
        )}
        <div className="center tiny faint" style={{ marginTop: 8 }}>
          {index + 1} of {total} · {correctCount} correct
        </div>
      </div>
    </div>
  );
}
