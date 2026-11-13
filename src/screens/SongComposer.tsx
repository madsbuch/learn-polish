import { useEffect, useState } from "react";

import {
  Banner,
  Button,
  Card,
  Choice,
  ChoiceRow,
  Field,
  Screen,
  Section,
  Segmented,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { sectionsToText, textToSections } from "../lib/lyrics";
import type { Sentence, Settings, SongDraft, SongLength, TopicProgress } from "../lib/types";

// ------------------------------------------------------------------ composer

const STYLE_PRESETS = [
  "catchy acoustic pop, mid tempo, warm female vocals, clear diction",
  "upbeat indie folk, hand claps, male vocals, clear diction",
  "80s synth pop, bright and danceable, clear vocals, clear diction",
  "gentle lullaby, slow, soft piano, clear vocals, clear diction",
  "reggae, relaxed groove, clear vocals, clear diction",
  "pop rock, driving guitars, anthemic chorus, clear vocals, clear diction",
];
const PRESET_NAMES = ["Acoustic pop", "Indie folk", "Synth pop", "Lullaby", "Reggae", "Pop rock"];

export function SongComposer({
  settings,
  topics,
  onCancel,
  onCreated,
}: {
  settings: Settings;
  topics: TopicProgress[];
  onCancel: () => void;
  onCreated: () => void;
}) {
  const [brief, setBrief] = useState("");
  const [topicId, setTopicId] = useState<number | null>(null);
  const [sentences, setSentences] = useState<Sentence[]>([]);
  const [chorusChoice, setChorusChoice] = useState(""); // "", sentence pl, or "custom"
  const [customChorus, setCustomChorus] = useState("");
  const [style, setStyle] = useState(STYLE_PRESETS[0]);
  const [vocalGender, setVocalGender] = useState("");
  const [length, setLength] = useState<SongLength>(settings.songLength);

  const [draft, setDraft] = useState<SongDraft | null>(null);
  const [lyricsText, setLyricsText] = useState("");
  const [feedback, setFeedback] = useState("");
  const [busy, setBusy] = useState<"draft" | "refine" | "create" | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setChorusChoice("");
    if (topicId === null) {
      setSentences([]);
      return;
    }
    api.browseSentences(topicId).then(setSentences).catch(() => setSentences([]));
  }, [topicId]);

  const chorus = chorusChoice === "custom" ? customChorus.trim() : chorusChoice;
  const songwriter = settings.songwriterModel || settings.aiModel;

  function accept(next: SongDraft) {
    setDraft(next);
    setLyricsText(sectionsToText(next.sections));
  }

  /** The draft as currently edited on screen. */
  function edited(): SongDraft | null {
    if (!draft) return null;
    return { ...draft, sections: textToSections(lyricsText) };
  }

  async function run(kind: "draft" | "refine" | "create", action: () => Promise<void>) {
    setBusy(kind);
    setError(null);
    try {
      await action();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }

  return (
    <Screen title={draft ? "Your song" : "New song"} onBack={onCancel}>
      {error ? <Banner tone="error">{error}</Banner> : null}
      {!songwriter ? <Banner tone="info">Pick a songwriter model in Settings.</Banner> : null}

      {!draft ? (
        <>
          <Section title="What it is about">
            <Card>
              <Stack gap={4}>
                <Field label="Theme">
                  <textarea
                    rows={2}
                    value={brief}
                    placeholder="e.g. ordering breakfast in a café in Kraków"
                    onChange={(event) => setBrief(event.target.value)}
                  />
                </Field>
                <Field label="Words from">
                  <select
                    value={topicId ?? ""}
                    onChange={(event) => setTopicId(event.target.value ? Number(event.target.value) : null)}
                  >
                    <option value="">Words I have practised</option>
                    {topics.map((entry) => (
                      <option key={entry.topic.id} value={entry.topic.id}>
                        {entry.topic.titlePl} — {entry.topic.title}
                      </option>
                    ))}
                  </select>
                </Field>
                <Field label="Chorus sentence" hint="The one sentence to get stuck in your head.">
                  <select value={chorusChoice} onChange={(event) => setChorusChoice(event.target.value)}>
                    <option value="">Let the songwriter choose</option>
                    {sentences.map((sentence) => (
                      <option key={sentence.id} value={sentence.pl}>
                        {sentence.pl} — {sentence.meaning}
                      </option>
                    ))}
                    <option value="custom">Write my own…</option>
                  </select>
                </Field>
                {chorusChoice === "custom" ? (
                  <input
                    value={customChorus}
                    placeholder="Polish sentence, e.g. Poproszę kawę z mlekiem"
                    onChange={(event) => setCustomChorus(event.target.value)}
                  />
                ) : null}
              </Stack>
            </Card>
          </Section>

          <Section title="Sound">
            <Card>
              <Stack gap={4}>
                <ChoiceRow>
                  {PRESET_NAMES.map((name, i) => (
                    <Choice key={name} selected={style === STYLE_PRESETS[i]} onClick={() => setStyle(STYLE_PRESETS[i])}>
                      {name}
                    </Choice>
                  ))}
                </ChoiceRow>
                <Field label="Style for Suno">
                  <input value={style} onChange={(event) => setStyle(event.target.value)} />
                </Field>
                <Field label="Length">
                  <Segmented
                    value={length}
                    onChange={setLength}
                    options={[
                      { value: "short", label: "~2 min" },
                      { value: "medium", label: "~3 min" },
                      { value: "long", label: "4–5 min" },
                    ]}
                  />
                </Field>
                <Field label="Voice">
                  <Segmented
                    value={vocalGender}
                    onChange={setVocalGender}
                    options={[
                      { value: "", label: "Any" },
                      { value: "f", label: "Female" },
                      { value: "m", label: "Male" },
                    ]}
                  />
                </Field>
              </Stack>
            </Card>
          </Section>

          <Stack gap={2}>
            <Button
              variant="primary"
              size="lg"
              block
              loading={busy === "draft"}
              disabled={busy !== null || !songwriter || (chorusChoice === "custom" && !chorus)}
              onClick={() =>
                run("draft", async () =>
                  accept(await api.songDraft({ brief, topicId, chorusPl: chorus, style, vocalGender, length })),
                )
              }
            >
              {busy === "draft" ? "The songwriter is writing…" : "Write the lyrics"}
            </Button>
            <Text variant="caption" tone="faint" align="center">
              Written by {songwriter || "—"}. Nothing goes to Suno until you approve the lyrics.
            </Text>
          </Stack>
        </>
      ) : (
        <>
          <Card>
            <Stack gap={4}>
              <Field label="Title">
                <input value={draft.title} onChange={(event) => setDraft({ ...draft, title: event.target.value })} />
              </Field>
              <Field label="Chorus (Polish)">
                <input
                  value={draft.chorusPl}
                  onChange={(event) => setDraft({ ...draft, chorusPl: event.target.value })}
                />
              </Field>
              <Field label="Its meaning">
                <input
                  value={draft.chorusEn}
                  onChange={(event) => setDraft({ ...draft, chorusEn: event.target.value })}
                />
              </Field>
              <Field label="Style for Suno">
                <input value={draft.style} onChange={(event) => setDraft({ ...draft, style: event.target.value })} />
              </Field>
              <Field
                label="Lyrics"
                hint="One phrase per line: English | Polish. Verses sing the English, then the Polish; the chorus only the Polish. Section names go in brackets, like [Chorus]."
              >
                <textarea
                  className="lyrics-editor"
                  rows={16}
                  value={lyricsText}
                  spellCheck={false}
                  onChange={(event) => setLyricsText(event.target.value)}
                />
              </Field>
            </Stack>
          </Card>

          <Section title="Ask the songwriter for changes">
            <div className="composer">
              <textarea
                rows={2}
                value={feedback}
                placeholder="e.g. shorter verses, more food words, a happier chorus"
                onChange={(event) => setFeedback(event.target.value)}
              />
              <Button
                loading={busy === "refine"}
                disabled={busy !== null || feedback.trim().length === 0}
                onClick={() =>
                  run("refine", async () => {
                    const current = edited();
                    if (!current) return;
                    accept(await api.songDraft({ previous: current, feedback }));
                    setFeedback("");
                  })
                }
              >
                Revise
              </Button>
            </div>
          </Section>

          <Stack gap={2}>
            <Button
              variant="primary"
              size="lg"
              block
              loading={busy === "create"}
              disabled={busy !== null || !settings.hasKieKey}
              onClick={() =>
                run("create", async () => {
                  const current = edited();
                  if (!current) return;
                  await api.songCreate(current);
                  onCreated();
                })
              }
            >
              Make it with Suno {settings.sunoModel}
            </Button>
            <Text variant="caption" tone="faint" align="center">
              Uses kie.ai credits. Suno sings two takes in 2–4 minutes; you can leave meanwhile.
            </Text>
            <Button variant="plain" onClick={() => setDraft(null)}>
              Start over
            </Button>
          </Stack>
        </>
      )}
    </Screen>
  );
}
