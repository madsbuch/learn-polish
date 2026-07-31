import { useEffect, useRef, useState } from "react";

import { Banner, Spinner } from "../components/ui";
import { api, errorMessage } from "../lib/api";
import { speakQuietly } from "../lib/speech";
import type { ChatMessage, Topic } from "../lib/types";

const STARTERS = [
  "Cześć! Możemy porozmawiać?",
  "Zadaj mi pytanie po polsku.",
  "Poprawiaj moje błędy, proszę.",
];

export function Tutor({
  topic,
  hasApiKey,
  hasModel,
  onOpenSettings,
}: {
  topic: Topic;
  hasApiKey: boolean;
  /** A key alone is not enough — a model has to be chosen too. */
  hasModel: boolean;
  onOpenSettings: () => void;
}) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [drill, setDrill] = useState<string | null>(null);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.aiHistory(topic.id).then(setMessages).catch(() => setMessages([]));
  }, [topic.id]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, sending]);

  async function send(text: string) {
    const message = text.trim();
    if (!message || sending) return;
    setDraft("");
    setError(null);
    setSending(true);
    // Show the learner's turn immediately; the backend persists both sides.
    setMessages((current) => [...current, { role: "user", content: message }]);
    try {
      const reply = await api.aiChat(topic.id, message);
      setMessages((current) => [...current, { role: "assistant", content: reply }]);
    } catch (cause) {
      setError(errorMessage(cause));
      setMessages((current) => current.slice(0, -1));
      setDraft(message);
    } finally {
      setSending(false);
    }
  }

  async function buildDrill() {
    setError(null);
    setDrill(null);
    setSending(true);
    try {
      setDrill(await api.aiDrill());
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setSending(false);
    }
  }

  if (!hasApiKey || !hasModel) {
    return (
      <div className="scroll">
        <h1>AI tutor</h1>
        <p>
          {hasApiKey
            ? "Your key is saved — now pick a model in Settings and the tutor is ready."
            : "Add an OpenRouter API key and this becomes a Polish conversation partner that knows exactly which words you are working on."}
        </p>
        <div className="card">
          <h3>What you get</h3>
          <ul className="muted small" style={{ paddingLeft: 18, lineHeight: 1.7 }}>
            <li>Free conversation in Polish, on today's topic</li>
            <li>Gentle corrections of what you write</li>
            <li>Grammar explanations for any word or sentence</li>
            <li>Extra drills built from the words you keep forgetting</li>
          </ul>
          <p className="tiny faint">
            Every request is made from the app's Rust core, so your key never touches the
            web layer and there are no cross-origin calls.
          </p>
        </div>
        <button className="btn primary block" onClick={onOpenSettings}>
          {hasApiKey ? "Choose a model in Settings" : "Add API key in Settings"}
        </button>
      </div>
    );
  }

  return (
    <div className="scroll">
      <div className="row between" style={{ marginBottom: 4 }}>
        <h1 style={{ marginBottom: 0 }}>AI tutor</h1>
        <button
          className="btn ghost small"
          onClick={async () => {
            await api.aiClearChat(topic.id);
            setMessages([]);
            setDrill(null);
          }}
        >
          Clear
        </button>
      </div>
      <p className="small">
        Topic: <b className="muted">{topic.titlePl}</b> — {topic.title}
      </p>

      {error ? <Banner kind="error">{error}</Banner> : null}

      {messages.length === 0 ? (
        <div className="card">
          <h3>Start with…</h3>
          <div className="stack">
            {STARTERS.map((starter) => (
              <button key={starter} className="btn ghost" onClick={() => send(starter)}>
                {starter}
              </button>
            ))}
          </div>
        </div>
      ) : null}

      <div className="chat">
        {messages.map((message, i) => (
          <div key={i} className={`msg ${message.role} selectable`}>
            {message.content}
            {message.role === "assistant" ? (
              <div style={{ marginTop: 8 }}>
                <button
                  className="btn ghost small"
                  onClick={() => speakQuietly(message.content.replace(/\([^)]*\)/g, ""))}
                >
                  🔊 Listen
                </button>
              </div>
            ) : null}
          </div>
        ))}
        {sending ? (
          <div className="msg assistant">
            <Spinner />
          </div>
        ) : null}
        <div ref={endRef} />
      </div>

      {drill ? (
        <div className="card">
          <h3>Drill from your weakest words</h3>
          <div className="ai-output selectable">{drill}</div>
        </div>
      ) : null}

      <div className="composer" style={{ marginBottom: 12 }}>
        <textarea
          className="grow"
          rows={2}
          value={draft}
          placeholder="Napisz coś po polsku…"
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
        >
          ➤
        </button>
      </div>

      <button className="btn ghost block" disabled={sending} onClick={buildDrill}>
        🎯 Build a drill from my weakest words
      </button>
    </div>
  );
}
