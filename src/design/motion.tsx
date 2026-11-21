import { createContext, useContext, useEffect, useMemo, useRef, useState } from "react";

import { hapticCelebrate } from "../lib/haptics";

/*
 * Motion that needs JavaScript: which way a screen comes in, numbers that
 * count up, lists that rise in once, confetti. The rest of the app's motion
 * is CSS (base.css → motion). Everything here does nothing when the system
 * asks for reduced motion. See DESIGN.md → Motion.
 */

const REDUCED = "(prefers-reduced-motion: reduce)";

/** True when the system asks for reduced motion: show the end state at once. */
export function prefersReducedMotion(): boolean {
  return window.matchMedia?.(REDUCED).matches ?? false;
}

// ------------------------------------------------------------ direction

/** How a screen came in: a tab's root, pushed on, slid up, or returned to. */
export type ScreenEnter = "tab" | "push" | "up" | "back";

/** When the learner last went back; screens mounting just after it come in from the left. */
let backAt = Number.NEGATIVE_INFINITY;
const BACK_WINDOW_MS = 600;

/**
 * Says the next screen is a return, not a new one (a back button, a
 * "Done" that goes back to where the learner was). Screen's own back and
 * close buttons call it; call it before anything else that goes back.
 */
export function markBack(): void {
  backAt = performance.now();
}

/** Wraps a back or close handler so it marks the way back first. */
export function backward<A extends unknown[]>(go: (...args: A) => void): (...args: A) => void {
  return (...args) => {
    markBack();
    go(...args);
  };
}

/** How a screen that is mounting now comes in, given what kind it is. */
function enterNow(kind: Exclude<ScreenEnter, "back">): ScreenEnter {
  return performance.now() - backAt < BACK_WINDOW_MS ? "back" : kind;
}

const EnterContext = createContext<ScreenEnter | null>(null);
export const ScreenEnterProvider = EnterContext.Provider;

/** How the screen around this component came in (null outside a Screen). */
export function useScreenEnter(): ScreenEnter | null {
  return useContext(EnterContext);
}

/** For Screen: decided once, when it mounts. */
export function useEnterDirection(kind: Exclude<ScreenEnter, "back">): ScreenEnter {
  const [enter] = useState(() => enterNow(kind));
  // The return has landed: the next screen is a new one again.
  useEffect(() => {
    if (enter === "back") backAt = Number.NEGATIVE_INFINITY;
  }, [enter]);
  return enter;
}

// ------------------------------------------------------------ remembered values

/**
 * The last value shown for a counter or a bar, kept while the app runs, so
 * a screen that is opened again (home after a lesson) moves from what the
 * learner saw last to the new value instead of starting over.
 */
const shownBefore = new Map<string, number>();

export function recall(key: string | undefined): number | undefined {
  return key === undefined ? undefined : shownBefore.get(key);
}

export function remember(key: string | undefined, value: number): void {
  if (key !== undefined) shownBefore.set(key, value);
}

// ------------------------------------------------------------ counting up

/**
 * A number that counts to `value` whenever it changes, easing out. It starts
 * from what `memory` last showed (see recall), or from `from`, or — with
 * neither — at the value itself, so only real changes count. Whole numbers
 * only. Instant under reduced motion.
 */
export function useCountUp(
  value: number,
  { memory, from, duration = 700 }: { memory?: string; from?: number; duration?: number } = {},
): number {
  const [start] = useState(() => (prefersReducedMotion() ? value : (recall(memory) ?? from ?? value)));
  const [shown, setShown] = useState(start);
  const current = useRef(start);
  useEffect(() => {
    remember(memory, value);
    const begin = current.current;
    if (begin === value || prefersReducedMotion()) {
      current.current = value;
      setShown(value);
      return undefined;
    }
    const t0 = performance.now();
    let frame = 0;
    const step = (now: number) => {
      const t = Math.min(1, (now - t0) / duration);
      const eased = 1 - (1 - t) ** 3;
      current.current = begin + (value - begin) * eased;
      setShown(t >= 1 ? value : Math.round(current.current));
      if (t < 1) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [value, memory, duration]);
  return shown;
}

/** A number that counts up when it changes (see useCountUp). */
export function CountUp({
  value,
  memory,
  from,
  duration,
  format,
}: {
  value: number;
  /** A name to remember it by between visits, e.g. "xp:today". */
  memory?: string;
  /** Where it starts the first time, e.g. 0 for a result just earned. */
  from?: number;
  duration?: number;
  format?: (value: number) => string;
}) {
  const shown = useCountUp(value, { memory, from, duration });
  return <>{format ? format(shown) : shown}</>;
}

/**
 * A ref for an element that swells for a moment whenever `value` changes
 * after it first appears (the XP badge as points come in). Uses the Web
 * Animations API, so nothing re-renders and nothing is left transformed.
 */
export function useBump<T extends HTMLElement>(value: unknown) {
  const ref = useRef<T>(null);
  const before = useRef(value);
  useEffect(() => {
    if (Object.is(before.current, value)) return;
    before.current = value;
    if (prefersReducedMotion()) return;
    ref.current?.animate?.(
      [{ transform: "scale(1)" }, { transform: "scale(1.18)" }, { transform: "scale(1)" }],
      { duration: 360, easing: "cubic-bezier(0.34, 1.36, 0.64, 1)" },
    );
  }, [value]);
  return ref;
}

// ------------------------------------------------------------ staggered entry

/** Long enough for the eighth row's delay and its rise to finish. */
const STAGGER_WINDOW_MS = 900;

/**
 * The class that makes a container's children rise in one after another —
 * only as the screen first shows them (`ready`: once their data is there),
 * never when the screen is returned to, and never again afterwards: the
 * class is dropped once they have risen, so rows added or moved by a sync
 * later simply appear.
 */
export function useStagger(ready = true): string | undefined {
  const enter = useScreenEnter();
  const [on, setOn] = useState(() => enter !== "back" && !prefersReducedMotion());
  useEffect(() => {
    if (!on || !ready) return undefined;
    const timer = window.setTimeout(() => setOn(false), STAGGER_WINDOW_MS);
    return () => window.clearTimeout(timer);
  }, [on, ready]);
  return on ? "stagger" : undefined;
}

// ------------------------------------------------------------ confetti

const CONFETTI_COLOURS = ["var(--gold)", "var(--brand)", "var(--mint)", "var(--sky)", "var(--crimson)"];

/**
 * Confetti falling over its container (`fixed`: over the whole window).
 * For the few moments worth a party: a course closed, the day done, a new
 * level. Nothing under reduced motion.
 */
export function Confetti({
  pieces = 48,
  rounds = 1,
  fixed,
}: {
  pieces?: number;
  /** How many times each piece falls. */
  rounds?: number;
  fixed?: boolean;
}) {
  const [still] = useState(prefersReducedMotion);
  // A party is felt too, on a phone — reduced motion or not: a buzz isn't
  // motion, and has its own switch (lib/haptics.ts).
  useEffect(hapticCelebrate, []);
  const bits = useMemo(
    () =>
      Array.from({ length: pieces }, (_, i) => ({
        x: `${(i * 37) % 100}%`,
        delay: `${((i * 53) % 90) / 100}s`,
        spin: `${(i * 71) % 360}deg`,
        colour: CONFETTI_COLOURS[i % CONFETTI_COLOURS.length],
      })),
    [pieces],
  );
  if (still) return null;
  return (
    <div className="confetti" data-fixed={fixed || undefined} aria-hidden="true">
      {bits.map((bit, i) => (
        <i
          key={i}
          style={{
            "--x": bit.x,
            "--delay": bit.delay,
            "--spin": bit.spin,
            "--colour": bit.colour,
            "--rounds": rounds,
          }}
        />
      ))}
    </div>
  );
}
