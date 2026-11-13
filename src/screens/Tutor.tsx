import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { CheckIcon, NavIcon, PlayerIcon, SendIcon } from "../components/icons";
import { Markdown } from "../components/Markdown";
import {
  Banner,
  Button,
  EmptyState,
  IconButton,
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  Spinner,
  Text,
} from "../design";
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
        <Inline gap={2} align="top">
          <span className="tone-ok">
            <CheckIcon />
          </span>
          <Text variant="small" weight="medium">
            {action.label}
          </Text>
        </Inline>
        {action.kind === "songStarted" && songId !== null ? (
          <Inline gap={2} wrap>
            <Button size="sm" icon={<NavIcon.songs />} onClick={() => onOpenSong(songId)}>
              Open in Music
            </Button>
          </Inline>
        ) : null}
        {practisable ? (
          <Inline gap={2} wrap>
            <Button
              variant="primary"
              size="sm"
              icon={<PlayerIcon.play size={14} />}
              onClick={() => onPractise(topicId)}
            >
              Practise now
            </Button>
            {action.kind === "programmeCreated" && focusTopicId !== topicId ? (
              <Button size="sm" loading={focusing === topicId} onClick={() => focus(topicId)}>
                Use for daily check-in
              </Button>
            ) : null}
          </Inline>
        ) : null}
      </div>
    );
  }

  if (!hasApiKey || !hasModel) {
    return (
      <Screen title="Tutor" info={<TutorInfo />}>
        <EmptyState
          icon="✨"
          title={hasApiKey ? "Pick a model" : "Add an OpenRouter key"}
          action={
            <Button variant="primary" onClick={onOpenSettings}>
              Open Settings
            </Button>
          }
        >
          The tutor runs on your own OpenRouter account. Choose a model marked 🛠, so it can
          build programmes and songs for you.
        </EmptyState>
      </Screen>
    );
  }

  return (
    <Screen
      title="Tutor"
      info={<TutorInfo />}
      actions={
        messages.length > 0 ? (
          <Button
            variant="plain"
            size="sm"
            disabled={sending}
            onClick={async () => {
              await api.tutorClear();
              setMessages([]);
            }}
          >
            Clear
          </Button>
        ) : null
      }
    >
      {error ? <Banner tone="error">{error}</Banner> : null}

      {messages.length === 0 && !sending ? (
        <Section title="Try asking">
          <List>
            {STARTERS.map((starter) => (
              <ListItem key={starter} title={starter} onClick={() => send(starter)} />
            ))}
          </List>
        </Section>
      ) : null}

      <div className="chat">
        {messages.map((message) => (
          <div key={message.id} className="msg selectable" data-role={message.role}>
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
          <div className="msg" data-role="assistant">
            {live ? (
              <div>
                <Markdown text={live} />
                {status ? null : <span className="caret" />}
              </div>
            ) : null}
            {status || !live ? (
              <Inline gap={2}>
                <Spinner />
                <Text variant="small" tone="faint">
                  {status ?? "Thinking…"}
                </Text>
              </Inline>
            ) : null}
          </div>
        ) : null}
        <div ref={endRef} />
      </div>

      <div className="composer composer-bar">
        <textarea
          rows={1}
          value={draft}
          placeholder="Ask the tutor"
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              void send(draft);
            }
          }}
        />
        <IconButton
          variant="primary"
          icon={<SendIcon />}
          label="Send"
          disabled={sending || draft.trim().length === 0}
          onClick={() => send(draft)}
        />
      </div>
    </Screen>
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
