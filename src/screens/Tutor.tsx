import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { CheckIcon, MemoryIcon, MoreIcon, NavIcon, PlayerIcon } from "../components/icons";
import { Composer } from "../components/Composer";
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
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { useDraft, writeDraft } from "../lib/drafts";
import { ensurePermission } from "../lib/notifications";
import type { AgentStatus, ChatMessage, Conversation, StreamEvent, TutorAction, TutorDone } from "../lib/types";

const STARTERS = [
  "Build me a course for learning the colours",
  "What should I practise today?",
  "Make a course from the words I keep getting wrong",
  "Make me a song about ordering coffee",
  "When do I use “ty” and when “pan” or “pani”?",
];


/**
 * Follows a conversation's background reply: whether the tutor is working,
 * the text so far, what a tool is doing, and a failure. `finished` counts
 * replies that ended while watching, so the caller can reload.
 */
function useTutorJob(conversationId: number | null) {
  const [working, setWorking] = useState(false);
  const [text, setText] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [finished, setFinished] = useState(0);

  // Opening a conversation the tutor is still answering (or failed on).
  useEffect(() => {
    if (conversationId === null) return;
    api
      .tutorConversations()
      .then((list) => {
        const entry = list.find((c) => c.id === conversationId);
        if (!entry) return;
        if (entry.working) setWorking(true);
        setError(entry.error);
      })
      .catch(() => {});
  }, [conversationId]);

  useEffect(() => {
    if (conversationId === null) return;
    const id = `tutor-${conversationId}`;
    const offs = [
      listen<StreamEvent>("ai://stream", (event) => {
        if (event.payload.id !== id || !event.payload.delta) return;
        setWorking(true);
        setStatus(null);
        setText((current) => current + event.payload.delta);
      }),
      listen<AgentStatus>("agent://status", (event) => {
        if (event.payload.id !== id) return;
        setWorking(true);
        setStatus(event.payload.label);
      }),
      listen<TutorDone>("tutor://done", (event) => {
        if (event.payload.conversationId !== conversationId) return;
        setWorking(false);
        setText("");
        setStatus(null);
        setError(event.payload.error);
        setFinished((n) => n + 1);
      }),
    ];
    return () => {
      for (const off of offs) void off.then((stop) => stop());
    };
  }, [conversationId]);

  return {
    working,
    text,
    status,
    error,
    finished,
    /** A message was sent: the reply is on its way. */
    begin: () => {
      setWorking(true);
      setText("");
      setStatus(null);
      setError(null);
    },
    /** Sending failed: nothing is on its way after all. */
    cancel: () => setWorking(false),
  };
}

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
  const [draft, setDraft] = useDraft("list");
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
  // A reply finished in the background: the list's "writing…" changes.
  useEffect(() => {
    const off = listen<TutorDone>("tutor://done", () => refresh());
    return () => {
      void off.then((stop) => stop());
    };
  }, [refresh]);

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
      footer={<Composer draft={draft} setDraft={setDraft} placeholder="Ask the tutor" onSend={start} />}
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
                subtitle={
                  conversation.working
                    ? "The tutor is writing…"
                    : conversation.error
                      ? `${when(conversation.updatedAt)} · didn't finish`
                      : when(conversation.updatedAt)
                }
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
  // What is being written survives leaving the conversation, and keeps
  // going while the tutor answers.
  const [draft, setDraft] = useDraft(conversationId === null ? "new" : String(conversationId));
  const draftRef = useRef(draft);
  draftRef.current = draft;
  // The reply is written in the background (the app can be left meanwhile):
  // it streams under the conversation's own id, and `tutor://done` says it
  // is stored.
  const job = useTutorJob(conversationId);
  const sending = job.working;
  const live = job.text;
  const status = job.status;
  const [localError, setError] = useState<string | null>(null);
  const error = localError ?? job.error;
  const [managing, setManaging] = useState(false);
  const [name, setName] = useState(title);
  const endRef = useRef<HTMLDivElement>(null);

  const loadHistory = useCallback(() => {
    if (conversationId === null) return;
    api
      .tutorHistory(conversationId)
      .then(setMessages)
      .catch((cause) => setError(errorMessage(cause)));
  }, [conversationId]);
  useEffect(loadHistory, [loadHistory]);
  // A reply finished: it is stored, so read the conversation back.
  useEffect(() => {
    if (job.finished > 0) loadHistory();
  }, [job.finished, loadHistory]);

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
    // Only clear the box if it still holds what is being sent (a starter or
    // the opening message leave it alone).
    if (draftRef.current.trim() === message) setDraft("");
    setError(null);
    // Show the learner's turn immediately; the backend has stored it by the
    // time tutorSend returns, and answers in the background.
    setMessages((current) => [
      ...current,
      { id: -Date.now(), conversationId: conversationId ?? 0, role: "user", content: message, actions: [] },
    ]);
    job.begin();
    // So a reply that finishes while the app is away can say so.
    void ensurePermission().catch(() => false);
    try {
      const started = await api.tutorSend(message, conversationId);
      if (conversationId === null) {
        // Whatever was written meanwhile goes with the conversation.
        writeDraft(String(started.conversationId), draftRef.current);
        writeDraft("new", "");
        onStarted(started.conversationId);
      }
    } catch (cause) {
      job.cancel();
      setError(errorMessage(cause));
      setMessages((current) => current.slice(0, -1));
      if (!draftRef.current.trim()) setDraft(message);
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
      footer={
        <Composer
          draft={draft}
          setDraft={setDraft}
          placeholder="Ask the tutor"
          canSend={!sending}
          onSend={(text) => void send(text)}
        />
      }
      actions={
        conversationId !== null ? (
          <IconButton icon={<MoreIcon />} label="Rename or delete" onClick={() => setManaging(true)} />
        ) : null
      }
    >
      {error ? (
        <Banner tone="error">
          <Stack gap={2}>
            <span>{error}</span>
            {job.error && conversationId !== null ? (
              <div>
                <Button
                  size="sm"
                  onClick={async () => {
                    job.begin();
                    try {
                      await api.tutorRetry(conversationId);
                    } catch (cause) {
                      job.cancel();
                      setError(errorMessage(cause));
                    }
                  }}
                >
                  Try again
                </Button>
              </div>
            ) : null}
          </Stack>
        </Banner>
      ) : null}

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
