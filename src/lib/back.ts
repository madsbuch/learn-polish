import { useEffect, useRef } from "react";

/**
 * The system back button, for an app whose pages are React state rather than
 * URLs.
 *
 * On Android, Tauri sends the back button to the web view's history: it goes
 * back if there is somewhere to go, and closes the app if not. So every open
 * page and sheet that has a way back registers here, and each adds one
 * history entry. Back then pops the newest entry and runs that page's own
 * back action; with nothing registered — the home tab — back closes the app,
 * as Android users expect.
 *
 * A page closed from inside the app (its ← button) takes its history entry
 * with it, so the history never holds entries for pages that are gone.
 */

interface Entry {
  run: () => void;
}

const stack: Entry[] = [];
/** History entries closed in-app this tick, removed together in one step. */
let pendingBack = 0;
/** popstate events caused by that clean-up, which must not run a handler. */
let ignoredPops = 0;

if (typeof window !== "undefined") {
  window.addEventListener("popstate", () => {
    if (ignoredPops > 0) {
      ignoredPops -= 1;
      return;
    }
    const entry = stack.pop();
    entry?.run();
  });
}

function dropHistoryEntry() {
  pendingBack += 1;
  if (pendingBack > 1) return;
  // Several pages can close in one render (a page replaced by the next);
  // their entries go in a single step, which fires a single popstate.
  queueMicrotask(() => {
    const steps = pendingBack;
    pendingBack = 0;
    ignoredPops += 1;
    window.history.go(-steps);
  });
}

/**
 * While `active`, the back button runs `onBack`. `key` re-registers when the
 * page shown changes without this component unmounting.
 */
export function useBackHandler(active: boolean, onBack: (() => void) | undefined, key?: unknown) {
  const handler = useRef(onBack);
  handler.current = onBack;

  useEffect(() => {
    if (!active) return;
    const entry: Entry = { run: () => handler.current?.() };
    stack.push(entry);
    window.history.pushState({ polski: stack.length }, "");
    return () => {
      const index = stack.indexOf(entry);
      // Still registered: the page closed from inside the app, not through
      // the back button, so its history entry has to go too.
      if (index >= 0) {
        stack.splice(index, 1);
        dropHistoryEntry();
      }
    };
  }, [active, key]);
}
