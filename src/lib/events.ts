import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { z } from "zod";

import { logError } from "./log";

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
