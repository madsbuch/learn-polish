import { useCallback, useEffect, useState } from "react";

import { Button, Inline, List, ListEmpty, ListItem, Section, Stack } from "../design";
import { api, errorMessage } from "../lib/api";
import { toast } from "../lib/toast";
import type { DiagnosticEntry } from "../lib/types";

/** How many entries show before "Show all". */
const FIRST = 15;

function when(at: number): string {
  return new Date(at * 1000).toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

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
 * failed. Newest first; Copy puts it all on the clipboard to pass on.
 */
export function Diagnostics() {
  const [entries, setEntries] = useState<DiagnosticEntry[] | null>(null);
  const [all, setAll] = useState(false);

  const load = useCallback(() => {
    api
      .diagnostics()
      .then(setEntries)
      .catch(() => setEntries([]));
  }, []);
  useEffect(load, [load]);

  const shown = entries === null ? [] : all ? entries : entries.slice(0, FIRST);
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
        <List>
          {entries !== null && entries.length === 0 ? <ListEmpty>Nothing noted yet.</ListEmpty> : null}
          {shown.map((entry, i) => (
            <ListItem
              key={`${entry.at}-${i}`}
              selectable
              title={entry.message}
              subtitle={`${when(entry.at)} · ${entry.kind}`}
            />
          ))}
        </List>
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
          {entries && entries.length > FIRST && !all ? (
            <Button size="sm" onClick={() => setAll(true)}>
              {`Show all ${entries.length}`}
            </Button>
          ) : null}
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
