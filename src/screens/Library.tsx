import { useEffect, useMemo, useState } from "react";

import { TitleWithInfo } from "../components/Info";
import { Banner, Loading, SpeakButton } from "../components/ui";
import { api, errorMessage } from "../lib/api";
import type { CataloguePage, TopicProgress } from "../lib/types";

type Section = "words" | "sentences" | "dialogues";

/** Enough to scroll through, few enough to stay responsive on a phone. */
const PAGE = 200;

export function Library({
  topics,
  onOpenTopic,
}: {
  topics: TopicProgress[];
  onOpenTopic: (progress: TopicProgress, section: "conversation" | "words" | "sentences") => void;
}) {
  const [section, setSection] = useState<Section>("words");
  const [query, setQuery] = useState("");
  const [topicId, setTopicId] = useState<number | null>(null);
  const [page, setPage] = useState<CataloguePage | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Debounced so typing does not fire a query per keystroke.
  useEffect(() => {
    let cancelled = false;
    const timer = setTimeout(() => {
      api
        .browseCatalogue(topicId, query, PAGE)
        .then((result) => {
          if (!cancelled) setPage(result);
        })
        .catch((cause) => {
          if (!cancelled) setError(errorMessage(cause));
        });
    }, 180);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query, topicId]);

  const shown = useMemo(() => {
    if (!page) return { rows: [], total: 0 };
    return section === "sentences"
      ? { rows: page.sentences, total: page.totalSentences }
      : { rows: page.words, total: page.totalWords };
  }, [page, section]);

  return (
    <div className="scroll">
      <TitleWithInfo
        as="h1"
        title="Library"
        info={<p>Every word, sentence and conversation in the app — whether or not you have met it yet.</p>}
      />

      {error ? <Banner kind="error">{error}</Banner> : null}

      <div className="segmented" style={{ marginBottom: 12 }}>
        <button data-active={section === "words"} onClick={() => setSection("words")}>
          Words
        </button>
        <button data-active={section === "sentences"} onClick={() => setSection("sentences")}>
          Sentences
        </button>
        <button data-active={section === "dialogues"} onClick={() => setSection("dialogues")}>
          Dialogues
        </button>
      </div>

      {section === "dialogues" ? (
        <div className="card flush">
          {topics.map((entry) => (
            <button
              key={entry.topic.id}
              className="list-row"
              onClick={() => onOpenTopic(entry, "conversation")}
            >
              <span className="avatar">{entry.topic.ord}</span>
              <span className="grow">
                <span className="pl">{entry.topic.titlePl}</span>
                <span className="tr">{entry.topic.title}</span>
              </span>
              <span className="faint">›</span>
            </button>
          ))}
        </div>
      ) : (
        <>
          <input
            value={query}
            placeholder={`Search ${section}…`}
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            onChange={(event) => setQuery(event.target.value)}
            style={{ marginBottom: 10 }}
          />

          <div className="row wrap" style={{ gap: 6, marginBottom: 12 }}>
            <button
              className={`chip${topicId === null ? " sky" : ""}`}
              onClick={() => setTopicId(null)}
            >
              All topics
            </button>
            {topics.map((entry) => (
              <button
                key={entry.topic.id}
                className={`chip${topicId === entry.topic.id ? " sky" : ""}`}
                onClick={() => setTopicId(entry.topic.id)}
              >
                {entry.topic.title}
              </button>
            ))}
          </div>

          {!page ? (
            <Loading />
          ) : (
            <>
              <div className="row between small faint" style={{ marginBottom: 8 }}>
                <span>
                  {shown.total} {section}
                  {topicId !== null || query.trim() ? " matching" : ""}
                </span>
                {shown.total > shown.rows.length ? (
                  <span>showing first {shown.rows.length}</span>
                ) : null}
              </div>

              <div className="card flush">
                {shown.rows.map((row) => (
                  <div key={row.id} className="list-row">
                    <span className="grow selectable">
                      <span className="pl">{row.pl}</span>
                      <span className="tr">
                        {row.meaning}
                        {"pos" in row ? ` · ${row.pos}` : ""}
                      </span>
                    </span>
                    <SpeakButton text={row.pl} />
                  </div>
                ))}
                {shown.rows.length === 0 ? (
                  <div className="list-row faint small">
                    Nothing matches “{query.trim()}”.
                  </div>
                ) : null}
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
}
