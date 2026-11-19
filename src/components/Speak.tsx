import { useEffect, useRef, useState, type RefObject } from "react";

import { Banner, Button, IconButton, Spinner, Stack, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { canScan, openAppSettings } from "../lib/keyshare";
import { MicError, release, startRecording, type Recording, type Take } from "../lib/mic";
import { stop as stopSpeech } from "../lib/speech";
import { toast } from "../lib/toast";
import type { Spoken } from "../lib/types";
import { MicIcon, PlayerIcon, StopIcon } from "./icons";
import { DictationDiff } from "./Listen";
import { SpeakButton } from "./ui";

/*
 * Speaking: the big mic of a speaking card (a Say it card, a spoken dialogue
 * turn, the Say it game) with what was heard, word by word, the sounds to
 * try, and the learner's own take beside the app's voice; and the small mic
 * in the chat boxes that writes what is said into the box. The recording
 * itself is lib/mic.ts; hearing it, speaking.rs.
 */

type MicState = "idle" | "recording" | "hearing";

/**
 * A mic that records a take on one tap and stops on the next (or by itself
 * after a pause), then hands the take to `onTake` while it is heard. The
 * button's ring follows the voice: `levelRef`'s element gets `--level`.
 */
function useMic(onTake: (take: Take) => Promise<void>) {
  const [state, setState] = useState<MicState>("idle");
  const [problem, setProblem] = useState<MicError | null>(null);
  const recording = useRef<Recording | null>(null);
  const levelRef = useRef<HTMLDivElement>(null);
  const alive = useRef(true);
  // A take that stops itself is handed to what the caller wants now.
  const handTake = useRef(onTake);
  handTake.current = onTake;
  useEffect(() => {
    // Set again on every mount: React may mount, unmount and remount.
    alive.current = true;
    return () => {
      alive.current = false;
      recording.current?.cancel();
    };
  }, []);

  async function finish() {
    const current = recording.current;
    if (!current) return;
    recording.current = null;
    levelRef.current?.style.setProperty("--level", "0");
    setState("hearing");
    try {
      const take = await current.stop();
      if (!alive.current) {
        release(take);
        return;
      }
      await handTake.current(take);
    } finally {
      if (alive.current) setState("idle");
    }
  }

  async function toggle() {
    if (state === "hearing") return;
    if (state === "recording") {
      await finish();
      return;
    }
    setProblem(null);
    stopSpeech();
    try {
      recording.current = await startRecording({
        onLevel: (level) => levelRef.current?.style.setProperty("--level", level.toFixed(2)),
        onAutoStop: () => void finish(),
      });
      if (!alive.current) {
        recording.current.cancel();
        return;
      }
      setState("recording");
    } catch (cause) {
      setProblem(cause instanceof MicError ? cause : new MicError("failed", errorMessage(cause)));
    }
  }

  return { state, problem, toggle, levelRef };
}

/** The big round mic: tap to speak, tap to stop; its ring swells with the voice. */
function BigMic({ state, onClick, levelRef, disabled }: { state: MicState; onClick: () => void; levelRef: RefObject<HTMLDivElement | null>; disabled?: boolean }) {
  const label = state === "recording" ? "Stop" : state === "hearing" ? "Listening to you" : "Tap and speak";
  return (
    <div className="big-mic" ref={levelRef} data-state={state}>
      <button className="big-mic__button" aria-label={label} title={label} disabled={disabled || state === "hearing"} onClick={onClick}>
        {state === "hearing" ? <Spinner /> : <span className="icon-swap" key={state}>{state === "recording" ? <StopIcon /> : <MicIcon />}</span>}
      </button>
      <Text variant="caption" tone="dim" align="center" as="div">
        {state === "recording" ? "Listening… tap when you're done" : state === "hearing" ? "Hearing you…" : disabled ? "" : "Tap and speak"}
      </Text>
    </div>
  );
}

/** Plays the learner's own take. */
function TakeButton({ url }: { url: string }) {
  const [playing, setPlaying] = useState(false);
  const audio = useRef<HTMLAudioElement | null>(null);
  useEffect(
    () => () => {
      audio.current?.pause();
    },
    [],
  );
  function toggle() {
    if (playing && audio.current) {
      audio.current.pause();
      setPlaying(false);
      return;
    }
    stopSpeech();
    const player = new Audio(url);
    audio.current = player;
    player.addEventListener("ended", () => setPlaying(false));
    player.addEventListener("pause", () => setPlaying(false));
    setPlaying(true);
    player.play().catch(() => setPlaying(false));
  }
  return (
    <IconButton
      icon={playing ? <PlayerIcon.stop size={16} /> : <PlayerIcon.play size={16} />}
      label={playing ? "Stop" : "Play your recording"}
      variant="tinted"
      onClick={toggle}
    />
  );
}

/** One go at saying it. */
export interface Attempt {
  heard: string;
  spoken: Spoken;
  take: Take;
}

/** How good an attempt was: right first, then the most words heard. */
function rank(attempt: Attempt): number {
  const heard = attempt.spoken.words.filter((w) => w.status === "right" || w.status === "accents").length;
  const extra = attempt.spoken.words.filter((w) => w.status === "extra").length;
  return (attempt.spoken.correct ? 1000 : 0) + heard * 10 - extra;
}

/** The best of the attempts so far (the first, of equals). */
export function bestOf(attempts: Attempt[]): Attempt | null {
  return attempts.reduce<Attempt | null>((best, a) => (best === null || rank(a) > rank(best) ? a : best), null);
}

/**
 * A speaking card's mic and what came of it: say `expected` as often as you
 * like; each go is heard and compared, word by word, with the sounds to try
 * when a word came out as another. `onBest` hears the best go so far (what
 * counts), `onRight` the first go heard right. Locked once the card is
 * checked: the best go stays shown. A refused microphone says how to allow
 * it and offers to go on without speaking (`onSkip`).
 */
export function SpeakPanel({
  expected,
  gender = "",
  locked,
  onBest,
  onRight,
  onSkip,
  skipLabel = "Skip speaking for now",
}: {
  expected: string;
  gender?: string;
  locked: boolean;
  onBest: (best: Attempt) => void;
  onRight?: (attempt: Attempt) => void;
  onSkip: () => void;
  skipLabel?: string;
}) {
  const [attempts, setAttempts] = useState<Attempt[]>([]);
  const [error, setError] = useState<string | null>(null);
  const takes = useRef<Take[]>([]);
  useEffect(
    () => () => {
      for (const take of takes.current) release(take);
    },
    [],
  );

  const mic = useMic(async (take) => {
    takes.current.push(take);
    setError(null);
    try {
      const heard = await api.speechHear(take.audio, take.format, expected);
      const spoken = heard.spoken ?? { correct: false, words: [], tips: [] };
      const attempt: Attempt = { heard: heard.text, spoken, take };
      const all = [...attempts, attempt];
      setAttempts(all);
      const best = bestOf(all);
      if (best) onBest(best);
      // The first go heard right finishes the card.
      if (spoken.correct && !attempts.some((a) => a.spoken.correct)) onRight?.(attempt);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  });

  const latest = attempts[attempts.length - 1];
  const best = bestOf(attempts);
  const shown = locked ? best : latest;
  const nothing = shown && shown.heard.trim() === "";

  if (mic.problem && !locked) {
    return (
      <Stack gap={3}>
        <Banner tone="error">{mic.problem.message}</Banner>
        <div className="speak-actions">
          {mic.problem.problem === "denied" && canScan ? (
            <Button size="sm" onClick={() => void openAppSettings().catch(() => undefined)}>
              Open settings
            </Button>
          ) : null}
          <Button size="sm" onClick={() => void mic.toggle()}>
            Try again
          </Button>
          <Button size="sm" variant="primary" onClick={onSkip}>
            Continue without speaking
          </Button>
        </div>
      </Stack>
    );
  }

  return (
    <Stack gap={3}>
      {!locked ? <BigMic state={mic.state} levelRef={mic.levelRef} onClick={() => void mic.toggle()} /> : null}
      {error ? <Banner tone="error">{error}</Banner> : null}
      {shown ? (
        <div className="speak-result" key={attempts.indexOf(shown)}>
          <Text variant="caption" tone="faint">
            {locked && attempts.length > 1 ? `Your best of ${attempts.length}` : attempts.length > 1 ? `Go ${attempts.length}: we heard` : "We heard"}
          </Text>
          {nothing ? (
            <Text tone="dim">Nothing we could make out — try again, a little closer to the mic.</Text>
          ) : (
            <DictationDiff words={shown.spoken.words} label="What we heard, word by word" />
          )}
          {shown.spoken.tips.map((tip) => (
            <div className="sound-tip" key={tip.label}>
              <Text variant="small">
                <strong>“{tip.word}”</strong> was heard as <strong>“{tip.heard}”</strong> — {tip.advice}.
              </Text>
              {tip.pair ? (
                <div className="sound-tip__pair">
                  <Text variant="caption" tone="dim">
                    Hear the difference:
                  </Text>
                  <SpeakButton text={tip.pair.a} size="sm" label={`Hear ${tip.pair.a}`} />
                  <Text variant="small" weight="medium">
                    {tip.pair.a}
                  </Text>
                  <SpeakButton text={tip.pair.b} size="sm" label={`Hear ${tip.pair.b}`} />
                  <Text variant="small" weight="medium">
                    {tip.pair.b}
                  </Text>
                </div>
              ) : null}
            </div>
          ))}
          <div className="speak-compare">
            <Stack gap={1} align="center">
              <TakeButton url={shown.take.url} />
              <Text variant="caption" tone="faint">
                You
              </Text>
            </Stack>
            <Stack gap={1} align="center">
              <SpeakButton text={expected} gender={gender} variant="tinted" label="Hear it said" />
              <Text variant="caption" tone="faint">
                Polski
              </Text>
            </Stack>
          </div>
          {!locked && !shown.spoken.correct ? (
            <Text variant="caption" tone="dim" align="center" as="div">
              Try again as often as you like — your best go counts.
            </Text>
          ) : null}
        </div>
      ) : null}
      {!locked ? (
        <div className="speak-skip">
          <Button variant="plain" size="sm" onClick={onSkip}>
            {skipLabel}
          </Button>
        </div>
      ) : null}
    </Stack>
  );
}

/**
 * The mic in a chat box: say it, and the words land in the box (after what
 * is there), to read over and change before sending. Any language.
 */
export function DictateButton({ onText, disabled }: { onText: (text: string) => void; disabled?: boolean }) {
  const mic = useMic(async (take) => {
    try {
      const heard = await api.speechHear(take.audio, take.format, null);
      if (heard.text.trim()) onText(heard.text.trim());
      else toast("Nothing we could make out — try again.", "error");
    } catch (cause) {
      toast(errorMessage(cause), "error");
    } finally {
      release(take);
    }
  });
  useEffect(() => {
    if (mic.problem) toast(mic.problem.message, "error");
  }, [mic.problem]);
  return (
    <div className="dictate" ref={mic.levelRef} data-state={mic.state}>
      <IconButton
        icon={mic.state === "recording" ? <StopIcon /> : <MicIcon />}
        label={mic.state === "recording" ? "Stop and write it down" : mic.state === "hearing" ? "Writing down what you said" : "Speak your message"}
        loading={mic.state === "hearing"}
        disabled={disabled}
        keepFocus
        onClick={() => void mic.toggle()}
      />
    </div>
  );
}
