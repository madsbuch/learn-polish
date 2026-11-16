import { useState } from "react";

import { CloseIcon, SearchIcon } from "../components/icons";
import {
  Avatar,
  Banner,
  Button,
  Card,
  Field,
  IconButton,
  List,
  ListItem,
  Screen,
  Section,
  Segmented,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import type { CourseProgress, PodcastCast, PodcastFormat, Settings, SongLength, TopicIdea, WikiHit } from "../lib/types";

const FORMAT_HINTS: Record<PodcastFormat, string> = {
  pairs: "Each sentence in English, then in Polish — like a song's verses.",
  immersion: "Only Polish, in simpler sentences. The English meaning shows with the lyrics.",
};

const MAX_ARTICLES = 5;

const CAST_HINTS: Record<PodcastCast, string> = {
  narrator: "One voice tells it all.",
  hosts: "Two hosts talk it through: one asks, one explains — each in their own voice.",
  characters: "A small radio play: characters act it out, each in their own voice.",
};

/**
 * Making a podcast episode in one go: a subject (suggested from what the
 * learner cares about), what it is written from — Wikipedia articles (none:
 * the best match), or a made-up story — and how it sounds. The episode is
 * written and recorded in the background; there is no script to approve.
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
  const [basis, setBasis] = useState<"wikipedia" | "story" | "news">("wikipedia");
  const [articles, setArticles] = useState<string[]>([]);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<WikiHit[] | null>(null);
  const [ideas, setIdeas] = useState<TopicIdea[] | null>(null);
  const [courseId, setCourseId] = useState<number | null>(null);
  const [format, setFormat] = useState<PodcastFormat>("pairs");
  // Until picked, who talks follows what it is written from.
  const [castChoice, setCastChoice] = useState<PodcastCast | null>(null);
  const [length, setLength] = useState<SongLength>("medium");
  const [vocalGender, setVocalGender] = useState("f");
  const [busy, setBusy] = useState<"ideas" | "search" | "make" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const writer = settings.songwriterModel || settings.aiModel;
  const story = basis === "story";
  const news = basis === "news";
  const cast: PodcastCast = castChoice ?? (story ? "characters" : "hosts");

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

  function addArticle(title: string) {
    setArticles((current) =>
      current.includes(title) || current.length >= MAX_ARTICLES ? current : [...current, title],
    );
  }

  function search() {
    if (!query.trim()) return;
    void run("search", async () => setHits(await api.wikiFind(query)));
  }

  const ready =
    Boolean(writer) &&
    settings.hasApiKey &&
    (news || topic.trim() !== "" || (!story && articles.length > 0));

  return (
    <Screen title="New podcast" onBack={onCancel}>
      {error ? <Banner tone="error">{error}</Banner> : null}
      {!writer ? <Banner tone="info">Pick a songwriter model in Settings — it writes the podcasts too.</Banner> : null}

      <Segmented
        value="podcast"
        onChange={(value) => value === "song" && onSwitch()}
        options={[
          { value: "song", label: "Song" },
          { value: "podcast", label: "Podcast" },
        ]}
      />

      <Section title="What it is about">
        <Card>
          <Stack gap={4}>
            <Field label="Subject" hint={news ? "Optional: narrow the news down. Empty: the day's mix." : undefined}>
              <input
                value={topic}
                placeholder={
                  news ? "e.g. housing, cycling, football" : "e.g. the Wieliczka Salt Mine, octopuses, a day at the bakery"
                }
                onChange={(event) => setTopic(event.target.value)}
              />
            </Field>
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
                subtitle={`${idea.articles.length === 0 ? "A story" : idea.articles.join(" · ")} — ${idea.why}`}
                selected={topic === idea.topic}
                chevron={false}
                onClick={() => {
                  setTopic(idea.topic);
                  setBasis(idea.articles.length === 0 ? "story" : "wikipedia");
                  setArticles(idea.articles.slice(0, MAX_ARTICLES));
                }}
              />
            ))}
          </List>
        ) : null}
      </Section>

      <Section
        title="Written from"
        info={
          <p>
            Facts come from English Wikipedia articles — up to five. Pick them, or leave the list
            empty to use the best match for the subject. The articles are kept with the episode, so
            you can read them afterwards in the app (in Polish too). Today's news comes from the
            news feeds of TVN24, Polsat News and RMF24, told only as far as the headlines and summaries go. A story is
            made up, from no articles.
          </p>
        }
      >
        <Card>
          <Stack gap={4}>
            <Segmented
              value={basis}
              onChange={setBasis}
              options={[
                { value: "wikipedia", label: "Wikipedia" },
                { value: "news", label: "Today's news" },
                { value: "story", label: "A story" },
              ]}
            />
            {news ? (
              <Text variant="small" tone="dim">
                What's going on in Poland: the latest stories from TVN24, Polsat News and RMF24, with
                Wikipedia for background. The stories are listed with the episode.
              </Text>
            ) : story ? (
              <Text variant="small" tone="dim">
                A short made-up story about the subject.
              </Text>
            ) : (
              <>
                {articles.length > 0 ? (
                  <List>
                    {articles.map((title) => (
                      <ListItem
                        key={title}
                        leading={<Avatar>W</Avatar>}
                        title={title}
                        trailing={
                          <IconButton
                            size="sm"
                            icon={<CloseIcon />}
                            label={`Remove “${title}”`}
                            onClick={() => setArticles(articles.filter((a) => a !== title))}
                          />
                        }
                      />
                    ))}
                  </List>
                ) : (
                  <Text variant="small" tone="dim">
                    No articles picked: the best match for the subject is used.
                  </Text>
                )}
                {articles.length < MAX_ARTICLES ? (
                  <div className="composer">
                    <input
                      value={query}
                      placeholder="Find an article on Wikipedia"
                      onChange={(event) => setQuery(event.target.value)}
                      onKeyDown={(event) => event.key === "Enter" && search()}
                    />
                    <IconButton
                      icon={<SearchIcon />}
                      label="Search Wikipedia"
                      loading={busy === "search"}
                      disabled={busy !== null || !query.trim()}
                      onClick={search}
                    />
                  </div>
                ) : null}
                {hits && hits.length > 0 ? (
                  <List>
                    {hits
                      .filter((hit) => !articles.includes(hit.title))
                      .map((hit) => (
                        <ListItem
                          key={hit.title}
                          title={hit.title}
                          subtitle={<span className="clamp-2">{hit.snippet}</span>}
                          chevron={false}
                          onClick={() => {
                            addArticle(hit.title);
                            if (articles.length + 1 >= MAX_ARTICLES) setHits(null);
                          }}
                        />
                      ))}
                  </List>
                ) : hits ? (
                  <Text variant="small" tone="dim">
                    Nothing found on Wikipedia.
                  </Text>
                ) : null}
              </>
            )}
          </Stack>
        </Card>
      </Section>

      <Section title="How it sounds">
        <Card>
          <Stack gap={4}>
            <Field label="Who talks" hint={CAST_HINTS[cast]}>
              <Segmented
                value={cast}
                onChange={setCastChoice}
                options={[
                  { value: "narrator", label: "Narrator" },
                  { value: "hosts", label: "Two hosts" },
                  { value: "characters", label: "Characters" },
                ]}
              />
            </Field>
            <Field label="Kind" hint={FORMAT_HINTS[format]}>
              <Segmented
                value={format}
                onChange={setFormat}
                options={[
                  { value: "pairs", label: "English, then Polish" },
                  { value: "immersion", label: "Only Polish" },
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
            <Field label={cast === "hosts" ? "Polish voice (first host)" : "Polish voice (narrator)"}>
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
          loading={busy === "make"}
          disabled={busy !== null || !ready}
          onClick={() =>
            run("make", async () => {
              await api.podcastMake({
                topic,
                articles: story || news ? [] : articles,
                story,
                news,
                cast,
                format,
                length,
                courseId,
                vocalGender,
              });
              onCreated();
            })
          }
        >
          Make the podcast
        </Button>
        <Text variant="caption" tone="faint" align="center">
          Written by {writer || "—"} and read aloud in the background — a few minutes. You can lock the
          phone or switch apps meanwhile.
        </Text>
      </Stack>
    </Screen>
  );
}
