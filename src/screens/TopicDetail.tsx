import { useEffect, useMemo, useState } from "react";

import { BlockRow } from "../components/BlockRow";
import { SpeakButton } from "../components/ui";
import {
  Banner,
  Button,
  ConfirmButton,
  Field,
  List,
  ListEmpty,
  ListItem,
  Screen,
  Section,
  Segmented,
  Sheet,
  Spinner,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import type { Block, Sentence, TopicProgress, Word } from "../lib/types";

export type TopicSection = "blocks" | "words" | "sentences";

/**
 * A course: the blocks it lists, in its order. Words and sentences belong to
 * those blocks; the other tabs list them all, each leading to its block.
 */
export function TopicDetail({
  progress,
  initialSection,
  today,
  onBack,
  onOpenBlock,
  onDeleted,
  onChanged,
}: {
  progress: TopicProgress;
  /** Which tab to land on. */
  initialSection?: TopicSection;
  today: number;
  onBack: () => void;
  onOpenBlock: (blockId: number) => void;
  onDeleted: () => void;
  /** The course was renamed; reload the lists that show it. */
  onChanged: () => void;
}) {
  const [blocks, setBlocks] = useState<Block[] | null>(null);
  const [tab, setTab] = useState<TopicSection>(initialSection ?? "blocks");
  const [words, setWords] = useState<Word[] | null>(null);
  const [sentences, setSentences] = useState<Sentence[] | null>(null);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [names, setNames] = useState({
    title: progress.topic.title,
    titlePl: progress.topic.titlePl,
    summary: progress.topic.summary,
  });
  const [renaming, setRenaming] = useState<typeof names | null>(null);

  const topicId = progress.topic.id;

  async function act(action: () => Promise<void>) {
    setError(null);
    try {
      await action();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }

  useEffect(() => {
    setError(null);
    Promise.all([api.blocksForTopic(topicId), api.browseWords(topicId, "", 500), api.browseSentences(topicId)])
      .then(([blockList, wordList, sentenceList]) => {
        setBlocks(blockList);
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
      (word) => word.pl.toLowerCase().includes(needle) || word.meaning.toLowerCase().includes(needle),
    );
  }, [words, query]);

  // A course of lessons has no words or sentences: then there are no tabs.
  const tabs = [
    { value: "blocks" as const, label: blocks ? `Blocks ${blocks.length}` : "Blocks" },
    ...(words && words.length > 0 ? [{ value: "words" as const, label: `Words ${words.length}` }] : []),
    ...(sentences && sentences.length > 0
      ? [{ value: "sentences" as const, label: `Sentences ${sentences.length}` }]
      : []),
  ];
  const shown = tabs.some((t) => t.value === tab) ? tab : "blocks";

  return (
    <Screen title={names.titlePl} subtitle={names.title} onBack={onBack} nav={false}>
      {error ? <Banner tone="error">{error}</Banner> : null}
      {names.summary ? (
        <Text variant="small" tone="dim" as="p">
          {names.summary}
        </Text>
      ) : null}

      {tabs.length > 1 ? <Segmented value={shown} onChange={setTab} options={tabs} /> : null}

      {shown === "blocks" ? (
        blocks ? (
          <List>
            {blocks.map((block) => (
              <BlockRow key={block.id} block={block} today={today} onOpen={(b) => onOpenBlock(b.id)} />
            ))}
            {blocks.length === 0 ? <ListEmpty>No blocks yet. Ask the tutor to add some.</ListEmpty> : null}
          </List>
        ) : (
          <Spinner />
        )
      ) : null}

      {shown === "words" && words ? (
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
                subtitle={`${word.meaning} · ${word.block}`}
                label={`${word.pl}: open ${word.block}`}
                onClick={() => onOpenBlock(word.blockId)}
                trailing={<SpeakButton text={word.pl} />}
              />
            ))}
            {filtered.length === 0 ? <ListEmpty>No match.</ListEmpty> : null}
          </List>
        </Stack>
      ) : null}

      {shown === "sentences" && sentences ? (
        <List>
          {sentences.map((sentence) => (
            <ListItem
              key={sentence.id}
              selectable
              title={sentence.pl}
              subtitle={`${sentence.meaning} · ${sentence.block}`}
              label={`${sentence.pl}: open ${sentence.block}`}
              onClick={() => onOpenBlock(sentence.blockId)}
              trailing={<SpeakButton text={sentence.pl} />}
            />
          ))}
        </List>
      ) : null}

      <Section title="Manage">
        <Stack gap={2}>
          <Button block onClick={() => setRenaming(names)}>
            Rename
          </Button>
          <ConfirmButton
            label="Delete course"
            confirmLabel="Delete"
            question="Delete this course? Blocks only in this course go with it, with their words and your progress on them."
            block
            onConfirm={() =>
              act(async () => {
                await api.courseDelete(topicId);
                onDeleted();
              })
            }
          />
        </Stack>
      </Section>

      {renaming ? (
        <Sheet title="Rename course" onClose={() => setRenaming(null)}>
          <Stack gap={4}>
            <Field label="Title in Polish">
              <input
                value={renaming.titlePl}
                onChange={(event) => setRenaming({ ...renaming, titlePl: event.target.value })}
              />
            </Field>
            <Field label="Title in English">
              <input
                value={renaming.title}
                onChange={(event) => setRenaming({ ...renaming, title: event.target.value })}
              />
            </Field>
            <Field label="What it is about">
              <textarea
                rows={2}
                value={renaming.summary}
                onChange={(event) => setRenaming({ ...renaming, summary: event.target.value })}
              />
            </Field>
            <Button
              variant="primary"
              size="lg"
              block
              disabled={!renaming.title.trim() || !renaming.titlePl.trim()}
              onClick={() =>
                act(async () => {
                  await api.courseRename(topicId, renaming.title, renaming.titlePl, renaming.summary);
                  setNames({
                    title: renaming.title.trim(),
                    titlePl: renaming.titlePl.trim(),
                    summary: renaming.summary.trim(),
                  });
                  setRenaming(null);
                  onChanged();
                })
              }
            >
              Save
            </Button>
          </Stack>
        </Sheet>
      ) : null}
    </Screen>
  );
}
