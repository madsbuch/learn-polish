import { useEffect, useState } from "react";

import { Banner, Button, Inline, Spinner, Stack, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import type { GoalHelp } from "../lib/types";

/**
 * A guided writing block's word bank: for each goal, a way to start and
 * words to use. Tapping one adds it to the message being written.
 */
export function WordBank({ blockId, onUse }: { blockId: number; onUse: (text: string) => void }) {
  const [help, setHelp] = useState<GoalHelp[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(true);

  const load = () => {
    setError(null);
    api
      .writingScaffold(blockId)
      .then(setHelp)
      .catch((cause) => setError(errorMessage(cause)));
  };
  useEffect(load, [blockId]);

  return (
    <div className="word-bank">
      <Inline justify="between">
        <Text variant="eyebrow">Word bank</Text>
        <Button variant="plain" size="sm" onClick={() => setOpen(!open)}>
          {open ? "Hide" : "Show"}
        </Button>
      </Inline>
      {!open ? null : error ? (
        <Stack gap={2}>
          <Banner tone="error">{error}</Banner>
          <Button size="sm" onClick={load}>
            Try again
          </Button>
        </Stack>
      ) : help === null ? (
        <Inline gap={2}>
          <Spinner />
          <Text variant="small" tone="faint">
            Gathering words…
          </Text>
        </Inline>
      ) : (
        <Stack gap={3}>
          {help.map((goal) => (
            <Stack key={goal.goal} gap={1}>
              <Text variant="small" tone="dim">
                {goal.goal}
              </Text>
              <div className="word-bank__chips">
                <button className="word-chip" data-starter onClick={() => onUse(goal.starter.pl.replace(/…$/, " "))}>
                  {goal.starter.pl}
                  <span>{goal.starter.en}</span>
                </button>
                {goal.words.map((word) => (
                  <button key={word.pl} className="word-chip" onClick={() => onUse(word.pl)}>
                    {word.pl}
                    <span>{word.en}</span>
                  </button>
                ))}
              </div>
            </Stack>
          ))}
        </Stack>
      )}
    </div>
  );
}
