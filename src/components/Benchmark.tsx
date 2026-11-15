import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { Badge, Banner, Button, ConfirmButton, Field, List, ListEmpty, ListItem, Section, Sheet, Stack, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import type { AiModel, BenchRun } from "../lib/types";

const PARTS: { key: "polish" | "tools" | "rules"; label: string }[] = [
  { key: "polish", label: "Polish" },
  { key: "tools", label: "Tools" },
  { key: "rules", label: "Rules" },
];

function tone(score: number): "mint" | "gold" | "crimson" {
  return score >= 85 ? "mint" : score >= 65 ? "gold" : "crimson";
}

function money(cost: number | null): string {
  if (cost === null) return "price not reported";
  return cost < 0.01 ? `$${cost.toFixed(4)}` : `$${cost.toFixed(3)}`;
}

/**
 * The model benchmark: run the app's own test suite on a model and compare
 * the scores, speed and price of every run.
 */
export function Benchmark({ onClose }: { onClose: () => void }) {
  const [runs, setRuns] = useState<BenchRun[] | null>(null);
  const [models, setModels] = useState<AiModel[]>([]);
  const [model, setModel] = useState("");
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<BenchRun | null>(null);

  const load = () => api.benchRuns().then(setRuns).catch((cause) => setError(errorMessage(cause)));
  useEffect(() => {
    void load();
    api
      .aiListModels()
      .then((list) => setModels(list.filter((m) => m.tools && m.voices.length === 0)))
      .catch(() => {});
    const off = listen<{ done: number; total: number }>("bench://progress", (event) => setProgress(event.payload));
    return () => {
      void off.then((stop) => stop());
    };
  }, []);

  async function run() {
    if (!model || running) return;
    setRunning(true);
    setError(null);
    setProgress(null);
    try {
      const result = await api.benchRun(model);
      await load();
      setOpen(result);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setRunning(false);
    }
  }

  const name = (id: string) => models.find((m) => m.id === id)?.name ?? id;

  return (
    <Sheet title="Benchmark" onClose={onClose}>
      <Stack gap={4}>
        <Text variant="small" tone="dim">
          About 20 short tests of the jobs this app gives a model: correct Polish (cases, verbs, fixing
          a mistake), using the tutor's tools properly, and following the app's rules — your
          language, short answers, staying in Polish as the writing partner. Scored by fixed rules,
          not by another model. A run costs a few cents at most.
        </Text>
        <Field label="Model to test">
          <select value={model} onChange={(event) => setModel(event.target.value)} disabled={running}>
            <option value="">— choose a model —</option>
            {models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name}
              </option>
            ))}
          </select>
        </Field>
        <Button variant="primary" block loading={running} disabled={!model} onClick={() => void run()}>
          {running ? (progress ? `Testing… ${progress.done} of ${progress.total}` : "Testing…") : "Run the benchmark"}
        </Button>
        {error ? <Banner tone="error">{error}</Banner> : null}

        <Section title="Results">
          <List>
            {runs === null ? null : runs.length === 0 ? (
              <ListEmpty>No runs yet.</ListEmpty>
            ) : (
              runs.map((r) => (
                <ListItem
                  key={r.id}
                  title={name(r.model)}
                  subtitle={`${PARTS.map((p) => `${p.label} ${r[p.key]}`).join(" · ")} · ${r.seconds.toFixed(1)} s · ${money(r.costUsd)}`}
                  trailing={<Badge tone={tone(r.score)}>{r.score}</Badge>}
                  chevron
                  onClick={() => setOpen(r)}
                />
              ))
            )}
          </List>
        </Section>
      </Stack>

      {open ? (
        <Sheet title={`${name(open.model)} · ${open.score}`} onClose={() => setOpen(null)}>
          <Stack gap={4}>
            <Text variant="small" tone="dim">
              {PARTS.map((p) => `${p.label} ${open[p.key]}`).join(" · ")} · {open.seconds.toFixed(1)} s per reply ·{" "}
              {money(open.costUsd)} · {new Date(open.createdAt * 1000).toLocaleString()}
            </Text>
            {PARTS.map((part) => (
              <Section key={part.key} title={`${part.label} · ${open[part.key]}`}>
                <List>
                  {open.items
                    .filter((item) => item.part === part.key)
                    .sort((a, b) => a.score - b.score)
                    .map((item) => (
                      <ListItem
                        key={item.name}
                        title={`${item.score >= 1 ? "✓" : item.score > 0 ? "◐" : "✗"} ${item.name}`}
                        subtitle={[item.note, item.answer ? `Said: ${item.answer}` : ""].filter(Boolean).join(" — ")}
                        selectable
                      />
                    ))}
                </List>
              </Section>
            ))}
            <ConfirmButton
              label="Delete this run"
              confirmLabel="Delete"
              question="Delete this benchmark run?"
              block
              onConfirm={async () => {
                await api.benchDelete(open.id);
                setOpen(null);
                await load();
              }}
            />
          </Stack>
        </Sheet>
      ) : null}
    </Sheet>
  );
}
