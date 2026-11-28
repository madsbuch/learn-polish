import { useCallback, useEffect, useState } from "react";

import { Cover } from "../components/Cover";
import { PlayerIcon } from "../components/icons";
import {
  Banner,
  Button,
  Hero,
  IconButton,
  List,
  ListItem,
  Screen,
  Section,
  Segmented,
  SettingRow,
  Spinner,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { onSignal } from "../lib/events";
import { currentItem, formatTime, itemTitle, toggle, usePlayer, type QueueItem } from "../lib/player";
import { radioOn, tuneIn, tuneOut } from "../lib/radio";
import type { MakingView, RadioFormat, RadioState, SongId } from "../lib/schemas";

const MAKING_STATUS: Record<string, string> = {
  generating: "Being made",
  ready: "Ready — playing soon",
  failed: "Could not be made",
};

const WHAT: Record<MakingView["what"], string> = {
  song: "New song",
  podcast: "New podcast",
  news: "Today's news",
};

/**
 * The radio: tune in, see what plays and what comes next, and what the
 * radio is making for the library (RADIO-10).
 */
// Spec: RADIO-10
export function RadioScreen({
  onBack,
  onOpenPlayer,
  onOpenSong,
}: {
  onBack: () => void;
  onOpenPlayer: () => void;
  onOpenSong: (songId: SongId) => void;
}) {
  const player = usePlayer();
  const [radio, setRadio] = useState<RadioState | null>(null);
  const [format, setFormat] = useState<RadioFormat | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    void api
      .radioState()
      .then(setRadio)
      .catch((cause: unknown) => setError(errorMessage(cause)));
  }, []);

  useEffect(() => {
    load();
    void api
      .getSettings()
      .then((settings) => setFormat(settings.radioFormat))
      .catch(() => {});
    const unlisten = onSignal("radio://changed", load);
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [load]);

  const on = radioOn() && player.owner === "radio" && (radio?.on ?? false);

  async function start() {
    setStarting(true);
    setError(null);
    try {
      await tuneIn();
      load();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setStarting(false);
    }
  }

  function changeFormat(next: RadioFormat) {
    setFormat(next);
    void api.updateSettings({ radioFormat: next }).catch((cause: unknown) => setError(errorMessage(cause)));
  }

  const current = on ? currentItem(player) : null;
  const upNext: QueueItem[] = on ? player.queue.slice(player.index + 1) : [];

  return (
    <Screen
      title="Radio"
      onBack={onBack}
      nav={false}
      info={
        <>
          <p>
            Polski Radio plays your songs and podcasts, and keeps making new ones for you while it plays: songs from
            the words you are learning, podcasts on what interests you, and the day's news.
          </p>
          <p>
            Short spoken bridges link what plays, in Polish with the English. Everything new lands in your library
            and stays there; the radio orders nothing while it is paused.
          </p>
        </>
      }
    >
      {error ? <Banner tone="error">{error}</Banner> : null}

      {!on ? (
        <Hero>
          <Stack gap={2} align="center">
            <h2 className="t-center">Polski Radio</h2>
            <Text align="center" tone="dim">
              Your library, new songs and podcasts, and the news — in Polish, between your other things.
            </Text>
            <Button variant="primary" wide loading={starting} onClick={() => void start()}>
              Tune in
            </Button>
          </Stack>
        </Hero>
      ) : (
        <>
          {radio?.note ? <Banner tone="info">{radio.note}</Banner> : null}
          <Section title="On air">
            <List>
              {current ? (
                <ListItem
                  leading={<Cover fileId={current.track?.imageFileId ?? null} size={44} />}
                  title={current.bridge ? "Bridge" : current.song.title}
                  subtitle={itemTitle(current).artist}
                  label="Open the player"
                  onClick={onOpenPlayer}
                  trailing={
                    <IconButton
                      icon={player.playing ? <PlayerIcon.pause size={20} /> : <PlayerIcon.play size={20} />}
                      label={player.playing ? "Pause" : "Play"}
                      onClick={toggle}
                    />
                  }
                />
              ) : null}
            </List>
          </Section>

          <Section title="Coming up">
            <List>
              {upNext.map((item, index) =>
                item.bridge ? (
                  <ListItem
                    key={`${item.bridge.id}-${index}`}
                    title="Bridge"
                    subtitle={item.bridge.lines[0]?.pl ?? ""}
                    chevron={false}
                  />
                ) : (
                  <ListItem
                    key={`${item.track.id}-${index}`}
                    leading={<Cover fileId={item.track.imageFileId} size={44} />}
                    title={item.song.title}
                    subtitle={`${item.song.chorusPl} · ${formatTime(item.track.duration)}`}
                    onClick={() => onOpenSong(item.song.id)}
                  />
                ),
              )}
              {(radio?.planned ?? []).map((planned, index) => {
                const songId = planned.songId;
                return (
                  <ListItem
                    key={`planned-${index}`}
                    title={planned.kind === "bridge" ? "Bridge" : planned.title}
                    subtitle={planned.kind === "bridge" ? planned.title : planned.detail}
                    chevron={songId !== null}
                    onClick={songId !== null ? () => onOpenSong(songId) : undefined}
                  />
                );
              })}
              {radio?.thinking ? (
                <ListItem leading={<Spinner />} title="Choosing what comes next…" chevron={false} />
              ) : null}
            </List>
          </Section>

          {radio && radio.making.length > 0 ? (
            <Section title="Being made for you">
              <List>
                {radio.making.map((made) => (
                  <ListItem
                    key={made.songId}
                    leading={made.status === "generating" ? <Spinner /> : undefined}
                    title={made.title || WHAT[made.what]}
                    subtitle={`${WHAT[made.what]} · ${MAKING_STATUS[made.status] ?? made.status}`}
                    onClick={() => onOpenSong(made.songId)}
                  />
                ))}
              </List>
            </Section>
          ) : null}

          <Button variant="secondary" wide onClick={tuneOut}>
            Turn off the radio
          </Button>
        </>
      )}

      {format ? (
        <Section title="Bridges">
          <List>
            <SettingRow
              label="How bridges are spoken"
              description="Each line in English, then in Polish; or only in Polish, with the English shown."
              stacked
              control={
                <Segmented
                  value={format}
                  onChange={changeFormat}
                  options={[
                    { value: "pairs", label: "English, then Polish" },
                    { value: "immersion", label: "Only Polish" },
                  ]}
                />
              }
            />
          </List>
        </Section>
      ) : null}
    </Screen>
  );
}
