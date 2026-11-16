import { useEffect, useState } from "react";

import { Inline, Stack, Text } from "../design";
import { api } from "../lib/api";
import type { WordExample } from "../lib/types";
import { SlowButton, SpeakButton } from "./ui";

/**
 * Reinforcement after a wrong answer: the right answer to hear (normal and
 * slow), and — for a word — two sentences that use it, each playable.
 */
export function WrongAnswerReview({
  pl,
  meaning,
  wordId,
}: {
  pl: string;
  meaning: string;
  /** Set for words; sentences have no examples of their own. */
  wordId: number | null;
}) {
  const [examples, setExamples] = useState<WordExample[] | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (wordId === null) return;
    let live = true;
    setLoading(true);
    api
      .wordExamples(wordId)
      .then((found) => live && setExamples(found))
      .catch(() => live && setExamples([]))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [wordId]);

  return (
    <Stack gap={2} className="wrong-review">
      <Inline gap={2}>
        <SpeakButton text={pl} size="sm" variant="tinted" />
        <SlowButton text={pl} size="sm" variant="tinted" />
        <Text variant="small" selectable>
          <Text tone="polish" weight="bold">
            {pl}
          </Text>{" "}
          — {meaning}
        </Text>
      </Inline>
      {loading ? (
        <Text variant="caption" tone="dim">
          Finding example sentences…
        </Text>
      ) : null}
      {examples && examples.length > 0 ? (
        <Stack gap={2} className="wrong-examples">
          <Text variant="eyebrow">In a sentence</Text>
          {examples.map((example) => (
            <Inline key={example.pl} gap={2} align="top">
              <SpeakButton text={example.pl} size="sm" />
              <Stack gap={1}>
                <Text variant="small" tone="polish" selectable>
                  {example.pl}
                </Text>
                <Text variant="caption" tone="dim" selectable>
                  {example.en}
                </Text>
              </Stack>
            </Inline>
          ))}
        </Stack>
      ) : null}
    </Stack>
  );
}
