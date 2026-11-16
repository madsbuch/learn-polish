import { useState } from "react";

import { Banner, Button, Card, Field, List, ListItem, Screen, Section, Segmented, Stack, Text } from "../design";
import { PodcastSources } from "../components/Podcast";
import { api, errorMessage } from "../lib/api";
import { sectionsToText, textToSections } from "../lib/lyrics";
import type { CourseProgress, PodcastFormat, Settings, SongDraft, SongLength, TopicIdea } from "../lib/types";

const FORMAT_HINTS: Record<PodcastFormat, string> = {
  pairs: "Everyday sentences, each said in English and then in Polish — like a song's verses.",
  slow: "Very simple sentences; the Polish is read slowly and said twice.",
};

/**
 * Making a podcast episode: a subject (suggested from what the learner cares
 * about), a Wikipedia article to write it from, then the script to approve
 * before it is read aloud.
 */
export function PodcastComposer({
  settings,
  courses,
  onCancel,
  onCreated,
  onSwitch,
}: {
  settings: Settings;
  courses: CourseProgress[];
  onCancel: () => void;
  onCreated: () => void;
  /** Makes a song instead. */
  onSwitch: () => void;
}) {
  const [topic, setTopic] = useState("");
  const [article, setArticle] = useState("");
  const [ideas, setIdeas] = useState<TopicIdea[] | null>(null);
  const [courseId, setCourseId] = useState<number | null>(null);
  const [format, setFormat] = useState<PodcastFormat>("pairs");
  const [length, setLength] = useState<SongLength>("medium");
  const [vocalGender, setVocalGender] = useState("f");

  const [draft, setDraft] = useState<SongDraft | null>(null);
  const [scriptText, setScriptText] = useState("");
  const [feedback, setFeedback] = useState("");
  const [busy, setBusy] = useState<"ideas" | "draft" | "refine" | "create" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const writer = settings.songwriterModel || settings.aiModel;

  function accept(next: SongDraft) {
    setDraft(next);
    setScriptText(sectionsToText(next.sections, "Part"));
  }

  /** The draft as currently edited on screen. */
  function edited(): SongDraft | null {
    if (!draft) return null;
    return { ...draft, sections: textToSections(scriptText) };
  }

  async function run(kind: NonNullable<typeof busy>, action: () => Promise<void>) {
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
    <Screen title={draft ? "Your episode" : "New podcast"} onBack={onCancel}>
      {error ? <Banner tone="error">{error}</Banner> : null}
      {!writer ? <Banner tone="info">Pick a songwriter model in Settings — it writes the scripts too.</Banner> : null}

      {!draft ? (
        <>
          <Segmented
            value="podcast"
            onChange={(value) => value === "song" && onSwitch()}
            options={[
              { value: "song", label: "Song" },
              { value: "podcast", label: "Podcast" },
            ]}
          />

          <Section
            title="What it is about"
            info={
              <p>
                The episode is written from an English Wikipedia article on the subject, and the
                article (with its Polish version, when there is one) is kept with the episode so you
                can read more afterwards.
              </p>
            }
          >
            <Card>
              <Stack gap={4}>
                <Field label="Subject">
                  <input
                    value={topic}
                    placeholder="e.g. the Wieliczka Salt Mine, octopuses, Marie Curie"
                    onChange={(event) => {
                      setTopic(event.target.value);
                      setArticle("");
                    }}
                  />
                </Field>
                {article ? (
                  <Text variant="small" tone="dim">
                    Written from the Wikipedia article “{article}”.
                  </Text>
                ) : null}
                <Button
                  variant="plain"
                  loading={busy === "ideas"}
                  disabled={busy !== null || !writer}
                  onClick={() => run("ideas", async () => setIdeas(await api.podcastTopics()))}
                >
                  {ideas ? "Suggest other subjects" : "Suggest subjects from my interests"}
                </Button>
              </Stack>
            </Card>
            {ideas && ideas.length > 0 ? (
              <List>
                {ideas.map((idea) => (
                  <ListItem
                    key={idea.topic}
                    title={idea.topic}
                    subtitle={idea.why}
                    selected={topic === idea.topic}
                    chevron={false}
                    onClick={() => {
                      setTopic(idea.topic);
                      setArticle(idea.article);
                    }}
                  />
                ))}
              </List>
            ) : null}
          </Section>

          <Section title="How it sounds">
            <Card>
              <Stack gap={4}>
                <Field label="Format" hint={FORMAT_HINTS[format]}>
                  <Segmented
                    value={format}
                    onChange={setFormat}
                    options={[
                      { value: "pairs", label: "Sentence pairs" },
                      { value: "slow", label: "Simple & slow" },
                    ]}
                  />
                </Field>
                <Field label="Length">
                  <Segmented
                    value={length}
                    onChange={setLength}
                    options={[
                      { value: "short", label: "Short" },
                      { value: "medium", label: "Medium" },
                      { value: "long", label: "Long" },
                    ]}
                  />
                </Field>
                <Field label="Polish voice" hint="The English is read by the other voice; change it later.">
                  <Segmented
                    value={vocalGender}
                    onChange={setVocalGender}
                    options={[
                      { value: "f", label: "Female" },
                      { value: "m", label: "Male" },
                    ]}
                  />
                </Field>
                <Field label="Words from">
                  <select
                    value={courseId ?? ""}
                    onChange={(event) => setCourseId(event.target.value ? Number(event.target.value) : null)}
                  >
                    <option value="">Words I have practised</option>
                    {courses.map((entry) => (
                      <option key={entry.course.id} value={entry.course.id}>
                        {entry.course.titlePl} — {entry.course.title}
                      </option>
                    ))}
                  </select>
                </Field>
              </Stack>
            </Card>
          </Section>

          <Stack gap={2}>
            <Button
              variant="primary"
              size="lg"
              wide
              loading={busy === "draft"}
              disabled={busy !== null || !writer || !topic.trim()}
              onClick={() =>
                run("draft", async () =>
                  accept(await api.podcastDraft({ topic, article, format, length, courseId, vocalGender })),
                )
              }
            >
              {busy === "draft" ? "Reading Wikipedia and writing…" : "Write the script"}
            </Button>
            <Text variant="caption" tone="faint" align="center">
              Written by {writer || "—"}. Nothing is read aloud until you approve the script.
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
              <Field label="Title in Polish">
                <input
                  value={draft.chorusPl}
                  onChange={(event) => setDraft({ ...draft, chorusPl: event.target.value })}
                />
              </Field>
              <Field label="About">
                <input
                  value={draft.chorusEn}
                  onChange={(event) => setDraft({ ...draft, chorusEn: event.target.value })}
                />
              </Field>
              <Field
                label="Script"
                hint="One sentence per line: English | Polish. Each is said in English, then in Polish. Parts start with [Part]."
              >
                <textarea
                  className="lyrics-editor"
                  rows={16}
                  value={scriptText}
                  spellCheck={false}
                  onChange={(event) => setScriptText(event.target.value)}
                />
              </Field>
            </Stack>
          </Card>

          {draft.origin?.sources?.length ? (
            <PodcastSources sources={draft.origin.sources} />
          ) : (
            <Banner tone="info">No Wikipedia article was found, so the script is written from general knowledge.</Banner>
          )}

          <Section title="Ask the writer for changes">
            <div className="composer">
              <textarea
                rows={2}
                value={feedback}
                placeholder="e.g. simpler sentences, more about the history, a funnier start"
                onChange={(event) => setFeedback(event.target.value)}
              />
              <Button
                loading={busy === "refine"}
                disabled={busy !== null || feedback.trim().length === 0}
                onClick={() =>
                  run("refine", async () => {
                    const current = edited();
                    if (!current) return;
                    accept(await api.podcastDraft({ previous: current, feedback }));
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
              wide
              loading={busy === "create"}
              disabled={busy !== null || !settings.hasApiKey}
              onClick={() =>
                run("create", async () => {
                  const current = edited();
                  if (!current) return;
                  await api.podcastCreate(current);
                  onCreated();
                })
              }
            >
              Record the episode
            </Button>
            <Text variant="caption" tone="faint" align="center">
              Read aloud sentence by sentence through OpenRouter; usually a minute or two. You can leave meanwhile.
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
