import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { createPortal } from "react-dom";

import { Button, Text, prefersReducedMotion } from "../design";
import { hapticTick } from "../lib/haptics";
import type { Exercise, TileEnding } from "../lib/types";

/*
 * Sentence tiles: build the Polish sentence from its words. Drag a tile into
 * the sentence line (or tap it: it goes to the end), drag it along the line
 * to move it, out of the line to take it back. A tile whose word needs
 * another ending shows its dictionary form ("kawa") with a dotted line under
 * it; once placed, tap it to pick the ending ("kawę"). Tapping a placed
 * tile without endings takes it back out.
 *
 * Motion: a picked-up tile lifts (it scales up a touch and casts a shadow)
 * and follows the finger above everything; the tiles of the line make room
 * and close up smoothly, and a dropped tile springs into its slot (FLIP: each
 * tile is animated from where it was to where it now is, one-off and ending
 * untransformed, as DESIGN.md's Motion rules ask inside a scroller).
 */

/** Lower case without punctuation, Polish letters kept: how the core compares tiles (tiles.rs). */
export function foldTile(text: string) {
  return text
    .split(/\s+/)
    .map((word) => word.toLowerCase().replace(/[^\p{L}\p{N}]/gu, ""))
    .filter(Boolean)
    .join(" ");
}

/** A tile's text now: the ending picked, else its dictionary form, else the word itself. */
export function tileText(exercise: Exercise, forms: Partial<Record<number, string>>, token: number) {
  return forms[token] ?? endingOf(exercise, token)?.shown ?? exercise.tokens[token] ?? "";
}

function endingOf(exercise: Exercise, token: number): TileEnding | undefined {
  return exercise.endings.find((ending) => ending.token === token);
}

/** A pointer must travel this far (px) before a press becomes a drag. */
const DRAG_START = 6;
/** How far around the line (px) a dragged tile still counts as over it. */
const LINE_REACH = 28;

interface Drag {
  token: number;
  from: "bank" | "line";
  pointerId: number;
  startX: number;
  startY: number;
  x: number;
  y: number;
  /** Where on the tile it was picked up. */
  offsetX: number;
  offsetY: number;
  width: number;
  /** Past DRAG_START: the tile is lifted and follows the pointer. */
  active: boolean;
  /** Where in the line it would land, or null when not over the line. */
  insertAt: number | null;
}

/** A design token's value, read once per use (motion durations and easings). */
function token(name: string, fallback: string) {
  try {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
  } catch {
    return fallback;
  }
}

function seconds(value: string) {
  const n = parseFloat(value);
  return value.endsWith("ms") ? n : n * 1000;
}

export function SentenceTiles({
  exercise,
  built,
  forms,
  checked,
  onBuilt,
  onForms,
}: {
  exercise: Exercise;
  built: number[];
  forms: Partial<Record<number, string>>;
  checked: boolean;
  onBuilt: (built: number[]) => void;
  onForms: (forms: Partial<Record<number, string>>) => void;
}) {
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);
  // The placed tile whose endings are open.
  const [picking, setPicking] = useState<number | null>(null);
  const boardRef = useRef<HTMLDivElement>(null);
  const lineRef = useRef<HTMLDivElement>(null);
  // Each tile's element where it shows now (the line, or the bank while
  // unplaced), and where it was last drawn, relative to the board.
  const tiles = useRef(new Map<string, HTMLElement>());
  const lastRects = useRef(new Map<string, { x: number; y: number }>());
  // A press that became a drag doesn't also count as a tap.
  const dragged = useRef(false);
  const builtRef = useRef(built);
  builtRef.current = built;

  const expected = exercise.answer.split(/\s+/).filter(Boolean);
  const placing = drag?.active ? drag : null;
  const line = placing ? built.filter((t) => t !== placing.token) : built;

  function register(key: string) {
    return (element: HTMLElement | null) => {
      if (element) tiles.current.set(key, element);
      else tiles.current.delete(key);
    };
  }

  // FLIP: after each change, every tile that moved is animated from where it
  // was to where it is now.
  useLayoutEffect(() => {
    const board = boardRef.current;
    if (!board) return;
    const origin = board.getBoundingClientRect();
    const still = prefersReducedMotion();
    const duration = seconds(token("--duration-slow", "0.32s"));
    const easing = token("--ease-spring", "ease-out");
    const next = new Map<string, { x: number; y: number }>();
    for (const [key, element] of tiles.current) {
      const rect = element.getBoundingClientRect();
      const at = { x: rect.left - origin.left, y: rect.top - origin.top };
      next.set(key, at);
      const before = lastRects.current.get(key);
      if (!still && before && (Math.abs(before.x - at.x) > 1 || Math.abs(before.y - at.y) > 1)) {
        element.animate(
          [{ transform: `translate(${before.x - at.x}px, ${before.y - at.y}px)` }, { transform: "none" }],
          { duration, easing },
        );
      }
    }
    lastRects.current = next;
  });

  // While a tile is held: follow the pointer anywhere on the page.
  useEffect(() => {
    if (!drag) return undefined;
    const move = (event: PointerEvent) => {
      const current = dragRef.current;
      if (!current || event.pointerId !== current.pointerId) return;
      const moved = Math.hypot(event.clientX - current.startX, event.clientY - current.startY);
      const active = current.active || moved > DRAG_START;
      if (active && !current.active) hapticTick();
      const next: Drag = {
        ...current,
        x: event.clientX,
        y: event.clientY,
        active,
        insertAt: active ? insertionPoint(event.clientX, event.clientY, current.token) : null,
      };
      dragRef.current = next;
      setDrag(next);
      if (active) event.preventDefault();
    };
    const up = (event: PointerEvent) => {
      const current = dragRef.current;
      if (!current || event.pointerId !== current.pointerId) return;
      dragRef.current = null;
      setDrag(null);
      if (!current.active) return;
      dragged.current = true;
      const rest = builtRef.current.filter((t) => t !== current.token);
      // Where it was let go: the tile springs from there into its slot.
      lastRects.current.set(`t${current.token}`, dropPoint(current));
      if (current.insertAt !== null) {
        rest.splice(current.insertAt, 0, current.token);
        onBuilt(rest);
        hapticTick();
      } else if (current.from === "line") {
        onBuilt(rest);
      }
    };
    const cancel = (event: PointerEvent) => {
      if (dragRef.current?.pointerId !== event.pointerId) return;
      dragRef.current = null;
      setDrag(null);
    };
    window.addEventListener("pointermove", move, { passive: false });
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
    };
    // Re-attached when a drag starts or ends, not on every move.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drag === null]);

  /** The board-relative spot a dropped tile was let go at. */
  function dropPoint(current: Drag) {
    const origin = boardRef.current?.getBoundingClientRect();
    return {
      x: current.x - current.offsetX - (origin?.left ?? 0),
      y: current.y - current.offsetY - (origin?.top ?? 0),
    };
  }

  /** Where in the line a tile held at (x, y) would go; null when not over it. */
  function insertionPoint(x: number, y: number, held: number): number | null {
    const box = lineRef.current?.getBoundingClientRect();
    if (!box) return null;
    if (x < box.left - LINE_REACH || x > box.right + LINE_REACH || y < box.top - LINE_REACH || y > box.bottom + LINE_REACH) {
      return null;
    }
    const others = builtRef.current.filter((t) => t !== held);
    for (let i = 0; i < others.length; i++) {
      const element = tiles.current.get(`t${others[i]}`);
      if (!element) continue;
      const rect = element.getBoundingClientRect();
      if (y < rect.top) return i;
      if (y <= rect.bottom && x < rect.left + rect.width / 2) return i;
    }
    return others.length;
  }

  function press(event: ReactPointerEvent<HTMLButtonElement>, tokenIndex: number, from: "bank" | "line") {
    if (checked || event.button !== 0) return;
    dragged.current = false;
    const rect = event.currentTarget.getBoundingClientRect();
    const start: Drag = {
      token: tokenIndex,
      from,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      x: event.clientX,
      y: event.clientY,
      offsetX: event.clientX - rect.left,
      offsetY: event.clientY - rect.top,
      width: rect.width,
      active: false,
      insertAt: null,
    };
    dragRef.current = start;
    setDrag(start);
  }

  /** A tap on a bank tile: to the end of the line. */
  function place(tokenIndex: number) {
    if (dragged.current) {
      dragged.current = false;
      return;
    }
    hapticTick();
    onBuilt([...built, tokenIndex]);
    // A tile with endings opens them as it lands: the next thing to do.
    if (endingOf(exercise, tokenIndex) && forms[tokenIndex] === undefined) setPicking(tokenIndex);
  }

  /** A tap on a placed tile: its endings, or back out of the line. */
  function tapPlaced(tokenIndex: number) {
    if (dragged.current) {
      dragged.current = false;
      return;
    }
    hapticTick();
    if (endingOf(exercise, tokenIndex)) {
      setPicking(picking === tokenIndex ? null : tokenIndex);
    } else {
      takeOut(tokenIndex);
    }
  }

  function takeOut(tokenIndex: number) {
    onBuilt(built.filter((t) => t !== tokenIndex));
    if (picking === tokenIndex) setPicking(null);
  }

  function choose(tokenIndex: number, choice: string) {
    hapticTick();
    onForms({ ...forms, [tokenIndex]: choice });
    setPicking(null);
  }

  /** After checking: whether the tile at this place in the line is right there. */
  function verdict(position: number, tokenIndex: number) {
    return foldTile(tileText(exercise, forms, tokenIndex)) === foldTile(expected[position] ?? "") ? "right" : "wrong";
  }

  const pickingEnding = picking !== null && built.includes(picking) && !checked ? endingOf(exercise, picking) : undefined;
  const hasEndings = exercise.endings.length > 0;

  // The line as drawn: the placed tiles, with a gap where a held tile would land.
  const drawn: ({ kind: "tile"; token: number; position: number } | { kind: "gap" })[] = line.map((t, position) => ({
    kind: "tile",
    token: t,
    position,
  }));
  if (placing && placing.insertAt !== null) drawn.splice(placing.insertAt, 0, { kind: "gap" });

  return (
    <div className="tile-board" ref={boardRef}>
      <div
        className="tile-line"
        ref={lineRef}
        data-over={placing?.insertAt != null || undefined}
        data-empty={drawn.length === 0 || undefined}
        aria-label="Your sentence"
      >
        {drawn.length === 0 ? (
          <Text variant="small" tone="faint" as="span" className="tile-line__hint">
            Drag or tap the words here
          </Text>
        ) : null}
        {drawn.map((item) =>
          item.kind === "gap" ? (
            <span key="gap" className="tile tile-gap" ref={register("gap")} style={{ width: placing?.width }} aria-hidden="true" />
          ) : (
            <button
              key={`t${item.token}`}
              ref={register(`t${item.token}`)}
              className="tile"
              data-state={checked ? verdict(item.position, item.token) : picking === item.token ? "picking" : "placed"}
              data-ending={endingOf(exercise, item.token) ? (forms[item.token] !== undefined ? "picked" : "open") : undefined}
              disabled={checked}
              aria-label={
                endingOf(exercise, item.token)
                  ? `${tileText(exercise, forms, item.token)}: pick its ending`
                  : `${tileText(exercise, forms, item.token)}: take it out`
              }
              onPointerDown={(event) => press(event, item.token, "line")}
              onClick={() => tapPlaced(item.token)}
            >
              {tileText(exercise, forms, item.token)}
            </button>
          ),
        )}
      </div>

      {pickingEnding && picking !== null ? (
        <div className="tile-endings pop" role="group" aria-label="Pick the ending">
          <Text variant="caption" tone="dim" as="div">
            Which form fits here?
          </Text>
          <div className="tile-endings__choices">
            {pickingEnding.choices.map((choice) => (
              <button
                key={choice}
                className="tile tile-choice"
                data-chosen={forms[picking] === choice || undefined}
                aria-pressed={forms[picking] === choice}
                onClick={() => choose(picking, choice)}
              >
                {choice}
              </button>
            ))}
          </div>
          <Button variant="plain" size="sm" onClick={() => takeOut(picking)}>
            Take it out
          </Button>
        </div>
      ) : null}

      <div className="tile-bank">
        {exercise.tokens.map((_, tokenIndex) => {
          const used = built.includes(tokenIndex) || placing?.token === tokenIndex;
          return (
            <button
              key={tokenIndex}
              ref={used ? undefined : register(`t${tokenIndex}`)}
              className="tile"
              data-used={used || undefined}
              data-ending={endingOf(exercise, tokenIndex) ? "open" : undefined}
              disabled={checked || used}
              aria-label={`Add ${tileText(exercise, forms, tokenIndex)}`}
              onPointerDown={(event) => press(event, tokenIndex, "bank")}
              onClick={() => place(tokenIndex)}
            >
              {tileText(exercise, forms, tokenIndex)}
            </button>
          );
        })}
      </div>

      {hasEndings && !checked ? (
        <Text variant="caption" tone="faint" as="div">
          Words with a dotted line need another ending: tap them in the sentence to pick it.
        </Text>
      ) : null}

      {placing
        ? createPortal(
            <div
              className="tile tile-floating"
              data-ending={endingOf(exercise, placing.token) ? "open" : undefined}
              style={{
                width: placing.width,
                transform: `translate(${placing.x - placing.offsetX}px, ${placing.y - placing.offsetY}px)`,
              }}
              aria-hidden="true"
            >
              {tileText(exercise, forms, placing.token)}
            </div>,
            document.body,
          )
        : null}
    </div>
  );
}
