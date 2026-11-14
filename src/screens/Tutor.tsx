import { useCallback, useEffect, useRef, useState } from "react";

import { CheckIcon, MemoryIcon, MoreIcon, NavIcon, PlayerIcon, SendIcon } from "../components/icons";
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
import { useAiStream } from "../lib/useAiStream";
import type { ChatMessage, Conversation, TutorAction } from "../lib/types";

const STARTERS = [
  "Build me a course for learning the colours",
  "What should I practise today?",
  "Make a course from the words I keep getting wrong",
  "Make me a song about ordering coffee",
  "When do I use “ty” and when “pan” or “pani”?",
];


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
  /** Opens a course page. */
  onOpenTopic: (topicId: number) => void;
  /** Opens a block's page. */
  onOpenBlock: (blockId: number) => void;
  /** Shows a song in the Music tab. */
  onOpenSong: (songId: number) => void;
  /** The tutor changed something in the app; reload what the screens show. */
  onChanged: () => void;
  /** Opens what the agents remember. */
  onOpenMemories: () => void;
};

/** The box a message is typed in, with its send button. */
function Composer({
  draft,
  setDraft,
  disabled,
  onSend,
}: {
  draft: string;
  setDraft: (text: string) => void;
  disabled?: boolean;
  onSend: (text: string) => void;
}) {
  return (
    <div className="composer">
      <textarea
        rows={1}
        value={draft}
        placeholder="Ask the tutor"
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            onSend(draft);
          }
        }}
      />
      <IconButton
        variant="primary"
        icon={<SendIcon />}
        label="Send"
        disabled={disabled || draft.trim().length === 0}
        onClick={() => onSend(draft)}
      />
    </div>
  );
}

/**
 * The Tutor tab: your conversations, newest first, with a box at the bottom
 * — writing in it starts a new conversation.
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
  const [draft, setDraft] = useState("");
  // The message a new conversation opens with, sent as soon as it shows.
  const [opening, setOpening] = useState<string | null>(null);

  function start(text: string) {
    const message = text.trim();
    if (!message) return;
    setDraft("");
    setOpening(message);
    setOpen("new");
  }

  const refresh = useCallback(() => {
    api
      .tutorConversations()
      .then(setConversations)
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
          build courses, lessons and songs for you.
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
        firstMessage={open === "new" ? opening : null}
        onStarted={(id) => {
          setOpening(null);
          setOpen(id);
          refresh();
        }}
        onBack={() => {
          setOpening(null);
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
      footer={<Composer draft={draft} setDraft={setDraft} onSend={start} />}
    >
      {error ? <Banner tone="error">{error}</Banner> : null}
      {conversations === null ? (
        <Spinner />
      ) : conversations.length === 0 ? (
        <Section title="Try asking">
          <List>
            {STARTERS.map((starter) => (
              <ListItem key={starter} title={starter} onClick={() => start(starter)} />
            ))}
          </List>
        </Section>
      ) : (
        <Section title="Conversations">
          <List>
            {conversations.map((conversation) => (
              <ListItem
                key={conversation.id}
                oneLine
                title={conversation.title}
                subtitle={when(conversation.updatedAt)}
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
  firstMessage,
  onStarted,
  onBack,
  onRenamed,
  onDeleted,
  onOpenTopic,
  onOpenBlock,
  onOpenSong,
  onChanged,
  onOpenMemories,
}: Shared & {
  /** null for a conversation not yet started. */
  conversationId: number | null;
  title: string;
  /** Sent once when the conversation opens (typed on the list). */
  firstMessage: string | null;
  /** The first reply arrived; the conversation now exists with this id. */
  onStarted: (id: number) => void;
  onBack: () => void;
  onRenamed: () => void;
  onDeleted: () => void;
}) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const stream = useAiStream();
  const sending = stream.streaming;
  const live = stream.text;
  const status = stream.status;
  const [localError, setError] = useState<string | null>(null);
  const error = localError ?? stream.error;
  const [managing, setManaging] = useState(false);
  const [name, setName] = useState(title);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (conversationId === null) return;
    api
      .tutorHistory(conversationId)
      .then(setMessages)
      .catch((cause) => setError(errorMessage(cause)));
  }, [conversationId]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, live, status, sending]);

  // Send the opening message once — a ref, so React's double effects in
  // development don't send it twice.
  const sentFirst = useRef(false);
  useEffect(() => {
    if (firstMessage && !sentFirst.current) {
      sentFirst.current = true;
      void send(firstMessage);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [firstMessage]);

  async function send(text: string) {
    const message = text.trim();
    if (!message || sending) return;
    setDraft("");
    setError(null);
    // Show the learner's turn immediately; the backend persists both sides.
    setMessages((current) => [
      ...current,
      { id: -Date.now(), conversationId: conversationId ?? 0, role: "user", content: message, actions: [] },
    ]);
    const reply = await stream.run((streamId) => api.tutorSend(message, streamId, conversationId));
    if (reply) {
      stream.reset();
      setMessages((current) => [...current, reply]);
      if (reply.actions.length > 0) onChanged();
      if (conversationId === null) onStarted(reply.conversationId);
    } else {
      setMessages((current) => current.slice(0, -1));
      setDraft(message);
    }
  }

  function ActionRow({ action }: { action: TutorAction }) {
    const { topicId, songId, blockId } = action;
    const opensCourse =
      topicId !== null &&
      ["courseCreated", "courseUpdated", "programmeCreated", "programmeUpdated"].includes(action.kind);
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
        {action.kind === "memory" ? (
          <Inline gap={2} wrap>
            <Button size="sm" icon={<MemoryIcon />} onClick={onOpenMemories}>
              Memories
            </Button>
          </Inline>
        ) : null}
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
        {opensCourse ? (
          <Inline gap={2} wrap>
            <Button
              variant="primary"
              size="sm"
              icon={<PlayerIcon.play size={14} />}
              onClick={() => onOpenTopic(topicId)}
            >
              Open course
            </Button>
          </Inline>
        ) : null}
      </div>
    );
  }

  return (
    <Screen
      title={title}
      onBack={onBack}
      footer={<Composer draft={draft} setDraft={setDraft} disabled={sending} onSend={(text) => void send(text)} />}
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
            question="Delete this conversation? Courses and songs the tutor made stay."
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
        <li>Build a course on any theme — words, sentences and a dialogue, cut into drills</li>
        <li>Write lessons (a conjugation, a grammar point, culture) with a check-in</li>
        <li>Add to, edit or delete any course or block</li>
        <li>Look at your progress and the words you find hardest</li>
        <li>Make learning songs, and tidy up the ones you don't want</li>
        <li>Make playlists and add songs to them, or take them out</li>
        <li>Explain grammar, words and phrases</li>
      </ul>
      <p>
        It needs a model that can use tools (marked 🛠 in Settings). It runs on your own
        OpenRouter key, which never leaves the app's core.
      </p>
    </>
  );
}
