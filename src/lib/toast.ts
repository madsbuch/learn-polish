/**
 * Short messages that float over the screen — "Added to Morning run",
 * "Saved to Downloads", an action that failed — so they are seen wherever
 * the page is scrolled. Shown by the design system's <Toaster />.
 */

export type ToastTone = "ok" | "error" | "info";

export interface Toast {
  id: number;
  text: string;
  tone: ToastTone;
}

let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();

function changed() {
  for (const listener of listeners) listener();
}

/** Shows a message; it goes by itself (errors stay a little longer). */
export function toast(text: string, tone: ToastTone = "ok") {
  const id = nextId++;
  // At most three at a time; the oldest goes first.
  toasts = [...toasts.slice(-2), { id, text, tone }];
  changed();
  window.setTimeout(() => dismiss(id), tone === "error" ? 7000 : 4000);
}

export function dismiss(id: number) {
  const before = toasts.length;
  toasts = toasts.filter((entry) => entry.id !== id);
  if (toasts.length !== before) changed();
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function current(): Toast[] {
  return toasts;
}
