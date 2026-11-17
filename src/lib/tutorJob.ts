import { useEffect, useState } from "react";

import { api } from "./api";
import { onEvent } from "./events";
import { AgentStatusSchema, StreamEventSchema, TutorDoneSchema } from "./types";

/**
 * Follows a conversation's background reply: whether the tutor is working,
 * the text so far, what a tool is doing, and a failure. `finished` counts
 * replies that ended while watching, so the caller can reload.
 */
export function useTutorJob(conversationId: number | null) {
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
      onEvent(StreamEventSchema, "ai://stream", (event) => {
        if (event.payload.id !== id || !event.payload.delta) return;
        setWorking(true);
        setStatus(null);
        setText((current) => current + event.payload.delta);
      }),
      onEvent(AgentStatusSchema, "agent://status", (event) => {
        if (event.payload.id !== id) return;
        setWorking(true);
        setStatus(event.payload.label);
      }),
      onEvent(TutorDoneSchema, "tutor://done", (event) => {
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
