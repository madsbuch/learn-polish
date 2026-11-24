import { useEffect, useRef, useState, type CSSProperties, type MouseEvent, type ReactNode } from "react";

import { fileUrl } from "../lib/files";
import type { SceneCard, SceneSpot } from "../lib/schemas";

/*
 * A scene lesson's picture with its things on it. The picture keeps its own
 * shape (from its size, before it has loaded, so nothing jumps) and fits the
 * room it is given: the full width on a phone, as large as the window's
 * height allows on a big screen. Each thing is a box in fractions of the
 * picture; a ring can be drawn around it — faint (tap me), lit (this one),
 * picked, right (a glow), wrong (a soft shake) or shown (the answer).
 *
 * Taps are read off the picture as a whole, not off each ring: the smallest
 * box under the finger wins (a cup on a table is the cup), and a tap just
 * beside a small thing still finds it. Rings change colour and shadow only;
 * the one-off entrances and the shake end untransformed (DESIGN.md → Motion).
 */

export type RingState = "faint" | "lit" | "picked" | "right" | "wrong" | "shown";

/** Room kept for the rest of the card below and above the picture. */
export type SceneFit = "explore" | "find" | "choose" | "where" | "page";

/** How far from a thing (in fractions of the picture) a tap still finds it. */
const NEAR = 0.06;

/** The thing at a point of the picture: the smallest box holding it, or the nearest one close by. */
export function spotAt(spots: SceneSpot[], x: number, y: number): SceneSpot | null {
  const pad = 0.01;
  const holding = spots
    .filter((s) => x >= s.x - pad && x <= s.x + s.w + pad && y >= s.y - pad && y <= s.y + s.h + pad)
    .sort((a, b) => a.w * a.h - b.w * b.h);
  if (holding[0]) return holding[0];
  let best: SceneSpot | null = null;
  let bestDistance = NEAR;
  for (const s of spots) {
    // Distance to the box's edge.
    const dx = Math.max(s.x - x, 0, x - (s.x + s.w));
    const dy = Math.max(s.y - y, 0, y - (s.y + s.h));
    const distance = Math.hypot(dx, dy);
    if (distance < bestDistance) {
      best = s;
      bestDistance = distance;
    }
  }
  return best;
}

/** A missed tap: where it landed, and a key so the same spot can be missed twice. */
export interface Miss {
  x: number;
  y: number;
  key: number;
}

export function SceneView({
  card,
  fit,
  ring,
  label,
  onTap,
  miss,
  disabled = false,
  description,
}: {
  card: SceneCard;
  fit: SceneFit;
  /** The ring around a thing, or none. */
  ring: (spot: SceneSpot) => RingState | null;
  /** A label above a thing, or none. */
  label?: (spot: SceneSpot) => ReactNode;
  /** A tap: the thing tapped (or null), and where, in fractions of the picture. */
  onTap?: (spot: SceneSpot | null, x: number, y: number) => void;
  miss?: Miss | null;
  disabled?: boolean;
  /** What the picture shows, for screen readers. */
  description?: string;
}) {
  const frame = useRef<HTMLDivElement>(null);
  const [loaded, setLoaded] = useState(false);
  const interactive = Boolean(onTap) && !disabled;
  // Its shape before it has loaded, so nothing moves when it does.
  const width = card.width > 0 ? card.width : 4;
  const height = card.height > 0 ? card.height : 3;
  const shape: CSSProperties & Record<"--scene-ratio" | "--scene-aspect", string> = {
    "--scene-ratio": `${width} / ${height}`,
    "--scene-aspect": String(width / height),
  };

  function tap(event: MouseEvent<HTMLDivElement>) {
    if (!interactive || !frame.current) return;
    const box = frame.current.getBoundingClientRect();
    const x = (event.clientX - box.left) / box.width;
    const y = (event.clientY - box.top) / box.height;
    onTap?.(spotAt(card.spots, x, y), x, y);
  }

  return (
    <div className="scene" data-fit={fit} style={shape}>
      <div
        ref={frame}
        className="scene-frame"
        data-loaded={loaded}
        data-interactive={interactive}
        onClick={tap}
        role="group"
        aria-label={description ?? "The scene"}
      >
        <img src={fileUrl(card.image)} alt="" draggable={false} onLoad={() => setLoaded(true)} />
        {card.spots.map((spot) => {
          const state = ring(spot);
          const text = label?.(spot);
          return (
            <Spot key={spot.wordId} spot={spot} state={state} interactive={interactive} onTap={onTap}>
              {text ? <span className={labelClass(spot)}>{text}</span> : null}
            </Spot>
          );
        })}
        {miss ? (
          <span
            key={miss.key}
            className="scene-miss"
            style={{ left: `${miss.x * 100}%`, top: `${miss.y * 100}%` }}
            aria-hidden="true"
          />
        ) : null}
      </div>
    </div>
  );
}

/**
 * Where a thing's label goes: above it, or under it at the very top; and
 * held to the picture's side near an edge, so it stays on the screen.
 */
function labelClass(spot: SceneSpot): string {
  const centre = spot.x + spot.w / 2;
  return [
    "scene-label pop",
    spot.y < 0.14 ? "scene-label--below" : "",
    centre < 0.2 ? "scene-label--start" : centre > 0.8 ? "scene-label--end" : "",
  ]
    .filter(Boolean)
    .join(" ");
}

/**
 * One thing's ring. Also a button for the keyboard (taps are read off the
 * picture, so the pointer passes through it).
 */
function Spot({
  spot,
  state,
  interactive,
  onTap,
  children,
}: {
  spot: SceneSpot;
  state: RingState | null;
  interactive: boolean;
  onTap?: (spot: SceneSpot | null, x: number, y: number) => void;
  children: ReactNode;
}) {
  // A wrong ring shakes once each time it turns wrong.
  const [shakes, setShakes] = useState(0);
  const was = useRef(state);
  useEffect(() => {
    if (state === "wrong" && was.current !== "wrong") setShakes((n) => n + 1);
    was.current = state;
  }, [state]);
  const place = {
    left: `${spot.x * 100}%`,
    top: `${spot.y * 100}%`,
    width: `${spot.w * 100}%`,
    height: `${spot.h * 100}%`,
  };
  return (
    <span className="scene-spot" style={place}>
      <button
        key={shakes}
        type="button"
        className="scene-ring"
        data-state={state ?? "none"}
        tabIndex={interactive ? 0 : -1}
        aria-label={state === "shown" || state === "right" ? spot.pl : "A thing in the picture"}
        onClick={(event) => {
          // Only a keyboard's click lands here: the pointer passes through.
          event.stopPropagation();
          if (interactive) onTap?.(spot, spot.x + spot.w / 2, spot.y + spot.h / 2);
        }}
      />
      {children}
    </span>
  );
}

/** The scene's place while it is being made: its shape, and what is happening. */
export function ScenePlaceholder({ children }: { children: ReactNode }) {
  return (
    <div className="scene" data-fit="page">
      <div className="scene-frame scene-frame--empty">{children}</div>
    </div>
  );
}
