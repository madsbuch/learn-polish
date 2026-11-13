import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { CheckIcon, MoreIcon, NavIcon, PlayerIcon, PlusIcon, SendIcon } from "../components/icons";
import { Markdown } from "../components/Markdown";
import {
  Banner,
  Button,
  ConfirmButton,
  EmptyState,
  Field,
  Sheet,
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
import type { ChatMessage, Conversation, StreamEvent, TutorAction, TutorStatus } from "../lib/types";

const STARTERS = [
  "Build me a programme for learning the colours",
  "What should I practise today?",
  "Make a programme from the words I keep getting wrong",
  "Make me a song about ordering coffee",
  "When do I use “ty” and when “pan” or “pani”?",
];

let nextStream = 0;

/** "3 min ago", "yesterday", "12 Nov" — for the conversation list. */
function when(seconds: number): string {
  const elapsed = Date.now() / 1000 - seconds;
  if (elapsed < 60) return "now";
  if (elapsed < 3600) return `${Math.floor(elapsed / 60)} min ago`;
  if (elapsed < 86_400) return `${Math.floor(elapsed / 3600)} h ago`;
  if (elapsed < 2 * 86_400) return "yesterday";
  return new Date(seconds * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

type Shared = {
  /** Opens a topic or programme page. */
  onOpenTopic: (topicId: number) => void;
  /** Opens a block's page. */
  onOpenBlock: (blockId: number) => void;
  /** Shows a song in the Music tab. */
  onOpenSong: (songId: number) => void;
  /** The tutor changed something in the app; reload what the screens show. */
  onChanged: () => void;
};

/**
 * The Tutor tab: your conversations, newest first. Each keeps its own
 * history; a new one starts from a blank page.
 */
export function Tutor({
  hasApiKey,
  hasModel,
  onOpenSettings,
  ...shared
}: Shared & {
  hasApiKey: boolean;
  /** A key alone is not enough — a model has to be chosen too. */
  hasModel: boolean;
  onOpenSettings: () => void;
}) {
  // null: the list; "new": a blank conversation; a number: that conversation.
  const [open, setOpen] = useState<number | "new" | null>(null);
  const [conversations, setConversations] = useState<Conversation[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api
      .tutorConversations()
      .then((list) => {
        setConversations(list);
        // With nothing to list, go straight to a new conversation.
        if (list.length === 0) setOpen((current) => current ?? "new");
      })
      .catch((cause) => setError(errorMessage(cause)));
  }, []);

  useEffect(refresh, [refresh]);

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

  if (open !== null) {
    const conversation =
      open === "new" ? null : (conversations?.find((entry) => entry.id === open) ?? null);
    return (
      <Chat
        key={open}
        conversationId={open === "new" ? null : open}
        title={conversation?.title ?? "New conversation"}
        canGoBack={(conversations?.length ?? 0) > 0 || open !== "new"}
        onStarted={(id) => {
          setOpen(id);
          refresh();
        }}
        onBack={() => {
          setOpen(null);
          refresh();
        }}
        onRenamed={refresh}
        onDeleted={() => {
          setOpen(null);
          refresh();
        }}
        {...shared}
      />
    );
  }

  return (
    <Screen
      title="Tutor"
      info={<TutorInfo />}
      actions={
        <IconButton
          variant="primary"
          icon={<PlusIcon />}
          label="New conversation"
          onClick={() => setOpen("new")}
        />
      }
    >
      {error ? <Banner tone="error">{error}</Banner> : null}
      {conversations === null ? (
        <Spinner />
      ) : (
        <Section title="Conversations">
          <List>
            {conversations.map((conversation) => (
              <ListItem
                key={conversation.id}
                title={conversation.title}
                subtitle={`${when(conversation.updatedAt)} · ${conversation.preview}`}
                onClick={() => setOpen(conversation.id)}
              />
            ))}
          </List>
        </Section>
      )}
    </Screen>
  );
}

function Chat({
  conversationId,
  title,
  canGoBack,
  onStarted,
  onBack,
  onRenamed,
  onDeleted,
  onOpenTopic,
  onOpenBlock,
  onOpenSong,
  onChanged,
}: Shared & {
  /** null for a conversation not yet started. */
  conversationId: number | null;
  title: string;
  canGoBack: boolean;
  /** The first reply arrived; the conversation now exists with this id. */
  onStarted: (id: number) => void;
  onBack: () => void;
  onRenamed: () => void;
  onDeleted: () => void;
}) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [live, setLive] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [managing, setManaging] = useState(false);
  const [name, setName] = useState(title);
  const activeStream = useRef<string | null>(null);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (conversationId === null) return;
    api
      .tutorHistory(conversationId)
      .then(setMessages)
      .catch((cause) => setError(errorMessage(cause)));
  }, [conversationId]);

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
      { id: -Date.now(), conversationId: conversationId ?? 0, role: "user", content: message, actions: [] },
    ]);
    try {
      const reply = await api.tutorSend(message, streamId, conversationId);
      setMessages((current) => [...current, reply]);
      if (reply.actions.length > 0) onChanged();
      if (conversationId === null) onStarted(reply.conversationId);
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

  function ActionRow({ action }: { action: TutorAction }) {
    const { topicId, songId, blockId } = action;
    const opensProgramme =
      topicId !== null && (action.kind === "programmeCreated" || action.kind === "programmeUpdated");
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
        {blockId !== null && action.kind !== "blockDeleted" ? (
          <Inline gap={2} wrap>
            <Button
              variant="primary"
              size="sm"
              icon={<PlayerIcon.play size={14} />}
              onClick={() => onOpenBlock(blockId)}
            >
              Open
            </Button>
          </Inline>
        ) : null}
        {opensProgramme ? (
          <Inline gap={2} wrap>
            <Button
              variant="primary"
              size="sm"
              icon={<PlayerIcon.play size={14} />}
              onClick={() => onOpenTopic(topicId)}
            >
              Open programme
            </Button>
          </Inline>
        ) : null}
      </div>
    );
  }

  return (
    <Screen
      title={title}
      onBack={canGoBack ? onBack : undefined}
      footer={
        <div className="composer">
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
      }
      actions={
        conversationId !== null ? (
          <IconButton icon={<MoreIcon />} label="Rename or delete" onClick={() => setManaging(true)} />
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

      {managing && conversationId !== null ? (
        <Sheet title="Conversation" onClose={() => setManaging(false)}>
          <Field label="Title">
            <div className="composer">
              <input value={name} onChange={(event) => setName(event.target.value)} />
              <Button
                variant="primary"
                disabled={!name.trim() || name.trim() === title}
                onClick={async () => {
                  try {
                    await api.tutorRename(conversationId, name);
                    onRenamed();
                    setManaging(false);
                  } catch (cause) {
                    setError(errorMessage(cause));
                  }
                }}
              >
                Save
              </Button>
            </div>
          </Field>
          <ConfirmButton
            label="Delete conversation"
            confirmLabel="Delete"
            question="Delete this conversation? Programmes and songs the tutor made stay."
            block
            onConfirm={async () => {
              await api.tutorDelete(conversationId);
              onDeleted();
            }}
          />
        </Sheet>
      ) : null}

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
