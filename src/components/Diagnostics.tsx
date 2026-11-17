import { useCallback, useEffect, useState } from "react";

import { Button, Inline, Section, Stack } from "../design";
import { api, errorMessage } from "../lib/api";
import { toast } from "../lib/toast";
import type { DiagnosticEntry } from "../lib/types";

/** The entries as plain text, to paste into a message. */
function asText(entries: DiagnosticEntry[]): string {
  return entries.map((e) => `${new Date(e.at * 1000).toISOString()} [${e.kind}] ${e.message}`).join("\n");
}

/** Copies text, with a fallback for webviews without the clipboard API. */
async function copy(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    if (!ok) throw new Error("Copying is not allowed here.");
  }
}

/**
 * Settings → Diagnostics: what the app noted about itself — how long slow
 * screens took, model replies it could not read, background work that
 * failed. Raw lines, newest first, in a box that is easy to select; Copy
 * puts it all on the clipboard to pass on.
 */
export function Diagnostics() {
  const [entries, setEntries] = useState<DiagnosticEntry[] | null>(null);

  const load = useCallback(() => {
    api
      .diagnostics()
      .then(setEntries)
      .catch(() => setEntries([]));
  }, []);
  useEffect(load, [load]);

  return (
    <Section
      title="Diagnostics"
      info={
        <p>
          What the app noted about itself: how long slow screens took to load, answers from AI models
          it could not read, and work in the background that failed. Copy it to pass it on when
          something seems wrong. It stays on this device.
        </p>
      }
    >
      <Stack gap={3}>
        <pre className="diagnostics-log selectable">
          {entries === null ? "Loading…" : entries.length === 0 ? "Nothing noted yet." : asText(entries)}
        </pre>
        <Inline gap={2} wrap>
          <Button
            size="sm"
            disabled={!entries?.length}
            onClick={async () => {
              try {
                await copy(asText(entries ?? []));
                toast("Copied — paste it into a message.");
              } catch (cause) {
                toast(errorMessage(cause));
              }
            }}
          >
            Copy all
          </Button>
          <Button size="sm" onClick={load}>
            Refresh
          </Button>
          <Button
            size="sm"
            variant="plain"
            disabled={!entries?.length}
            onClick={async () => {
              await api.diagnosticsClear().catch(() => {});
              load();
            }}
          >
            Clear
          </Button>
        </Inline>
      </Stack>
    </Section>
  );
}
