import { useEffect, useMemo, useRef, useState } from "react";

import { Card, Stack, Text } from "../design";
import type { Exercise } from "../lib/types";
import { playMatch, playWrong } from "../lib/sounds";

/**
 * Fill the gaps: a few sentences, each missing a word, and a word bank. Tap
 * a word to put it in the selected gap (or the first empty one); tap a filled
 * gap to take its word back out.
 */
export function GapFill({
  exercise,
  filled,
  onChange,
  checked,
}: {
  exercise: Exercise;
  /** For each gap, the index of the bank word in it, or null. */
  filled: (number | null)[];
  onChange: (filled: (number | null)[]) => void;
  checked: boolean;
}) {
  const [active, setActive] = useState(0);
  const slots = exercise.gaps.map((_, i) => filled[i] ?? null);

  function place(bankIndex: number) {
    const target = slots[active] === null ? active : slots.findIndex((slot) => slot === null);
    if (target < 0) return;
    const next = [...slots];
    next[target] = bankIndex;
    onChange(next);
    const following = next.findIndex((slot, i) => i > target && slot === null);
    setActive(following >= 0 ? following : Math.max(0, next.findIndex((slot) => slot === null)));
  }

  function clear(gapIndex: number) {
    if (checked) return;
    if (slots[gapIndex] === null) {
      setActive(gapIndex);
      return;
    }
    const next = [...slots];
    next[gapIndex] = null;
    onChange(next);
    setActive(gapIndex);
  }

  return (
    <>
      <Stack gap={2}>
        {exercise.gaps.map((gap, i) => {
          const word = slots[i] === null ? null : exercise.options[slots[i]!];
          const state = !checked
            ? i === active && word === null
              ? "active"
              : word
                ? "filled"
                : "empty"
            : word === gap.answer
              ? "right"
              : "wrong";
          return (
            <Card key={i}>
              <div className="gap-line selectable">
                {gap.before}
                <button className="gap-slot" data-state={state} onClick={() => clear(i)}>
                  {word ?? " "}
                </button>
                {gap.after}
              </div>
              {checked && word !== gap.answer ? (
                <Text variant="small" tone="ok" weight="bold" as="div">
                  → {gap.answer}
                </Text>
              ) : null}
              <Text variant="small" tone="faint" as="div">
                {gap.meaning}
              </Text>
            </Card>
          );
        })}
      </Stack>
      <div className="tokens">
        {exercise.options.map((option, bankIndex) => (
          <button
            key={`${option}-${bankIndex}`}
            className="token"
            data-used={slots.includes(bankIndex)}
            disabled={checked || slots.includes(bankIndex)}
            onClick={() => place(bankIndex)}
          >
            {option}
          </button>
        ))}
      </div>
    </>
  );
}

interface Card {
  id: number;
  pair: number;
  text: string;
  lang: "pl" | "en";
}

function shuffle<T>(items: T[]): T[] {
  const copy = [...items];
  for (let i = copy.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [copy[i], copy[j]] = [copy[j], copy[i]];
  }
  return copy;
}

/**
 * Memory cards: every card lies face down. Turn two over; a Polish word and
 * its English meaning stay up, anything else turns back.
 */
export function Memory({
  exercise,
  onComplete,
}: {
  exercise: Exercise;
  /** Called once, when every pair is found, with how many turns it took. */
  onComplete: (turns: number) => void;
}) {
  const cards = useMemo<Card[]>(
    () =>
      shuffle(
        exercise.pairs.flatMap((pair, i) => [
          { id: i * 2, pair: i, text: pair.pl, lang: "pl" as const },
          { id: i * 2 + 1, pair: i, text: pair.en, lang: "en" as const },
        ]),
      ),
    [exercise.key],
  );
  const [open, setOpen] = useState<number[]>([]);
  const [found, setFound] = useState<Set<number>>(new Set());
  const [miss, setMiss] = useState<number[]>([]);
  const [turns, setTurns] = useState(0);
  const timer = useRef<number | undefined>(undefined);
  const done = found.size === exercise.pairs.length;

  useEffect(() => () => window.clearTimeout(timer.current), []);
  useEffect(() => {
    if (done) onComplete(turns);
    // onComplete is called once, when the last pair is found.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [done]);

  function flip(index: number) {
    const card = cards[index];
    if (done || found.has(card.pair) || open.includes(index) || miss.length > 0) return;
    if (open.length === 0) {
      setOpen([index]);
      return;
    }
    const first = cards[open[0]];
    setTurns((n) => n + 1);
    if (first.pair === card.pair) {
      setFound(new Set([...found, card.pair]));
      setOpen([]);
      playMatch();
    } else {
      playWrong();
      setOpen([open[0], index]);
      setMiss([open[0], index]);
      timer.current = window.setTimeout(() => {
        setOpen([]);
        setMiss([]);
      }, 900);
    }
  }

  return (
    <>
      <div className="memory-grid">
        {cards.map((card, index) => {
          const state = found.has(card.pair)
            ? "found"
            : miss.includes(index)
              ? "miss"
              : open.includes(index)
                ? "open"
                : "down";
          return (
            <button
              key={card.id}
              className="memory-card"
              data-state={state}
              data-lang={card.lang}
              onClick={() => flip(index)}
              aria-label={state === "down" ? "Hidden card" : card.text}
            >
              {state === "down" ? "?" : card.text}
            </button>
          );
        })}
      </div>
      <Text variant="small" tone="faint" align="center" as="div">
        {found.size} of {exercise.pairs.length} pairs · {turns} turn{turns === 1 ? "" : "s"}
      </Text>
    </>
  );
}
