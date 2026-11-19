import { useEffect, useRef, useState, type ReactNode } from "react";

import { Badge, Banner, Button, Field, Stack, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { hapticRight, hapticWrong } from "../lib/haptics";
import { playCorrect, playWrong } from "../lib/sounds";
import { speakQuietly } from "../lib/speech";
import type { DictationChecked, DictationItem, SoundRound } from "../lib/types";
import { DictationDiff, ListenButtons, useSayOnce } from "./Listen";
import { SpeakPanel, type Attempt } from "./Speak";
import { SpeakButton } from "./ui";

/*
 * The quick games for the ear (screens/QuickGames.tsx): Dictation — five
 * sentences met in lessons, said, written down, compared word by word — and
 * Sound pairs — which of two near words was said. Each reports how many
 * were missed when the last is done. And one for the voice: Say it — five
 * sentences met, said aloud and heard (speaking.rs).
 */

/** "3 of 5" and the misses so far, above a game for the ear. */
function Tally({ at, of, misses }: { at: number; of: number; misses: number }) {
  return (
    <div className="game-bar">
      <Badge tone="sky">
        {at} of {of}
      </Badge>
      <Badge tone="neutral">{misses === 0 ? "No misses" : `${misses} missed`}</Badge>
    </div>
  );
}

/** The verdict after one: right or not, popping in as in a lesson. */
function Verdict({ correct, children }: { correct: boolean; children?: ReactNode }) {
  return (
    <div className="verdict pop" data-correct={correct}>
      <Text weight="bold">{correct ? "Dobrze! ✓" : "Not quite"}</Text>
      {children}
    </div>
  );
}

export function DictationGame({
  sentences,
  onStart,
  onDone,
}: {
  sentences: DictationItem[];
  onStart: () => void;
  onDone: (misses: number) => void;
}) {
  const [index, setIndex] = useState(0);
  const [typed, setTyped] = useState("");
  const [checked, setChecked] = useState<DictationChecked | null>(null);
  const [misses, setMisses] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const item = sentences[index];
  useSayOnce(item.pl, "", `${index}`);

  useEffect(() => {
    onStart();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    const timer = window.setTimeout(() => input.current?.focus(), 120);
    return () => window.clearTimeout(timer);
  }, [index]);

  async function check() {
    if (!typed.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.dictationCheck(item.pl, typed);
      setChecked(result);
      if (result.correct) {
        playCorrect();
        hapticRight();
      } else {
        playWrong();
        hapticWrong();
        setMisses((n) => n + 1);
      }
      // Heard once more, now with the words in front of you.
      window.setTimeout(() => speakQuietly(item.pl), result.correct ? 350 : 600);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  function next() {
    if (index + 1 < sentences.length) {
      setIndex(index + 1);
      setTyped("");
      setChecked(null);
    } else {
      onDone(misses);
    }
  }

  const last = index + 1 === sentences.length;
  return (
    <>
      <Tally at={index + 1} of={sentences.length} misses={misses} />
      {error ? <Banner tone="error">{error}</Banner> : null}
      <div key={index} className="exercise-in">
        <Stack gap={4}>
          <Text variant="eyebrow">Write what you hear</Text>
          <div className="prompt-card listen-card">
            <ListenButtons text={item.pl} />
          </div>
          {checked ? (
            <>
              <DictationDiff words={checked.words} />
              <Verdict correct={checked.correct}>
                <Text variant="small" selectable>
                  {item.pl} — {item.meaning}
                </Text>
              </Verdict>
            </>
          ) : (
            <Field label="What you heard" hint="Polish letters are optional: missing ones are pointed out.">
              <input
                ref={input}
                value={typed}
                placeholder="Type here"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
                onChange={(event) => setTyped(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void check();
                  }
                }}
              />
            </Field>
          )}
        </Stack>
      </div>
      {checked ? (
        <Button variant="success" size="lg" wide onClick={next}>
          {last ? "Finish" : "Next"}
        </Button>
      ) : (
        <Button variant="primary" size="lg" wide loading={busy} disabled={!typed.trim()} onClick={() => void check()}>
          Check
        </Button>
      )}
    </>
  );
}

/** How long after the chime the other word of the pair is said. */
const OTHER_AFTER_MS = 700;

export function SoundsGame({
  rounds,
  onStart,
  onDone,
}: {
  rounds: SoundRound[];
  onStart: () => void;
  onDone: (misses: number) => void;
}) {
  const [index, setIndex] = useState(0);
  const [picked, setPicked] = useState<string | null>(null);
  const [misses, setMisses] = useState(0);
  const round = rounds[index];
  useSayOnce(round.said, "", `${index}`);
  const other = round.said === round.a ? round.b : round.a;

  useEffect(() => {
    onStart();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    if (picked === null) return undefined;
    const timer = window.setTimeout(() => speakQuietly(other), OTHER_AFTER_MS);
    return () => window.clearTimeout(timer);
  }, [picked, other]);

  function pick(word: string) {
    if (picked !== null) return;
    setPicked(word);
    if (word === round.said) {
      playCorrect();
      hapticRight();
    } else {
      playWrong();
      hapticWrong();
      setMisses((n) => n + 1);
    }
  }

  function next() {
    if (index + 1 < rounds.length) {
      setIndex(index + 1);
      setPicked(null);
    } else {
      onDone(misses);
    }
  }

  const words: [string, string][] = [
    [round.a, round.aEn],
    [round.b, round.bEn],
  ];
  const state = (word: string) =>
    picked === null ? "idle" : word === round.said ? "right" : word === picked ? "wrong" : "idle";
  const meaning = (word: string) => (word === round.a ? round.aEn : round.bEn);
  return (
    <>
      <Tally at={index + 1} of={rounds.length} misses={misses} />
      <div key={index} className="exercise-in">
        <Stack gap={4}>
          <Text variant="eyebrow">Which did you hear?</Text>
          <div className="prompt-card listen-card">
            <SpeakButton text={round.said} label="Play it again" variant="tinted" />
            <Text variant="caption" tone="faint">
              Listen for {round.label}
            </Text>
          </div>
          <div className="pair-options">
            {words.map(([pl, en]) => (
              <Stack key={pl} gap={1} align="center">
                <button
                  className="option pair-option"
                  data-state={state(pl)}
                  disabled={picked !== null}
                  onClick={() => pick(pl)}
                >
                  <span className="pair-option__pl">{pl}</span>
                  <span className="pair-option__en">{en}</span>
                </button>
                {picked !== null ? <SpeakButton text={pl} size="sm" label={`Hear ${pl}`} /> : null}
              </Stack>
            ))}
          </div>
          {picked !== null ? (
            <Verdict correct={picked === round.said}>
              <Text variant="small" selectable>
                It was <strong>{round.said}</strong> ({meaning(round.said)}) — now hear <strong>{other}</strong> (
                {meaning(other)}).
              </Text>
            </Verdict>
          ) : null}
        </Stack>
      </div>
      {picked !== null ? (
        <Button variant="success" size="lg" wide onClick={next}>
          {index + 1 === rounds.length ? "Finish" : "Next"}
        </Button>
      ) : null}
    </>
  );
}

/**
 * Say it: five sentences met, each shown and said aloud as often as you
 * like; the best go counts, and the first go heard right moves on to the
 * verdict. A sentence skipped (or a microphone refused) is left out of the
 * score, not missed.
 */
export function SayGame({
  sentences,
  onStart,
  onDone,
}: {
  sentences: DictationItem[];
  onStart: () => void;
  onDone: (misses: number, played: number) => void;
}) {
  const [index, setIndex] = useState(0);
  const [best, setBest] = useState<Attempt | null>(null);
  const [done, setDone] = useState(false);
  const [misses, setMisses] = useState(0);
  const [skipped, setSkipped] = useState(0);
  const item = sentences[index];

  useEffect(() => {
    onStart();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function settle(attempt: Attempt | null) {
    if (done || !attempt) return;
    setDone(true);
    if (attempt.spoken.correct) {
      playCorrect();
      hapticRight();
    } else {
      playWrong();
      hapticWrong();
      setMisses((n) => n + 1);
    }
  }

  function next(skippedNow = 0) {
    const left = skipped + skippedNow;
    if (index + 1 < sentences.length) {
      setIndex(index + 1);
      setBest(null);
      setDone(false);
      setSkipped(left);
    } else {
      onDone(misses, sentences.length - left);
    }
  }

  const last = index + 1 === sentences.length;
  return (
    <>
      <Tally at={index + 1} of={sentences.length} misses={misses} />
      <div key={index} className="exercise-in">
        <Stack gap={4}>
          <Text variant="eyebrow">Say it</Text>
          <div className="prompt-card">
            <Text variant="title" weight="bold" selectable>
              {item.pl}
            </Text>
            <SpeakButton text={item.pl} variant="tinted" label="Hear it first" />
            <Text variant="caption" tone="faint">
              {item.meaning}
            </Text>
          </div>
          <SpeakPanel
            expected={item.pl}
            locked={done}
            onBest={setBest}
            onRight={settle}
            onSkip={() => next(1)}
            skipLabel="Skip this one"
          />
          {done && best ? <Verdict correct={best.spoken.correct} /> : null}
        </Stack>
      </div>
      {done ? (
        <Button variant="success" size="lg" wide onClick={() => next()}>
          {last ? "Finish" : "Next"}
        </Button>
      ) : (
        <Button variant="primary" size="lg" wide disabled={!best} onClick={() => settle(best)}>
          Done
        </Button>
      )}
    </>
  );
}
