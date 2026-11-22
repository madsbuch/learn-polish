import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import {
  Avatar,
  Button,
  EmptyState,
  IconButton,
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  Segmented,
  Spinner,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { fileUrl } from "../lib/files";
import { toast } from "../lib/toast";
import type { ArticleView, LessonId, SourceId, WikiSource } from "../lib/types";
import { ExternalIcon } from "./icons";
import { Markdown } from "./Markdown";
import { WikiArticle } from "./WikiArticle";

/** Sources as rows: picture, title and lead, each opening the article. */
export function SourceList({ sources, onRead }: { sources: WikiSource[]; onRead: (source: WikiSource) => void }) {
  return (
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
  );
}

/**
 * A study lesson's Wikipedia sources: a card each, opening the article in the
 * app. Nothing shows for a lesson without any.
 */
export function Sources({
  lessonId,
  factNote,
  factCheckedAt,
  onRead,
  onChecked,
}: {
  lessonId: LessonId;
  /** What the last check changed. */
  factNote: string | null;
  factCheckedAt: number | null;
  onRead: (source: WikiSource) => void;
  /** A check corrected the lesson: reload the lesson. */
  onChecked: () => void;
}) {
  const [sources, setSources] = useState<WikiSource[]>([]);
  const [checking, setChecking] = useState(false);
  useEffect(() => {
    api
      .lessonSources(lessonId)
      .then(setSources)
      .catch(() => setSources([]));
  }, [lessonId]);
  if (sources.length === 0) return null;
  return (
    <Section
      title="Sources"
      info={
        <p>
          The lesson is written from these Wikipedia articles. Read them here — in Polish too, when Polish Wikipedia has
          the article. Every three months the lesson is checked against them again.
        </p>
      }
    >
      {factNote ? (
        <Text variant="small" tone="dim" as="p">
          Updated from Wikipedia — {factNote}
        </Text>
      ) : null}
      <SourceList sources={sources} onRead={onRead} />
      <Inline gap={2} justify="between">
        <Text variant="caption" tone="faint">
          {factCheckedAt ? `Checked ${new Date(factCheckedAt * 1000).toLocaleDateString()}` : "Not checked yet"} · again
          every three months
        </Text>
        <Button
          size="sm"
          variant="plain"
          loading={checking}
          onClick={async () => {
            setChecking(true);
            try {
              const check = await api.factCheckLesson(lessonId);
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
 * A source's article, read in the app: in its own language or in Polish, laid
 * out as on Wikipedia (sections, lists, tables, the infobox, figures), with
 * where it comes from (Wikipedia's licence asks for that) and a way to open
 * it on Wikipedia.
 */
export function ArticleReader({
  source,
  onBack,
  onRemoved,
}: {
  source: WikiSource;
  onBack: () => void;
  /** Lessons can drop a source; an episode keeps what it was written from. */
  onRemoved?: () => void;
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

  const url = article?.url ?? source.url;
  const openExternally = () => void openUrl(url).catch(() => toast("Could not open the browser.", "error"));

  return (
    <Screen
      title={article?.title ?? source.title}
      subtitle="Wikipedia"
      onBack={onBack}
      nav={false}
      actions={<IconButton icon={<ExternalIcon />} label="Open on Wikipedia" onClick={openExternally} />}
    >
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
      {article ? (
        article.html ? (
          <WikiArticle html={article.html} />
        ) : (
          <>
            {source.imageFileId !== null && !polish ? (
              <img className="source-picture" src={fileUrl(source.imageFileId)} alt="" />
            ) : null}
            <div className="selectable">
              <Markdown text={article.content} />
            </div>
          </>
        )
      ) : (
        <Spinner />
      )}
      {article ? (
        <Stack gap={2}>
          <Text variant="caption" tone="faint" as="p">
            From Wikipedia, the free encyclopedia — “{article.title}”, available under the Creative Commons
            Attribution-ShareAlike 4.0 licence.
          </Text>
          <Button wide icon={<ExternalIcon />} onClick={openExternally}>
            Open on Wikipedia
          </Button>
          {onRemoved ? (
            <Button
              variant="plain"
              wide
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
          ) : null}
        </Stack>
      ) : null}
    </Screen>
  );
}

/**
 * A source read from its address (`…/src_…`): found among its lesson's or
 * song's sources, so the reader opens on a reload or coming back to it.
 */
export function SourceReader({
  sourceId,
  load,
  onBack,
  onRemoved,
}: {
  sourceId: SourceId;
  /** The sources it is among. */
  load: () => Promise<WikiSource[]>;
  onBack: () => void;
  onRemoved?: () => void;
}) {
  const [source, setSource] = useState<WikiSource | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    load()
      .then((sources) => live && setSource(sources.find((entry) => entry.id === sourceId) ?? null))
      .catch(() => live && setSource(null));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sourceId]);
  if (source) return <ArticleReader source={source} onBack={onBack} onRemoved={onRemoved} />;
  return (
    <Screen title="Wikipedia" onBack={onBack} nav={false}>
      {source === null ? <EmptyState title="This source is no longer there" /> : <Spinner />}
    </Screen>
  );
}
