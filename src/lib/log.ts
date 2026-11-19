import { invoke } from "@tauri-apps/api/core";

/** The text of an error, whatever was thrown. */
export function describe(cause: unknown): string {
  if (typeof cause === "string") return cause;
  if (cause instanceof Error) return cause.message || cause.name;
  try {
    return JSON.stringify(cause) ?? String(cause);
  } catch {
    return String(cause);
  }
}

const recent = new Map<string, number>();

/**
 * Writes an error to the Diagnostics log (Settings → Your data → Diagnostics), where it
 * can be copied. The same message twice within a few seconds is noted once;
 * a log that cannot be written is left alone (it must never cause errors of
 * its own).
 */
export function logError(kind: string, message: string): void {
  const now = Date.now();
  const key = `${kind}|${message}`;
  const last = recent.get(key);
  if (last !== undefined && now - last < 5000) return;
  recent.set(key, now);
  if (recent.size > 200) recent.clear();
  console.error(`${kind}: ${message}`);
  invoke<unknown>("diagnostics_note", { kind, message }).catch(() => undefined);
}

/** Errors already in the log (by the command that threw them), so passing them on doesn't note them twice. */
const logged = new WeakSet<object>();

/** Marks a thrown value as already logged. */
export function markLogged(cause: unknown): void {
  if (typeof cause === "object" && cause !== null) logged.add(cause);
}

/** Logs a thrown value, unless it is already in the log. */
export function logCause(kind: string, cause: unknown): void {
  if (typeof cause === "object" && cause !== null && logged.has(cause)) return;
  markLogged(cause);
  logError(kind, describe(cause));
}

/** Notes what nothing else caught: uncaught exceptions and rejected promises. */
export function logUncaught(): void {
  window.addEventListener("error", (event) => {
    const where = event.filename ? ` (${event.filename}:${event.lineno})` : "";
    if (event.error) logCause("uncaught", event.error);
    else logError("uncaught", `${event.message}${where}`);
  });
  window.addEventListener("unhandledrejection", (event) => {
    logCause("uncaught", event.reason);
  });
}
