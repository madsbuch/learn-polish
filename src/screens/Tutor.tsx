import { useCallback, useEffect, useRef, useState } from "react";

import { CheckIcon, MemoryIcon, MoreIcon, NavIcon, PlayerIcon, PlusIcon } from "../components/icons";
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
  Segmented,
  Spinner,
  Typing,
  SplitView,
  Stack,
  Text,
  useWideLayout,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { useDraft, writeDraft } from "../lib/drafts";
import { useStickToBottom } from "../lib/stickToBottom";
import { JumpToLatest } from "../components/JumpToLatest";
import { useTutorJob } from "../lib/tutorJob";
import { ensurePermission } from "../lib/notifications";
import type { ChatMessage, Conversation, ConversationId, CourseId, LessonId, SongId, TutorAction } from "../lib/schemas";
import { onEvent } from "../lib/events";
import { TutorDoneSchema } from "../lib/schemas";
import { useSyncReload } from "../lib/sync";
import { currentAddress, go, goUp, handoffSlot, swapLast, useEntryKey } from "../lib/navigation";

/** A message as shown: stored, or the learner's just sent (not stored yet). */
type Shown = Omit<ChatMessage, "id" | "conversationId"> & {
  id: ChatMessage["id"] | `pending-${number}`;
  conversationId: ConversationId | null;
};

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
  onOpenCourse: (courseId: CourseId) => void;
  /** Opens a lesson's page. */
  onOpenLesson: (lessonId: LessonId) => void;
  /** Shows a song in the Music tab. */
  onOpenSong: (songId: SongId) => void;
  /** The tutor changed something in the app; reload what the screens show. */
  onChanged: () => void;
  /** Opens what the agents remember. */
  onOpenMemories: () => void;
};

/**
 * The list as last loaded, kept while the app runs: leaving the tab and
 * coming back shows it at once (the list then refreshes), never a spinner
 * behind a slow database.
 */
let cachedConversations: Conversation[] | null = null;

/** The message a new conversation opens with, handed to its page (never in its address). */
export const tutorAsks = handoffSlot<string>();

/** The conversations, loaded and kept fresh: a reply finishing in the background changes the list. */
function useConversations(): {
  conversations: Conversation[] | null;
  error: string | null;
  refresh: () => void;
  /** Which refresh the list shown came from, counting up. */
  loaded: number;
  /** Which refresh started last. */
  started: () => number;
} {
  const [conversations, setConversations] = useState<Conversation[] | null>(cachedConversations);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(0);
  const startedRef = useRef(0);
  const refresh = useCallback(() => {
    const round = ++startedRef.current;
    api
      .tutorConversations()
      .then((list) => {
        cachedConversations = list;
        setConversations(list);
        setLoaded(round);
        setError(null);
      })
      .catch((cause) => setError(errorMessage(cause)));
  }, []);
  const started = useCallback(() => startedRef.current, []);
  useEffect(refresh, [refresh]);
  // Conversations from another device (sync).
  useSyncReload(refresh);
  useEffect(() => {
    const off = onEvent(TutorDoneSchema, "tutor://done", () => refresh());
    return () => {
      void off.then((stop) => stop());
    };
  }, [refresh]);
  return { conversations, error, refresh, loaded, started };
}

/**
 * A conversation on a page of its own: in the Tutor tab, or opened from
 * another page ("ask the tutor" in Insights), above it. `new` is a blank one
 * that opens with the message handed to its page, if any, and takes its id
 * (in place, in the address) once the first reply names it.
 */
function ChatPage({
  target,
  inSplit,
  list,
  ...shared
}: Shared & {
  target: ConversationId | "new";
  /** Beside the list: no way back, the list stays in view. */
  inSplit: boolean;
  list: ReturnType<typeof useConversations>;
}) {
  const { conversations, refresh } = list;
  const entry = useEntryKey();
  const conversation = target === "new" ? null : (conversations?.find((c) => c.id === target) ?? null);
  // One deleted elsewhere (another device): the page gives way — judged by a
  // list asked for after the conversation was opened, never an older one.
  // Spec: SHELL-26
  const [since, setSince] = useState(list.started);
  const [shownTarget, setShownTarget] = useState(target);
  if (shownTarget !== target) {
    setShownTarget(target);
    setSince(list.started());
  }
  const gone = target !== "new" && list.loaded > since && conversation === null;
  useEffect(() => {
    if (gone) goUp();
  }, [gone]);
  return (
    <Chat
      key={target}
      conversationId={target === "new" ? null : target}
      title={conversation?.title ?? "New conversation"}
      firstMessage={target === "new" ? (tutorAsks.get(entry) ?? null) : null}
      onFirstSent={() => tutorAsks.drop(entry)}
      onStarted={(id) => {
        // The new conversation's page takes its id, in place.
        // Spec: SHELL-16
        const here = currentAddress();
        const last = here.trail[here.trail.length - 1];
        if (last?.kind === "chat" && last.id === "new") go(swapLast(here, { kind: "chat", id }));
        else if (here.tab === "tutor" && here.trail.length === 0) go({ tab: "tutor", trail: [{ kind: "chat", id }] });
        refresh();
      }}
      onBack={inSplit ? undefined : goUp}
      onRenamed={refresh}
      onDeleted={() => {
        goUp();
        refresh();
      }}
      {...shared}
    />
  );
}

/** A conversation opened from a page outside the Tutor tab. */
export function TutorChat({ target, ...shared }: Shared & { target: ConversationId | "new" }) {
  const list = useConversations();
  return <ChatPage target={target} inSplit={false} list={list} {...shared} />;
}

/** Shows a conversation in the Tutor tab ("new", or none for the list). */
function openInTab(target: ConversationId | "new" | null, ask?: string): void {
  go(
    { tab: "tutor", trail: target === null ? [] : [{ kind: "chat", id: target }] },
    ask === undefined ? {} : { handoff: (entry) => tutorAsks.set(entry, ask) },
  );
}

/**
 * The Tutor tab: your conversations, newest first, with a box at the bottom
 * — writing in it starts a new conversation. The conversation open is the
 * address's (`/tutor/cv_…`).
 */
export function Tutor({
  open,
  hasApiKey,
  hasModel,
  onOpenSettings,
  ...shared
}: Shared & {
  /** The conversation open: null for the list. */
  open: ConversationId | "new" | null;
  hasApiKey: boolean;
  /** A key alone is not enough — a model has to be chosen too. */
  hasModel: boolean;
  onOpenSettings: () => void;
}) {
  const conversationList = useConversations();
  const { conversations, error } = conversationList;
  const [draft, setDraft] = useDraft("list");
  // All conversations, or only the ones about a word ("Talk about it").
  const [filter, setFilter] = useState<"all" | "word">("all");
  const wide = useWideLayout();

  function start(text: string) {
    const message = text.trim();
    if (!message) return;
    setDraft("");
    openInTab("new", message);
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
          The tutor runs on your own OpenRouter account. Choose a model marked 🛠, so it can build courses, lessons and
          songs for you.
        </EmptyState>
      </Screen>
    );
  }

  const chat = (target: ConversationId | "new", inSplit: boolean) => (
    <ChatPage target={target} inSplit={inSplit} list={conversationList} {...shared} />
  );

  // A tablet in landscape: the list, and the open conversation beside it —
  // a new one when none is open. The address is the same at every width.
  // Spec: SHELL-24
  if (wide) {
    const current = open ?? "new";
    return <SplitView list={list(current)} detail={chat(current, true)} />;
  }
  if (open !== null) return chat(open, false);
  return list(null);

  /** The conversations; `current` is the one open beside them (wide windows). */
  function list(current: ConversationId | "new" | null) {
    const inSplit = current !== null;
    return (
      <Screen
        title="Tutor"
        info={<TutorInfo />}
        footer={
          inSplit ? undefined : (
            <Composer draft={draft} setDraft={setDraft} placeholder="Ask the tutor" dictate onSend={start} />
          )
        }
        actions={
          inSplit ? (
            <IconButton
              variant={current === "new" ? "primary" : undefined}
              icon={<PlusIcon />}
              label="New conversation"
              onClick={() => openInTab(null)}
            />
          ) : null
        }
      >
        {error ? <Banner tone="error">{error}</Banner> : null}
        {conversations === null ? (
          <Spinner />
        ) : conversations.length === 0 && inSplit ? (
          <Text variant="small" tone="dim">
            No conversations yet — ask anything on the right.
          </Text>
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
            {conversations.some((c) => c.tag === "word") ? (
              <Segmented
                value={filter}
                options={[
                  { value: "all", label: "All" },
                  { value: "word", label: "Words" },
                ]}
                onChange={setFilter}
              />
            ) : null}
            <List>
              {conversations
                .filter((c) => filter === "all" || c.tag === filter)
                .map((conversation) => (
                  <ListItem
                    key={conversation.id}
                    oneLine
                    title={conversation.tag === "word" ? `💬 ${conversation.title}` : conversation.title}
                    subtitle={
                      conversation.working
                        ? "The tutor is writing…"
                        : [
                            conversation.tag === "word" ? "Word" : null,
                            when(conversation.updatedAt),
                            conversation.error ? "didn't finish" : null,
                          ]
                            .filter(Boolean)
                            .join(" · ")
                    }
                    selected={current === conversation.id}
                    onClick={() => openInTab(conversation.id)}
                  />
                ))}
            </List>
          </Section>
        )}
      </Screen>
    );
  }
}

function Chat({
  conversationId,
  title,
  firstMessage,
  onFirstSent,
  onStarted,
  onBack,
  onRenamed,
  onDeleted,
  onOpenCourse,
  onOpenLesson,
  onOpenSong,
  onOpenMemories,
}: Shared & {
  /** null for a conversation not yet started. */
  conversationId: ConversationId | null;
  title: string;
  /** Sent once when the conversation opens (typed on the list). */
  firstMessage: string | null;
  /** The first message is on its way: it is not to be sent again. */
  onFirstSent: () => void;
  /** The first reply arrived; the conversation now exists with this id. */
  onStarted: (id: ConversationId) => void;
  /** Back to the list; left out beside it, where the list stays in view. */
  onBack?: () => void;
  onRenamed: () => void;
  onDeleted: () => void;
}) {
  const [messages, setMessages] = useState<Shown[]>([]);
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
  // Stop was tapped; the reply ends at the next point it can.
  const [stopping, setStopping] = useState(false);
  useEffect(() => {
    if (!sending) setStopping(false);
  }, [sending]);
  const live = job.text;
  const status = job.status;
  const [localError, setError] = useState<string | null>(null);
  const error = localError ?? job.error;
  const [managing, setManaging] = useState(false);
  const [name, setName] = useState(title);

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

  // Follows the reply as it comes in — until the learner scrolls up to read.
  const scroll = useStickToBottom([messages, live, status, sending]);
  const endRef = scroll.endRef;

  // Send the opening message once — a ref, so React's double effects in
  // development don't send it twice.
  const sentFirst = useRef(false);
  useEffect(() => {
    if (firstMessage && !sentFirst.current) {
      sentFirst.current = true;
      onFirstSent();
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
      { id: `pending-${Date.now()}`, conversationId, role: "user", content: message, actions: [] },
    ]);
    scroll.follow();
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
    const { courseId, songId, lessonId } = action;
    const opensCourse =
      courseId !== null &&
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
              Open the song
            </Button>
          </Inline>
        ) : null}
        {lessonId !== null && action.kind !== "lessonDeleted" ? (
          <Inline gap={2} wrap>
            <Button
              variant="primary"
              size="sm"
              icon={<PlayerIcon.play size={14} />}
              onClick={() => onOpenLesson(lessonId)}
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
              onClick={() => onOpenCourse(courseId)}
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
          dictate
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
          <div
            key={message.id}
            className="msg selectable"
            data-role={message.role}
            // Just sent: it rises into place (history and stored replies don't).
            data-fresh={(typeof message.id === "string" && message.id.startsWith("pending-")) || undefined}
          >
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
          <div className="msg" data-role="assistant" data-fresh>
            {live ? (
              <div>
                <Markdown text={live} />
                {status ? null : <span className="caret" />}
              </div>
            ) : null}
            {status || !live ? (
              <Inline gap={2}>
                <Typing />
                <Text variant="small" tone="faint">
                  {status ?? "Thinking…"}
                </Text>
              </Inline>
            ) : null}
            {conversationId !== null ? (
              // Spec: counsel.AGT-2
              <Button
                size="sm"
                variant="plain"
                loading={stopping}
                onClick={() => {
                  setStopping(true);
                  api.tutorStop(conversationId).catch((cause: unknown) => {
                    setStopping(false);
                    setError(errorMessage(cause));
                  });
                }}
              >
                Stop
              </Button>
            ) : null}
          </div>
        ) : null}
        <div ref={endRef} />
      </div>
      {scroll.away ? <JumpToLatest onClick={scroll.follow} /> : null}

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
            wide
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
        <li>Write study lessons (a conjugation, a grammar point, culture), then talk them through</li>
        <li>Add to, edit or delete any course or lesson</li>
        <li>Look at your progress and the words you find hardest</li>
        <li>Make learning songs, and tidy up the ones you don't want</li>
        <li>Make playlists and add songs to them, or take them out</li>
        <li>Explain grammar, words and phrases</li>
      </ul>
      <p>
        It needs a model that can use tools (marked 🛠 in Settings → AI). It runs on your own OpenRouter key, which never
        leaves the app's core.
      </p>
    </>
  );
}
