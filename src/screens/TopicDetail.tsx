import { useEffect, useMemo, useState } from "react";

import { DialogueToolbar, DialogueView } from "../components/DialogueView";
import { BlockRow } from "../components/BlockRow";
import { SpeakButton } from "../components/ui";
import {
  Banner,
  ConfirmButton,
  List,
  ListEmpty,
  ListItem,
  Screen,
  Section,
  Segmented,
  Spinner,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import type { Block, DialogueLine, Sentence, TopicProgress, Word } from "../lib/types";

export type TopicSection = "conversation" | "words" | "sentences";

export function TopicDetail({
  progress,
  initialSection,
  today,
  onBack,
  onOpenBlock,
  onDeleted,
}: {
  progress: TopicProgress;
  /** Which tab to land on. */
  initialSection?: TopicSection;
  today: number;
  onBack: () => void;
  onOpenBlock: (block: Block) => void;
  onDeleted: () => void;
}) {
  const [blocks, setBlocks] = useState<Block[] | null>(null);
  const [tab, setTab] = useState<TopicSection>(initialSection ?? "conversation");
  const [lines, setLines] = useState<DialogueLine[] | null>(null);
  const [words, setWords] = useState<Word[] | null>(null);
  const [sentences, setSentences] = useState<Sentence[] | null>(null);
  const [query, setQuery] = useState("");
  const [revealAll, setRevealAll] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const topicId = progress.topic.id;
  const programme = progress.topic.kind === "programme";

  // Programmes may have no dialogue; then there is no conversation tab.
  useEffect(() => {
    if (lines && lines.length === 0 && tab === "conversation") setTab("words");
  }, [lines, tab]);

  async function act(action: () => Promise<void>) {
    setError(null);
    try {
      await action();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }

  useEffect(() => {
    api.blocksForTopic(topicId).then(setBlocks).catch((cause) => setError(errorMessage(cause)));
  }, [topicId]);

  useEffect(() => {
    setError(null);
    Promise.all([
      api.getDialogue(topicId),
      api.browseWords(topicId, "", 400),
      api.browseSentences(topicId),
    ])
      .then(([dialogue, wordList, sentenceList]) => {
        setLines(dialogue.lines);
        setWords(wordList);
        setSentences(sentenceList);
      })
      .catch((cause) => setError(errorMessage(cause)));
  }, [topicId]);

  const filtered = useMemo(() => {
    if (!words) return [];
    const needle = query.trim().toLowerCase();
    if (!needle) return words;
    return words.filter(
      (word) =>
        word.pl.toLowerCase().includes(needle) ||
        word.meaning.toLowerCase().includes(needle),
    );
  }, [words, query]);

  return (
    <Screen title={progress.topic.titlePl} subtitle={progress.topic.title} onBack={onBack} nav={false}>
      {error ? <Banner tone="error">{error}</Banner> : null}

      <Section title={blocks ? `${blocks.length} blocks` : "Blocks"}>
        {blocks ? (
          <List>
            {blocks.map((block) => (
              <BlockRow key={block.id} block={block} today={today} onOpen={onOpenBlock} />
            ))}
          </List>
        ) : (
          <Spinner />
        )}
      </Section>

      <Section title="Browse">
      <Stack gap={4}>
        <Segmented
          value={tab}
          onChange={setTab}
          options={[
            ...(lines === null || lines.length > 0
              ? [{ value: "conversation" as const, label: "Conversation" }]
              : []),
            { value: "words" as const, label: `Words ${progress.seenWords}/${progress.totalWords}` },
            { value: "sentences" as const, label: "Sentences" },
          ]}
        />

        {tab === "conversation" ? (
          lines ? (
            <Stack gap={4}>
              {progress.topic.summary ? (
                <Text variant="small" tone="dim" as="p">
                  {progress.topic.summary}
                </Text>
              ) : null}
              <DialogueToolbar
                lines={lines}
                revealAll={revealAll}
                onToggleReveal={() => setRevealAll((value) => !value)}
              />
              <DialogueView lines={lines} revealAll={revealAll} />
            </Stack>
          ) : (
            <Spinner />
          )
        ) : null}

        {tab === "words" ? (
          words ? (
            <Stack gap={3}>
              <input
                type="search"
                value={query}
                placeholder="Search these words"
                onChange={(event) => setQuery(event.target.value)}
              />
              <List>
                {filtered.map((word) => (
                  <ListItem
                    key={word.id}
                    selectable
                    title={word.pl}
                    subtitle={`${word.meaning} · ${word.pos}`}
                    trailing={<SpeakButton text={word.pl} />}
                  />
                ))}
                {filtered.length === 0 ? <ListEmpty>No match.</ListEmpty> : null}
              </List>
            </Stack>
          ) : (
            <Spinner />
          )
        ) : null}

        {tab === "sentences" ? (
          sentences ? (
            <List>
              {sentences.map((sentence) => (
                <ListItem
                  key={sentence.id}
                  selectable
                  title={sentence.pl}
                  subtitle={sentence.meaning}
                  trailing={<SpeakButton text={sentence.pl} />}
                />
              ))}
            </List>
          ) : (
            <Spinner />
          )
        ) : null}
      </Stack>

      </Section>

      {programme ? (
        <Section title="Manage">
          <ConfirmButton
            label="Delete programme"
            confirmLabel="Delete"
            question="Delete this programme, its words and your progress on them?"
            block
            onConfirm={() =>
              act(async () => {
                await api.programmeDelete(topicId);
                onDeleted();
              })
            }
          />
        </Section>
      ) : null}
    </Screen>
  );
}
