import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { z } from "zod";

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
    else console.error(`Unexpected payload for ${name}`, parsed.error.issues, event.payload);
  });
}

/** Listens to an event whose payload is not read: a signal that something changed. */
export function onSignal(name: string, handler: () => void): Promise<UnlistenFn> {
  return listen<unknown>(name, () => handler());
}
