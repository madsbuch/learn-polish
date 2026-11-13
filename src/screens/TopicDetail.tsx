import { useEffect, useMemo, useState } from "react";

import { DialogueToolbar, DialogueView } from "../components/DialogueView";
import { Banner, Loading, SpeakButton, Spinner } from "../components/ui";
import { api, errorMessage } from "../lib/api";
import type { DialogueLine, Sentence, TopicProgress, Word } from "../lib/types";

export type TopicSection = "conversation" | "words" | "sentences";

export function TopicDetail({
  progress,
  initialSection,
  isFocus,
  onBack,
  onPractise,
  onFocus,
  onDeleted,
}: {
  progress: TopicProgress;
  /** Which tab to land on — the Library deep-links straight to one. */
  initialSection?: TopicSection;
  /** True when the daily check-in is focused on this topic. */
  isFocus: boolean;
  onBack: () => void;
  onPractise: () => void;
  onFocus: (focused: boolean) => Promise<void>;
  onDeleted: () => void;
}) {
  const [tab, setTab] = useState<TopicSection>(initialSection ?? "conversation");
  const [lines, setLines] = useState<DialogueLine[] | null>(null);
  const [words, setWords] = useState<Word[] | null>(null);
  const [sentences, setSentences] = useState<Sentence[] | null>(null);
  const [query, setQuery] = useState("");
  const [revealAll, setRevealAll] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"focus" | "delete" | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const topicId = progress.topic.id;
  const programme = progress.topic.kind === "programme";

  // Programmes may have no dialogue; then there is no conversation tab.
  useEffect(() => {
    if (lines && lines.length === 0 && tab === "conversation") setTab("words");
  }, [lines, tab]);

  async function act(kind: "focus" | "delete", action: () => Promise<void>) {
    setBusy(kind);
    setError(null);
    try {
      await action();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }

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
    <div className="scroll no-nav">
      <div className="row" style={{ marginBottom: 14 }}>
        <button className="icon-btn" onClick={onBack} aria-label="Back">
          ‹
        </button>
        <div className="grow">
          <h1 style={{ marginBottom: 0, fontSize: 21 }}>{progress.topic.titlePl}</h1>
          <div className="small faint">{progress.topic.title}</div>
        </div>
      </div>

      {error ? <Banner kind="error">{error}</Banner> : null}

      <div className="row" style={{ gap: 8, marginBottom: 10 }}>
        <button className="btn primary grow" onClick={onPractise}>
          ▶︎ Practise now
        </button>
        <button
          className={`btn small ${isFocus ? "success" : "ghost"}`}
          disabled={busy === "focus"}
          onClick={() => act("focus", () => onFocus(!isFocus))}
        >
          {busy === "focus" ? <Spinner /> : isFocus ? "✓ Daily focus" : "Make daily"}
        </button>
      </div>
      {isFocus ? (
        <p className="tiny faint" style={{ marginTop: 0 }}>
          The daily check-in uses this until you have met all its words.
        </p>
      ) : null}

      <div className="segmented" style={{ marginBottom: 16 }}>
        {lines === null || lines.length > 0 ? (
          <button data-active={tab === "conversation"} onClick={() => setTab("conversation")}>
            Conversation
          </button>
        ) : null}
        <button data-active={tab === "words"} onClick={() => setTab("words")}>
          Words {progress.seenWords}/{progress.totalWords}
        </button>
        <button data-active={tab === "sentences"} onClick={() => setTab("sentences")}>
          Sentences
        </button>
      </div>

      {tab === "conversation" ? (
        lines ? (
          <>
            <p className="small">{progress.topic.summary}</p>
            <DialogueToolbar
              lines={lines}
              revealAll={revealAll}
              onToggleReveal={() => setRevealAll((value) => !value)}
            />
            <DialogueView lines={lines} revealAll={revealAll} />
          </>
        ) : (
          <Loading />
        )
      ) : null}

      {tab === "words" ? (
        words ? (
          <>
            <input
              value={query}
              placeholder="Search these words…"
              onChange={(event) => setQuery(event.target.value)}
              style={{ marginBottom: 12 }}
            />
            <div className="card flush">
              {filtered.map((word) => (
                <div key={word.id} className="list-row">
                  <span className="grow selectable">
                    <span className="pl">{word.pl}</span>
                    <span className="tr">
                      {word.meaning} · {word.pos}
                    </span>
                  </span>
                  <SpeakButton text={word.pl} />
                </div>
              ))}
              {filtered.length === 0 ? (
                <div className="list-row faint small">No match.</div>
              ) : null}
            </div>
          </>
        ) : (
          <Loading />
        )
      ) : null}

      {tab === "sentences" ? (
        sentences ? (
          <div className="card flush">
            {sentences.map((sentence) => (
              <div key={sentence.id} className="list-row">
                <span className="grow selectable">
                  <span className="pl">{sentence.pl}</span>
                  <span className="tr">{sentence.meaning}</span>
                </span>
                <SpeakButton text={sentence.pl} />
              </div>
            ))}
          </div>
        ) : (
          <Loading />
        )
      ) : null}

      {programme ? (
        <div style={{ marginTop: 20 }}>
          {confirmDelete ? (
            <div className="row">
              <button
                className="btn danger grow"
                disabled={busy === "delete"}
                onClick={() =>
                  act("delete", async () => {
                    await api.programmeDelete(topicId);
                    onDeleted();
                  })
                }
              >
                {busy === "delete" ? <Spinner /> : "Yes, delete it and its history"}
              </button>
              <button className="btn ghost" onClick={() => setConfirmDelete(false)}>
                Cancel
              </button>
            </div>
          ) : (
            <button className="btn danger block" onClick={() => setConfirmDelete(true)}>
              Delete programme
            </button>
          )}
        </div>
      ) : null}
    </div>
  );
}
