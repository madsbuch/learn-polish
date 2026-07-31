import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { errorMessage } from "./api";
import type { StreamEvent } from "./types";

let nextStreamId = 0;

/**
 * Runs a streaming AI command and exposes the answer as it arrives.
 *
 * Each run gets its own id so a stale stream — one the user navigated away
 * from, or superseded by a newer request — cannot write into the current text.
 */
export function useAiStream() {
  const [text, setText] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const activeId = useRef<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    const unlisten = listen<StreamEvent>("ai://stream", (event) => {
      const payload = event.payload;
      if (!mounted.current || payload.id !== activeId.current) return;
      if (payload.delta) setText((current) => current + payload.delta);
      if (payload.done) {
        setStreaming(false);
        if (payload.error) setError(payload.error);
        activeId.current = null;
      }
    });
    return () => {
      mounted.current = false;
      void unlisten.then((off) => off());
    };
  }, []);

  const run = useCallback(async (start: (streamId: string) => Promise<string>) => {
    const id = `s${nextStreamId++}`;
    activeId.current = id;
    setText("");
    setError(null);
    setStreaming(true);
    try {
      await start(id);
    } catch (cause) {
      if (mounted.current && activeId.current === id) {
        setError(errorMessage(cause));
        setStreaming(false);
        activeId.current = null;
      }
    }
  }, []);

  const reset = useCallback(() => {
    activeId.current = null;
    setText("");
    setError(null);
    setStreaming(false);
  }, []);

  return { text, streaming, error, run, reset };
}
