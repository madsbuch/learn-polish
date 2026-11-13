import { useEffect, useMemo, useState } from "react";

import { TitleWithInfo } from "../components/Info";
import { Banner, Loading, SpeakButton } from "../components/ui";
import { api, errorMessage } from "../lib/api";
import type { CataloguePage, TopicProgress } from "../lib/types";

type Section = "topics" | "words" | "sentences";

/** Enough to scroll through, few enough to stay responsive on a phone. */
const PAGE = 200;

export function Library({
  topics,
  focusTopicId,
  onOpenTopic,
  onOpenTutor,
}: {
  topics: TopicProgress[];
  focusTopicId: number | null;
  onOpenTopic: (progress: TopicProgress, section: "conversation" | "words" | "sentences") => void;
  onOpenTutor: () => void;
}) {
  const [section, setSection] = useState<Section>("topics");
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

  const course = topics.filter((entry) => entry.topic.kind === "course");
  const programmes = topics.filter((entry) => entry.topic.kind === "programme");

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
        info={
          <>
            <p>Every topic, word and sentence in the app — whether or not you have met it yet.</p>
            <p>Open a topic to practise it now, or to make it your daily check-in. Ask the tutor to build programmes on anything else.</p>
          </>
        }
      />

      {error ? <Banner kind="error">{error}</Banner> : null}

      <div className="segmented" style={{ marginBottom: 12 }}>
        <button data-active={section === "topics"} onClick={() => setSection("topics")}>
          Topics
        </button>
        <button data-active={section === "words"} onClick={() => setSection("words")}>
          Words
        </button>
        <button data-active={section === "sentences"} onClick={() => setSection("sentences")}>
          Sentences
        </button>
      </div>

      {section === "topics" ? (
        <>
          <h3 style={{ margin: "4px 0 8px" }}>Your programmes</h3>
          {programmes.length === 0 ? (
            <button className="card row" style={{ width: "100%", textAlign: "left" }} onClick={onOpenTutor}>
              <span style={{ fontSize: 22 }}>✨</span>
              <span className="grow small">
                None yet. Ask the tutor, e.g. <i>“Build me a programme for learning the colours”</i>.
              </span>
              <span className="faint">›</span>
            </button>
          ) : (
            <TopicList entries={programmes} focusTopicId={focusTopicId} onOpen={onOpenTopic} />
          )}

          <h3 style={{ margin: "18px 0 8px" }}>Course</h3>
          <TopicList entries={course} focusTopicId={focusTopicId} onOpen={onOpenTopic} />
        </>
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

function TopicList({
  entries,
  focusTopicId,
  onOpen,
}: {
  entries: TopicProgress[];
  focusTopicId: number | null;
  onOpen: (progress: TopicProgress, section: "conversation" | "words" | "sentences") => void;
}) {
  return (
    <div className="card flush">
      {entries.map((entry) => (
        <button
          key={entry.topic.id}
          className="list-row"
          onClick={() => onOpen(entry, "conversation")}
        >
          <span className="avatar">{entry.topic.kind === "course" ? entry.topic.ord : "✦"}</span>
          <span className="grow">
            <span className="pl">{entry.topic.title}</span>
            <span className="tr">
              {entry.topic.titlePl} · {entry.seenWords}/{entry.totalWords} words
              {entry.topic.id === focusTopicId ? " · daily focus" : ""}
            </span>
          </span>
          <span className="faint">›</span>
        </button>
      ))}
    </div>
  );
}
