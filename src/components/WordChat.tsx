import { useCallback, useEffect, useRef, useState } from "react";

import { Banner, Inline, Sheet, Spinner, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { useTutorJob } from "../lib/tutorJob";
import type { ChatMessage } from "../lib/types";
import { Composer } from "./Composer";
import { Markdown } from "./Markdown";

/**
 * A chat with the tutor about a word or sentence just practised: how it is
 * really used, and the learner trying it out. It is an ordinary tutor
 * conversation, tagged "word" — saved, listed in the Tutor tab, and carried
 * on the next time the same word is talked about. The tutor writes in the
 * background, so closing the sheet loses nothing.
 */
export function WordChat({
  itemKind,
  itemId,
  title,
  onClose,
}: {
  itemKind: string;
  itemId: number;
  /** The Polish, shown as the sheet's title. */
  title: string;
  onClose: () => void;
}) {
  const [conversationId, setConversationId] = useState<number | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const job = useTutorJob(conversationId);
  const endRef = useRef<HTMLDivElement>(null);

  // Open (or find) the conversation about this word.
  const opened = useRef(false);
  useEffect(() => {
    if (opened.current) return;
    opened.current = true;
    api
      .tutorTalkAbout(itemKind, itemId)
      .then((talk) => {
        if (!talk.existing) job.begin();
        setConversationId(talk.conversationId);
      })
      .catch((cause) => setError(errorMessage(cause)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const load = useCallback(() => {
    if (conversationId === null) return;
    api
      .tutorHistory(conversationId)
      .then(setMessages)
      .catch((cause) => setError(errorMessage(cause)));
  }, [conversationId]);

  useEffect(load, [load, job.finished]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [messages, job.text]);
  // The keyboard opening makes the sheet shorter: keep the newest in view.
  useEffect(() => {
    const keep = () => endRef.current?.scrollIntoView({ block: "end" });
    window.addEventListener("resize", keep);
    return () => window.removeEventListener("resize", keep);
  }, []);

  async function send(text: string) {
    const message = text.trim();
    if (!message || job.working || conversationId === null) return;
    setDraft("");
    job.begin();
    try {
      const started = await api.tutorSend(message, conversationId);
      setMessages((current) => [...current, started.message]);
    } catch (cause) {
      job.cancel();
      setDraft(message);
      setError(errorMessage(cause));
    }
  }

  // The opening "Let's talk about …" is the app's, not the learner's: not shown.
  const shown = messages[0]?.role === "user" ? messages.slice(1) : messages;

  return (
    <Sheet
      title={title}
      onClose={onClose}
      footer={
        <>
          {error || job.error ? <Banner tone="error">{error ?? job.error}</Banner> : null}
          <Composer
            draft={draft}
            setDraft={setDraft}
            placeholder="Try it, or ask about it"
            canSend={!job.working && conversationId !== null}
            onSend={(text) => void send(text)}
          />
          <span className="sheet-note">
            <Text variant="small" tone="faint" align="center" as="div">
              Saved in the Tutor tab, under Words.
            </Text>
          </span>
        </>
      }
    >
      <div className="chat word-chat">
        {shown.map((message) => (
          <div key={message.id} className="msg selectable" data-role={message.role}>
            {message.role === "assistant" ? <Markdown text={message.content} /> : message.content}
          </div>
        ))}
        {job.working ? (
          <div className="msg" data-role="assistant">
            {job.text ? (
              <div>
                <Markdown text={job.text} />
                <span className="caret" />
              </div>
            ) : (
              <Inline gap={2}>
                <Spinner />
                <Text variant="small" tone="faint">
                  {job.status ?? "Thinking…"}
                </Text>
              </Inline>
            )}
          </div>
        ) : null}
        <div ref={endRef} />
      </div>
    </Sheet>
  );
}
