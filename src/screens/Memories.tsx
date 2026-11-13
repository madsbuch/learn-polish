import { useEffect, useState } from "react";

import { MemoryIcon, PlusIcon } from "../components/icons";
import {
  Banner,
  Button,
  Choice,
  ConfirmButton,
  EmptyState,
  Field,
  IconButton,
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  SettingRow,
  Sheet,
  Spinner,
  Stack,
  Switch,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import type { Memory, MemoryCategory, MemoryOverview } from "../lib/types";

const CATEGORIES: { value: MemoryCategory; title: string; label: string }[] = [
  { value: "goal", title: "Goals", label: "Goal" },
  { value: "preference", title: "Preferences", label: "Preference" },
  { value: "context", title: "About you", label: "About you" },
  { value: "strength", title: "Strengths", label: "Strength" },
  { value: "struggle", title: "Struggles", label: "Struggle" },
];

const SOURCE: Record<string, string> = {
  tutor: "Saved by the tutor",
  examiner: "Saved by the examiner",
  you: "Added by you",
};

function when(seconds: number): string {
  return new Date(seconds * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

/** A new memory, or the one being edited. */
type Editing = { id: number | null; content: string; category: MemoryCategory };

/**
 * What the tutor and the examiner remember about the learner. Everything
 * here goes into their prompts, so the learner can correct or delete any of it.
 */
export function Memories({ onBack }: { onBack: () => void }) {
  const [overview, setOverview] = useState<MemoryOverview | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.memoriesOverview().then(setOverview).catch((cause) => setError(errorMessage(cause)));
  }, []);

  async function run(action: () => Promise<MemoryOverview>) {
    setBusy(true);
    setError(null);
    try {
      setOverview(await action());
      return true;
    } catch (cause) {
      setError(errorMessage(cause));
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function save() {
    if (!editing || !editing.content.trim()) return;
    const { id, content, category } = editing;
    const ok = await run(() =>
      id === null ? api.memoryAdd(content, category) : api.memoryUpdate(id, content, category),
    );
    if (ok) setEditing(null);
  }

  const open = (memory: Memory) =>
    setEditing({ id: memory.id, content: memory.content, category: memory.category });

  return (
    <Screen
      title="Memories"
      subtitle={overview ? `${overview.memories.length} remembered` : undefined}
      onBack={onBack}
      nav={false}
      info={
        <>
          <p>
            The tutor and the examiner keep short notes about you — your goals, what you like,
            what you find hard and what you have mastered — and both read all of them before they
            answer. What the examiner notices in a check-in reaches the tutor, and the other way
            round.
          </p>
          <p>
            Add anything you want them to know, and fix or delete anything that is wrong. Memories
            stay on this device, in the app's database, and are sent to the AI model with each
            request while memory is on.
          </p>
        </>
      }
      actions={
        <IconButton
          variant="tinted"
          icon={<PlusIcon />}
          label="Add a memory"
          onClick={() => setEditing({ id: null, content: "", category: "goal" })}
        />
      }
    >
      {error ? <Banner tone="error">{error}</Banner> : null}
      {!overview ? (
        <Spinner />
      ) : (
        <>
          <List>
            <SettingRow
              label="Let the agents remember"
              description="Off: they neither read nor save memories"
              control={
                <Switch
                  label="Let the agents remember"
                  checked={overview.enabled}
                  onChange={(enabled) => void run(() => api.memoriesSetEnabled(enabled))}
                />
              }
            />
          </List>

          {overview.memories.length === 0 ? (
            <EmptyState
              icon={<MemoryIcon />}
              title="Nothing remembered yet"
              action={
                <Button
                  variant="primary"
                  icon={<PlusIcon />}
                  onClick={() => setEditing({ id: null, content: "", category: "goal" })}
                >
                  Tell them something
                </Button>
              }
            >
              As you talk to the tutor and do check-ins, they note what matters — or add something
              yourself, like why you are learning Polish.
            </EmptyState>
          ) : (
            CATEGORIES.map((category) => {
              const items = overview.memories.filter((m) => m.category === category.value);
              if (items.length === 0) return null;
              return (
                <Section key={category.value} title={category.title}>
                  <List>
                    {items.map((memory) => (
                      <ListItem
                        key={memory.id}
                        title={memory.content}
                        subtitle={`${SOURCE[memory.source] ?? memory.source} · ${when(memory.updatedAt)}`}
                        onClick={() => open(memory)}
                        label={`Edit: ${memory.content}`}
                      />
                    ))}
                  </List>
                </Section>
              );
            })
          )}

          {overview.memories.length > 0 ? (
            <Section title="Manage">
              <ConfirmButton
                label="Forget everything"
                confirmLabel="Forget all"
                question={`Delete all ${overview.memories.length} memories? The agents start from scratch.`}
                block
                onConfirm={() => run(api.memoriesClear)}
              />
            </Section>
          ) : null}
        </>
      )}

      {editing ? (
        <Sheet title={editing.id === null ? "New memory" : "Memory"} onClose={() => setEditing(null)}>
          <Stack gap={4}>
            <Field label="What they should know" hint="One fact, in a sentence or two">
              <textarea
                rows={3}
                value={editing.content}
                maxLength={400}
                placeholder="e.g. Moving to Gdańsk in the spring and wants to handle the flat viewing in Polish"
                onChange={(event) => setEditing({ ...editing, content: event.target.value })}
              />
            </Field>
            <Stack gap={2}>
              <Text variant="small" weight="medium" tone="dim">
                Kind
              </Text>
              <Inline gap={2} wrap>
                {CATEGORIES.map((category) => (
                  <Choice
                    key={category.value}
                    selected={editing.category === category.value}
                    onClick={() => setEditing({ ...editing, category: category.value })}
                  >
                    {category.label}
                  </Choice>
                ))}
              </Inline>
            </Stack>
            <Button
              variant="primary"
              size="lg"
              block
              loading={busy}
              disabled={!editing.content.trim()}
              onClick={save}
            >
              {editing.id === null ? "Remember this" : "Save"}
            </Button>
            {editing.id !== null ? (
              <ConfirmButton
                label="Forget this"
                confirmLabel="Forget"
                block
                onConfirm={async () => {
                  if (await run(() => api.memoryDelete(editing.id!))) setEditing(null);
                }}
              />
            ) : null}
          </Stack>
        </Sheet>
      ) : null}
    </Screen>
  );
}
