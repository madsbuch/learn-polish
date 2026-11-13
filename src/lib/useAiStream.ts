import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { errorMessage } from "./api";
import type { AgentStatus, StreamEvent } from "./types";

let nextStreamId = 0;

/**
 * Runs an AI command — the tutor, a check-in, an explanation — and exposes
 * its answer as it streams in (`ai://stream`), plus what a tool is doing
 * meanwhile (`agent://status`).
 *
 * Each run gets its own id so a stale stream — one the user navigated away
 * from, or superseded by a newer request — cannot write into the current text.
 */
export function useAiStream() {
  const [text, setText] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const activeId = useRef<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    const offText = listen<StreamEvent>("ai://stream", (event) => {
      const payload = event.payload;
      if (!mounted.current || payload.id !== activeId.current) return;
      if (payload.delta) {
        setStatus(null);
        setText((current) => current + payload.delta);
      }
      if (payload.done && payload.error) setError(payload.error);
    });
    const offStatus = listen<AgentStatus>("agent://status", (event) => {
      if (mounted.current && event.payload.id === activeId.current) setStatus(event.payload.label);
    });
    return () => {
      mounted.current = false;
      void offText.then((off) => off());
      void offStatus.then((off) => off());
    };
  }, []);

  /**
   * Starts a run. Resolves to the command's result, or undefined if it failed
   * (the message is in `error`). The streamed text stays until the next run or
   * `reset`.
   */
  const run = useCallback(async <T>(start: (streamId: string) => Promise<T>): Promise<T | undefined> => {
    const id = `s${nextStreamId++}`;
    activeId.current = id;
    setText("");
    setStatus(null);
    setError(null);
    setStreaming(true);
    try {
      return await start(id);
    } catch (cause) {
      if (mounted.current && activeId.current === id) setError(errorMessage(cause));
      return undefined;
    } finally {
      if (mounted.current && activeId.current === id) {
        activeId.current = null;
        setStreaming(false);
        setStatus(null);
      }
    }
  }, []);

  /** Clears the text and error, and ignores a run still in flight. */
  const reset = useCallback(() => {
    activeId.current = null;
    setText("");
    setStatus(null);
    setError(null);
    setStreaming(false);
  }, []);

  return { text, status, streaming, error, run, reset };
}
