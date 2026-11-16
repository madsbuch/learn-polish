import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Avatar, Button, Card, List, ListItem, Section, Segmented, SettingRow, Stack, Switch, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { toast } from "../lib/toast";
import type { PodcastAudio, Song, WikiSource } from "../lib/types";
import { SourceList } from "./Sources";

/**
 * The Wikipedia articles an episode was written from, kept with it so they
 * can be read afterwards — in the app's reader, in English or in Polish. A
 * story has none, and shows nothing.
 */
export function EpisodeSources({ song, onRead }: { song: Song; onRead: (source: WikiSource) => void }) {
  const [sources, setSources] = useState<WikiSource[] | null>(null);
  useEffect(() => {
    api
      .songSources(song.id)
      .then(setSources)
      .catch(() => setSources([]));
    // The articles are kept while the episode is made: look again when it is.
  }, [song.id, song.status]);

  const linked = song.origin?.sources ?? [];
  if (sources === null || (sources.length === 0 && linked.length === 0)) return null;

  const info = (
    <p>
      The episode is written from these Wikipedia articles. Read them here — in Polish too, when
      Polish Wikipedia has the article — or open them on Wikipedia.
    </p>
  );
  if (sources.length > 0) {
    return (
      <Section title="Sources" info={info}>
        <SourceList sources={sources} onRead={onRead} />
      </Section>
    );
  }
  // Not kept in the app (it could not be fetched yet): the links still work.
  return (
    <Section title="Sources" info={info}>
      <List>
        {linked.map((source) => (
          <ListItem
            key={source.url}
            leading={<Avatar>W</Avatar>}
            title={source.title}
            subtitle="Opens on Wikipedia"
            onClick={() => void openUrl(source.url).catch(() => toast("Could not open the browser.", "error"))}
          />
        ))}
      </List>
    </Section>
  );
}

const PAUSES = [0, 0.8, 1.5, 2.5, 4];

function same(a: PodcastAudio, b: PodcastAudio): boolean {
  return (
    a.english === b.english &&
    a.pause === b.pause &&
    a.repeats === b.repeats &&
    a.slow === b.slow &&
    a.voices === b.voices
  );
}

const FALLBACK: PodcastAudio = { english: true, pause: 0.8, repeats: 1, slow: false, voices: "two" };

/**
 * How an episode sounds: whether the English is said, the pause to answer
 * in, how often and how fast the Polish is said, and the voices. Changing it
 * puts the audio together again from the sentences already read — new ones
 * only for a new speed or voice.
 */
export function PodcastSound({ song, onChanged }: { song: Song; onChanged: () => void }) {
  const saved: PodcastAudio = { ...FALLBACK, ...song.podcast };
  const [audio, setAudio] = useState<PodcastAudio>(saved);
  const [busy, setBusy] = useState(false);
  // A new version of the episode (or another episode) resets the controls.
  useEffect(
    () => setAudio(saved),
    [song.id, saved.english, saved.pause, saved.repeats, saved.slow, saved.voices],
  );

  const nearest = PAUSES.reduce((best, p) => (Math.abs(p - audio.pause) < Math.abs(best - audio.pause) ? p : best));
  const generating = song.status === "generating";

  return (
    <Section
      title="Sound"
      info={
        <p>
          Try what helps you most. A pause before the Polish gives you time to say it yourself
          first; hearing it twice, or slowly, makes it easier to catch. Without the English you
          hear only Polish, with the meaning in the lyrics. Making the audio again reuses every
          sentence already read, so it is quick and free — only a new speed or voice has new
          sentences read.
        </p>
      }
    >
      <Card>
        <Stack gap={4}>
          <SettingRow
            label="Say the English"
            description="Off: only Polish is heard; the meaning shows in the lyrics."
            control={
              <Switch
                checked={audio.english}
                label="Say the English"
                onChange={(english) => setAudio({ ...audio, english })}
              />
            }
          />
          {audio.english ? (
            <SettingRow
              stacked
              label="Pause before the Polish"
              description="Time to say it yourself first."
              control={
                <Segmented
                  value={String(nearest)}
                  onChange={(value) => setAudio({ ...audio, pause: Number(value) })}
                  options={PAUSES.map((p) => ({ value: String(p), label: p === 0 ? "None" : `${p} s` }))}
                />
              }
            />
          ) : null}
          <SettingRow
            stacked
            label="Say the Polish"
            control={
              <Segmented
                value={String(audio.repeats)}
                onChange={(value) => setAudio({ ...audio, repeats: Number(value) })}
                options={[
                  { value: "1", label: "Once" },
                  { value: "2", label: "Twice" },
                  { value: "3", label: "3 times" },
                ]}
              />
            }
          />
          <SettingRow
            label="Slow Polish"
            description="Every syllable clear, a little gap between words."
            control={
              <Switch checked={audio.slow} label="Slow Polish" onChange={(slow) => setAudio({ ...audio, slow })} />
            }
          />
          {audio.english ? (
            <SettingRow
              stacked
              label="Voices"
              control={
                <Segmented
                  value={audio.voices}
                  onChange={(voices) => setAudio({ ...audio, voices })}
                  options={[
                    { value: "two", label: "English and Polish apart" },
                    { value: "one", label: "One voice" },
                  ]}
                />
              }
            />
          ) : null}
          <Button
            wide
            loading={busy || generating}
            disabled={busy || generating || (same(audio, saved) && song.status === "ready")}
            onClick={async () => {
              setBusy(true);
              try {
                await api.podcastRerender(song.id, audio);
                onChanged();
              } catch (cause) {
                toast(errorMessage(cause), "error");
              } finally {
                setBusy(false);
              }
            }}
          >
            {generating ? "Making the audio…" : "Make the audio again"}
          </Button>
          {!same(audio, saved) ? (
            <Text variant="caption" tone="faint" align="center">
              Not applied yet.
            </Text>
          ) : null}
        </Stack>
      </Card>
    </Section>
  );
}
