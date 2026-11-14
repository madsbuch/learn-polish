import { useEffect, useState } from "react";

/**
 * Unsent messages, kept per conversation so leaving a chat — or the app —
 * doesn't lose what was being written. Stored on the device only.
 */
const PREFIX = "polski.draft.";

export function readDraft(key: string): string {
  try {
    return localStorage.getItem(PREFIX + key) ?? "";
  } catch {
    return "";
  }
}

export function writeDraft(key: string, text: string) {
  try {
    if (text) localStorage.setItem(PREFIX + key, text);
    else localStorage.removeItem(PREFIX + key);
  } catch {
    // Storage unavailable: the draft just isn't kept.
  }
}

/** A draft for `key`, saved as it changes. */
export function useDraft(key: string): [string, (text: string) => void] {
  const [draft, setDraft] = useState(() => readDraft(key));
  useEffect(() => writeDraft(key, draft), [key, draft]);
  return [draft, setDraft];
}
