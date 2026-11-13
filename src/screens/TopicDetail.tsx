import { useEffect, useMemo, useState } from "react";

import { DialogueToolbar, DialogueView } from "../components/DialogueView";
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
import type { Block, DialogueLine, Sentence, TopicProgress, Word } from "../lib/types";

export type TopicSection = "conversation" | "words" | "sentences";

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
  onOpenBlock: (block: Block) => void;
  onDeleted: () => void;
  /** The course was renamed; reload the lists that show it. */
  onChanged: () => void;
}) {
  const [blocks, setBlocks] = useState<Block[] | null>(null);
  const [tab, setTab] = useState<TopicSection>(initialSection ?? "conversation");
  const [lines, setLines] = useState<DialogueLine[] | null>(null);
  const [words, setWords] = useState<Word[] | null>(null);
  const [sentences, setSentences] = useState<Sentence[] | null>(null);
  const [query, setQuery] = useState("");
  const [revealAll, setRevealAll] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [names, setNames] = useState({
    title: progress.topic.title,
    titlePl: progress.topic.titlePl,
    summary: progress.topic.summary,
  });
  const [renaming, setRenaming] = useState<typeof names | null>(null);

  const topicId = progress.topic.id;

  // A course may have no dialogue, words or sentences (a course of lessons):
  // the browser shows what there is, and nothing when there is nothing.
  const hasLines = lines === null || lines.length > 0;
  const hasWords = words === null || words.length > 0;
  const hasSentences = sentences === null || sentences.length > 0;
  useEffect(() => {
    if (tab === "conversation" && !hasLines) setTab(hasWords ? "words" : "sentences");
    if (tab === "words" && !hasWords) setTab(hasSentences ? "sentences" : "conversation");
  }, [tab, hasLines, hasWords, hasSentences]);

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
    <Screen title={names.titlePl} subtitle={names.title} onBack={onBack} nav={false}>
      {error ? <Banner tone="error">{error}</Banner> : null}
      {names.summary ? (
        <Text variant="small" tone="dim" as="p">
          {names.summary}
        </Text>
      ) : null}

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

      {hasLines || hasWords || hasSentences ? (
      <Section title="Browse">
      <Stack gap={4}>
        <Segmented
          value={tab}
          onChange={setTab}
          options={[
            ...(hasLines ? [{ value: "conversation" as const, label: "Conversation" }] : []),
            ...(hasWords
              ? [{ value: "words" as const, label: `Words ${progress.seenWords}/${progress.totalWords}` }]
              : []),
            ...(hasSentences ? [{ value: "sentences" as const, label: "Sentences" }] : []),
          ]}
        />

        {tab === "conversation" ? (
          lines ? (
            <Stack gap={4}>
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
      ) : null}

      <Section title="Manage">
        <Stack gap={2}>
          <Button block onClick={() => setRenaming(names)}>
            Rename
          </Button>
          <ConfirmButton
            label="Delete course"
            confirmLabel="Delete"
            question="Delete this course, all its blocks, its words and your progress on them?"
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
