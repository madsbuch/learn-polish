import { useEffect, useState } from "react";

import { Button, Inline, Spinner, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import type { GoalHelp, Phrase } from "../lib/types";

/** True when the phrase is already in the message being written. */
function used(draft: string, phrase: Phrase): boolean {
  const words = phrase.pl.replace(/…$/, "").trim().toLowerCase();
  return words.length > 0 && draft.toLowerCase().includes(words);
}

/**
 * A guided writing lesson's word bank, docked above the message box so it is
 * at hand while writing: one goal at a time, a way to start and words to use.
 * Tapping one adds it to the message; what is in the message already is ticked.
 */
export function WordBank({
  lessonId,
  draft,
  onUse,
}: {
  lessonId: number;
  draft: string;
  onUse: (text: string) => void;
}) {
  const [help, setHelp] = useState<GoalHelp[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(true);
  const [goal, setGoal] = useState(0);

  const load = () => {
    setError(null);
    api
      .writingScaffold(lessonId)
      .then(setHelp)
      .catch((cause) => setError(errorMessage(cause)));
  };
  useEffect(load, [lessonId]);

  const current = help?.[Math.min(goal, help.length - 1)];

  return (
    <div className="word-bank" data-open={open}>
      <Inline justify="between" gap={2}>
        <button
          className="word-bank__toggle"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          <Text variant="eyebrow">Word bank</Text>
          <span aria-hidden="true">{open ? "▾" : "▸"}</span>
        </button>
        {open && help && help.length > 1 ? (
          <div className="word-bank__goals" role="tablist" aria-label="Goals">
            {help.map((g, i) => (
              <button
                key={g.goal}
                role="tab"
                aria-selected={i === goal}
                aria-label={g.goal}
                className="word-bank__goal"
                onClick={() => setGoal(i)}
              >
                {i + 1}
              </button>
            ))}
          </div>
        ) : null}
      </Inline>
      {!open ? null : error ? (
        <Inline gap={2} justify="between">
          <Text variant="small" tone="bad">
            {error}
          </Text>
          <Button size="sm" onClick={load}>
            Try again
          </Button>
        </Inline>
      ) : !current ? (
        <Inline gap={2}>
          <Spinner />
          <Text variant="small" tone="faint">
            Gathering words…
          </Text>
        </Inline>
      ) : (
        <>
          <Text variant="caption" tone="dim" ellipsis>
            {current.goal}
          </Text>
          <div className="word-bank__chips">
            {[current.starter, ...current.words].map((phrase, i) => {
              const done = used(draft, phrase);
              return (
                <button
                  key={phrase.pl}
                  className="word-chip"
                  data-starter={i === 0 || undefined}
                  data-used={done || undefined}
                  onClick={() =>
                    onUse(i === 0 ? phrase.pl.replace(/…$/, " ") : phrase.pl)
                  }
                >
                  {done ? `✓ ${phrase.pl}` : phrase.pl}
                  <span>{phrase.en}</span>
                </button>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
