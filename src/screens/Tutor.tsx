import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { TitleWithInfo } from "../components/Info";
import { Markdown } from "../components/Markdown";
import { Banner, Spinner } from "../components/ui";
import { api, errorMessage } from "../lib/api";
import type { ChatMessage, StreamEvent, TutorAction, TutorStatus } from "../lib/types";

const STARTERS = [
  "Build me a programme for learning the colours",
  "What should I practise today?",
  "Make a programme from the words I keep getting wrong",
  "Make me a song about ordering coffee",
  "When do I use “ty” and when “pan” or “pani”?",
];

let nextStream = 0;

export function Tutor({
  hasApiKey,
  hasModel,
  focusTopicId,
  onOpenSettings,
  onPractise,
  onOpenSong,
  onChanged,
}: {
  hasApiKey: boolean;
  /** A key alone is not enough — a model has to be chosen too. */
  hasModel: boolean;
  focusTopicId: number | null;
  onOpenSettings: () => void;
  /** Starts a practice session for a topic or programme. */
  onPractise: (topicId: number) => void;
  /** Shows a song in the Music tab. */
  onOpenSong: (songId: number) => void;
  /** The tutor changed something in the app; reload what the screens show. */
  onChanged: () => void;
}) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [live, setLive] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [focusing, setFocusing] = useState<number | null>(null);
  const activeStream = useRef<string | null>(null);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.tutorHistory().then(setMessages).catch(() => setMessages([]));
  }, []);

  // Streamed text and "working on it" lines, for the request in flight only.
  useEffect(() => {
    const offText = listen<StreamEvent>("ai://stream", (event) => {
      if (event.payload.id !== activeStream.current) return;
      if (event.payload.delta) {
        setStatus(null);
        setLive((current) => current + event.payload.delta);
      }
    });
    const offStatus = listen<TutorStatus>("tutor://status", (event) => {
      if (event.payload.id === activeStream.current) setStatus(event.payload.label);
    });
    return () => {
      void offText.then((off) => off());
      void offStatus.then((off) => off());
    };
  }, []);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, live, status, sending]);

  async function send(text: string) {
    const message = text.trim();
    if (!message || sending) return;
    const streamId = `tutor${nextStream++}`;
    activeStream.current = streamId;
    setDraft("");
    setError(null);
    setLive("");
    setStatus(null);
    setSending(true);
    // Show the learner's turn immediately; the backend persists both sides.
    setMessages((current) => [
      ...current,
      { id: -Date.now(), role: "user", content: message, actions: [] },
    ]);
    try {
      const reply = await api.tutorSend(message, streamId);
      setMessages((current) => [...current, reply]);
      if (reply.actions.length > 0) onChanged();
    } catch (cause) {
      setError(errorMessage(cause));
      setMessages((current) => current.slice(0, -1));
      setDraft(message);
    } finally {
      activeStream.current = null;
      setLive("");
      setStatus(null);
      setSending(false);
    }
  }

  async function focus(topicId: number) {
    setFocusing(topicId);
    try {
      await api.setFocus(topicId);
      onChanged();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setFocusing(null);
    }
  }

  function ActionRow({ action }: { action: TutorAction }) {
    const topicId = action.topicId;
    const songId = action.songId;
    const practisable =
      topicId !== null &&
      (action.kind === "programmeCreated" ||
        action.kind === "programmeUpdated" ||
        action.kind === "practiceSuggested" ||
        action.kind === "focusSet");
    return (
      <div className="tutor-action">
        <div className="small">
          <span aria-hidden>✓ </span>
          {action.label}
        </div>
        {action.kind === "songStarted" && songId !== null ? (
          <div className="row wrap" style={{ gap: 6, marginTop: 8 }}>
            <button className="btn ghost small" onClick={() => onOpenSong(songId)}>
              ♪ Open in Music
            </button>
          </div>
        ) : null}
        {practisable ? (
          <div className="row wrap" style={{ gap: 6, marginTop: 8 }}>
            <button className="btn primary small" onClick={() => onPractise(topicId)}>
              ▶︎ Practise now
            </button>
            {action.kind === "programmeCreated" && focusTopicId !== topicId ? (
              <button
                className="btn ghost small"
                disabled={focusing === topicId}
                onClick={() => focus(topicId)}
              >
                {focusing === topicId ? <Spinner /> : "Make it my daily check-in"}
              </button>
            ) : null}
          </div>
        ) : null}
      </div>
    );
  }

  if (!hasApiKey || !hasModel) {
    return (
      <div className="scroll">
        <TitleWithInfo as="h1" title="Tutor" info={<TutorInfo />} />
        <p>{hasApiKey ? "Pick a model to start." : "Needs an OpenRouter key."}</p>
        <button className="btn primary block" onClick={onOpenSettings}>
          {hasApiKey ? "Choose a model in Settings" : "Add API key in Settings"}
        </button>
      </div>
    );
  }

  return (
    <div className="scroll">
      <div className="row between" style={{ marginBottom: 4 }}>
        <TitleWithInfo as="h1" title="Tutor" info={<TutorInfo />} />
        {messages.length > 0 ? (
          <button
            className="btn ghost small"
            disabled={sending}
            onClick={async () => {
              await api.tutorClear();
              setMessages([]);
            }}
          >
            Clear
          </button>
        ) : null}
      </div>

      {error ? <Banner kind="error">{error}</Banner> : null}

      {messages.length === 0 && !sending ? (
        <div className="card">
          <p className="small" style={{ marginTop: 0 }}>
            Ask about Polish or the app. I can build practice programmes on any theme,
            check your progress, and set up your daily check-in.
          </p>
          <div className="stack">
            {STARTERS.map((starter) => (
              <button
                key={starter}
                className="btn ghost"
                style={{ textAlign: "left", justifyContent: "flex-start" }}
                onClick={() => send(starter)}
              >
                {starter}
              </button>
            ))}
          </div>
        </div>
      ) : null}

      <div className="chat">
        {messages.map((message) => (
          <div key={message.id} className={`msg ${message.role} selectable`}>
            {message.role === "assistant" ? (
              <>
                <Markdown text={message.content} />
                {message.actions.map((action, i) => (
                  <ActionRow key={i} action={action} />
                ))}
              </>
            ) : (
              message.content
            )}
          </div>
        ))}
        {sending ? (
          <div className="msg assistant">
            {live ? (
              <>
                <Markdown text={live} />
                {status ? null : <span className="caret" />}
              </>
            ) : null}
            {status || !live ? (
              <div className="row small faint" style={{ gap: 8, marginTop: live ? 8 : 0 }}>
                <Spinner />
                {status ?? ""}
              </div>
            ) : null}
          </div>
        ) : null}
        <div ref={endRef} />
      </div>

      <div className="composer" style={{ marginBottom: 12 }}>
        <textarea
          className="grow"
          rows={2}
          value={draft}
          placeholder="Ask the tutor…"
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              void send(draft);
            }
          }}
        />
        <button
          className="btn primary"
          style={{ padding: "14px 18px" }}
          disabled={sending || draft.trim().length === 0}
          onClick={() => send(draft)}
          aria-label="Send"
        >
          ➤
        </button>
      </div>
    </div>
  );
}

function TutorInfo() {
  return (
    <>
      <p>Your assistant for the app. It speaks English and can:</p>
      <ul>
        <li>Build a practice programme on any theme — words, sentences and a dialogue</li>
        <li>Add to, edit or delete your programmes</li>
        <li>Make a programme your daily check-in</li>
        <li>Look at your progress and the words you find hardest</li>
        <li>Make learning songs, and tidy up the ones you don't want</li>
        <li>Explain grammar, words and phrases</li>
      </ul>
      <p>
        It needs a model that can use tools (marked 🛠 in Settings). It runs on your own
        OpenRouter key, which never leaves the app's core.
      </p>
    </>
  );
}
