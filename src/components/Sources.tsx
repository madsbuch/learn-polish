import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Avatar, Button, Inline, List, ListItem, Screen, Section, Segmented, Spinner, Stack, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { fileUrl } from "../lib/files";
import { toast } from "../lib/toast";
import type { ArticleView, WikiSource } from "../lib/types";
import { Markdown } from "./Markdown";

/**
 * A study block's Wikipedia sources: a card each, opening the article in the
 * app. Nothing shows for a block without any.
 */
export function Sources({
  blockId,
  factNote,
  factCheckedAt,
  onRead,
  onChecked,
}: {
  blockId: number;
  /** What the last check changed. */
  factNote: string | null;
  factCheckedAt: number | null;
  onRead: (source: WikiSource) => void;
  /** A check corrected the lesson: reload the block. */
  onChecked: () => void;
}) {
  const [sources, setSources] = useState<WikiSource[]>([]);
  const [checking, setChecking] = useState(false);
  useEffect(() => {
    api
      .blockSources(blockId)
      .then(setSources)
      .catch(() => setSources([]));
  }, [blockId]);
  if (sources.length === 0) return null;
  return (
    <Section
      title="Sources"
      info={
        <p>
          The lesson is written from these Wikipedia articles. Read them here — in Polish too, when
          Polish Wikipedia has the article. Every three months the lesson is checked against them again.
        </p>
      }
    >
      {factNote ? (
        <Text variant="small" tone="dim" as="p">
          Updated from Wikipedia — {factNote}
        </Text>
      ) : null}
      <List>
        {sources.map((source) => (
          <ListItem
            key={source.id}
            leading={
              source.imageFileId !== null ? (
                <img className="source-thumb" src={fileUrl(source.imageFileId)} alt="" />
              ) : (
                <Avatar>W</Avatar>
              )
            }
            title={source.title}
            subtitle={<span className="clamp-2">{source.summary}</span>}
            label={`Read “${source.title}”`}
            onClick={() => onRead(source)}
          />
        ))}
      </List>
      <Inline gap={2} justify="between">
        <Text variant="caption" tone="faint">
          {factCheckedAt ? `Checked ${new Date(factCheckedAt * 1000).toLocaleDateString()}` : "Not checked yet"} ·
          again every three months
        </Text>
        <Button
          size="sm"
          variant="plain"
          loading={checking}
          onClick={async () => {
            setChecking(true);
            try {
              const check = await api.factCheckBlock(blockId);
              const what = check.note?.replace(/^\d{4}-\d{2}-\d{2}: /, "") ?? "the lesson was out of date";
              toast(check.updated ? `Corrected — ${what}` : "Still matches its sources");
              onChecked();
            } catch (cause) {
              toast(errorMessage(cause), "error");
            } finally {
              setChecking(false);
            }
          }}
        >
          Check now
        </Button>
      </Inline>
    </Section>
  );
}

/**
 * A source's article, read in the app: in its own language or in Polish,
 * with where it comes from (Wikipedia's licence asks for that).
 */
export function ArticleReader({
  source,
  onBack,
  onRemoved,
}: {
  source: WikiSource;
  onBack: () => void;
  onRemoved: () => void;
}) {
  const [polish, setPolish] = useState(false);
  const [article, setArticle] = useState<ArticleView | null>(null);
  useEffect(() => {
    setArticle(null);
    api
      .sourceArticle(source.id, polish)
      .then(setArticle)
      .catch((cause) => toast(errorMessage(cause), "error"));
  }, [source.id, polish]);

  return (
    <Screen title={article?.title ?? source.title} subtitle="Wikipedia" onBack={onBack} nav={false}>
      {source.polishTitle && source.lang !== "pl" ? (
        <Segmented
          value={polish ? "pl" : "own"}
          onChange={(value) => setPolish(value === "pl")}
          options={[
            { value: "own", label: source.lang === "en" ? "English" : source.lang.toUpperCase() },
            { value: "pl", label: "Polski" },
          ]}
        />
      ) : null}
      {source.imageFileId !== null && !polish ? (
        <img className="source-picture" src={fileUrl(source.imageFileId)} alt="" />
      ) : null}
      {article ? (
        <div className="selectable">
          <Markdown text={article.content} />
        </div>
      ) : (
        <Spinner />
      )}
      {article ? (
        <Stack gap={2}>
          <Text variant="caption" tone="faint" as="p">
            From Wikipedia, the free encyclopedia — “{article.title}”, available under the Creative
            Commons Attribution-ShareAlike 4.0 licence.
          </Text>
          <Button block onClick={() => void openUrl(article.url).catch(() => toast("Could not open the browser.", "error"))}>
            Open on Wikipedia
          </Button>
          <Button
            variant="plain"
            block
            onClick={async () => {
              try {
                await api.sourceRemove(source.id);
                toast(`Removed “${source.title}” from the sources`);
                onRemoved();
              } catch (cause) {
                toast(errorMessage(cause), "error");
              }
            }}
          >
            Remove this source
          </Button>
        </Stack>
      ) : null}
    </Screen>
  );
}
