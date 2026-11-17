import { useEffect, useRef, useState } from "react";

import { IconButton } from "../design";
import { playbackFinished, speak, speakSlowly, stop } from "../lib/speech";
import { MutedIcon, PlayerIcon, SpeakerIcon, SlowIcon } from "./icons";

/**
 * A play button's states: idle, fetching its audio (a spinner), playing (a
 * stop icon: tapping it stops), or failed (says why).
 */
function usePlayback(play: () => Promise<void>, onPlayed?: () => void) {
  const [busy, setBusy] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(
    () => () => {
      alive.current = false;
    },
    [],
  );

  async function toggle() {
    if (playing) {
      stop();
      setPlaying(false);
      return;
    }
    setFailure(null);
    setBusy(true);
    try {
      await play();
      onPlayed?.();
      if (!alive.current) return;
      setBusy(false);
      setPlaying(true);
      // Until it ends, is stopped, or another clip takes over.
      await playbackFinished();
    } catch (cause) {
      if (alive.current) setFailure(cause instanceof Error ? cause.message : "Could not play audio.");
    } finally {
      if (alive.current) {
        setBusy(false);
        setPlaying(false);
      }
    }
  }

  return { busy, playing, failure, toggle };
}

/**
 * Plays a line of Polish. Always shown — a button that silently vanishes is
 * more confusing than one that explains why it cannot play. Spins while the
 * first play of a line is being made, shows stop while it plays, and says why
 * on failure.
 */
export function SpeakButton({
  text,
  gender,
  label,
  size = "md",
  variant,
  onPlayed,
}: {
  text: string;
  /** "f" or "m" for dialogue lines; omitted elsewhere. */
  gender?: string;
  label?: string;
  size?: "sm" | "md";
  variant?: "tinted";
  /** After it played (its audio is stored by then). */
  onPlayed?: () => void;
}) {
  const { busy, playing, failure, toggle } = usePlayback(() => speak(text, gender), onPlayed);
  return (
    <IconButton
      icon={failure ? <MutedIcon /> : playing ? <PlayerIcon.stop size={16} /> : <SpeakerIcon />}
      label={failure ?? (playing ? "Stop" : (label ?? `Listen to ${text}`))}
      size={size}
      variant={variant}
      loading={busy}
      failed={Boolean(failure)}
      onClick={toggle}
    />
  );
}

/**
 * Plays a line slowly and clearly: its own version, made once and kept —
 * for a sentence that needs hearing properly. Stop while it plays.
 */
export function SlowButton({
  text,
  gender,
  size = "md",
  variant,
}: {
  text: string;
  gender?: string;
  size?: "sm" | "md";
  variant?: "tinted";
}) {
  const { busy, playing, failure, toggle } = usePlayback(() => speakSlowly(text, gender));
  return (
    <IconButton
      icon={failure ? <MutedIcon /> : playing ? <PlayerIcon.stop size={16} /> : <SlowIcon />}
      label={failure ?? (playing ? "Stop" : `Slowly: ${text}`)}
      size={size}
      variant={variant}
      loading={busy}
      failed={Boolean(failure)}
      onClick={toggle}
    />
  );
}
