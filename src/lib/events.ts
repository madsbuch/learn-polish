import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { z } from "zod";

import { logError } from "./log";
import { SyncActivitySchema, SyncReceivedSchema } from "./types";
import type { SyncActivity, SyncReceived } from "./types";

/**
 * Listens to an event from the Rust core, checking each payload against
 * `schema`: a payload of the wrong shape is reported and dropped, never
 * handed to a screen.
 */
export function onEvent<S extends z.ZodTypeAny>(
  schema: S,
  name: string,
  handler: (event: { payload: z.infer<S> }) => void,
): Promise<UnlistenFn> {
  return listen<unknown>(name, (event) => {
    const parsed = schema.safeParse(event.payload);
    if (parsed.success) handler({ payload: parsed.data });
    else
      logError(
        "shape",
        `Event ${name}: ${parsed.error.issues
          .slice(0, 3)
          .map((issue) => `${issue.path.join(".") || "(payload)"}: ${issue.message}`)
          .join("; ")}`,
      );
  });
}

/** Listens to an event whose payload is not read: a signal that something changed. */
export function onSignal(name: string, handler: () => void): Promise<UnlistenFn> {
  return listen<unknown>(name, () => handler());
}

/** Another device's data arrived here: the screens reload what they show. */
export function onSyncChanged(handler: (received: SyncReceived) => void): Promise<UnlistenFn> {
  return onEvent(SyncReceivedSchema, "sync-changed", (event) => handler(event.payload));
}

/** Sync's status moved (a round started or ended). */
export function onSyncStatus(handler: (activity: SyncActivity) => void): Promise<UnlistenFn> {
  return onEvent(SyncActivitySchema, "sync-status", (event) => handler(event.payload));
}
