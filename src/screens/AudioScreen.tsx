import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { CheckIcon, PlayerIcon } from "../components/icons";
import { SpeakButton } from "../components/ui";
import {
  Banner,
  Button,
  ConfirmButton,
  List,
  ListEmpty,
  ListItem,
  Progress,
  Screen,
  Section,
  Segmented,
  SettingRow,
  Spinner,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import type { AudioItem, AudioOverview, CacheProgress, CourseAudio } from "../lib/types";

export function megabytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

const KIND_NAMES: Record<AudioItem["kind"], string> = {
  word: "Word",
  sentence: "Sentence",
  line: "Dialogue",
};

/** Download progress, reported line by line while a batch is made. */
function useCacheProgress(onFinished: () => void): CacheProgress | null {
  const [progress, setProgress] = useState<CacheProgress | null>(null);
  useEffect(() => {
    const unlisten = listen<CacheProgress>("tts://progress", (event) => {
      setProgress(event.payload);
      if (event.payload.finished) onFinished();
    });
    return () => {
      void unlisten.then((off) => off());
    };
  }, [onFinished]);
  return progress;
}

function Downloading({ progress }: { progress: CacheProgress | null }) {
  if (!progress || progress.finished) return null;
  return (
    <Stack gap={1}>
      <Progress value={progress.done} max={Math.max(1, progress.total)} />
      <Text variant="caption" tone="faint">
        Downloading {progress.done} of {progress.total}…
        {progress.failed > 0 ? ` ${progress.failed} failed.` : ""}
      </Text>
    </Stack>
  );
}

/**
 * Downloaded audio: what is stored, per course and per line. Audio is made
 * the first time a line is played and kept, so most of it arrives on
 * demand; here it can be seen, filled in ahead of time, or cleared.
 */
export function AudioScreen({ onBack }: { onBack: () => void }) {
  const [overview, setOverview] = useState<AudioOverview | null>(null);
  const [open, setOpen] = useState<CourseAudio | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api
      .audioOverview()
      .then((next) => {
        setOverview(next);
        // Keep an open course's numbers current.
        setOpen((current) => (current ? (next.courses.find((c) => c.topicId === current.topicId) ?? current) : null));
      })
      .catch((cause) => setError(errorMessage(cause)));
  }, []);
  useEffect(refresh, [refresh]);
  const progress = useCacheProgress(refresh);

  async function run(action: () => Promise<string | void>) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const message = await action();
      if (message) setNotice(message);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
      refresh();
    }
  }

  if (open) {
    return (
      <CourseAudioPage
        course={open}
        progress={progress}
        onBack={() => {
          setOpen(null);
          refresh();
        }}
        onChanged={refresh}
      />
    );
  }

  const missing = overview ? overview.courses.reduce((n, c) => n + c.total - c.stored, 0) : 0;
  const total = overview ? overview.courses.reduce((n, c) => n + c.total, 0) : 0;

  return (
    <Screen
      title="Downloaded audio"
      onBack={onBack}
      nav={false}
      info={
        <p>
          A line's audio is made the first time it's played, then kept: it replays instantly,
          for free and offline. Download a course to have all of it ready ahead of time.
        </p>
      }
    >
      {error ? <Banner tone="error">{error}</Banner> : null}
      {notice ? <Banner tone="ok">{notice}</Banner> : null}
      {overview === null ? (
        <Spinner />
      ) : (
        <>
          <List>
            <SettingRow
              label={`${total - missing} of ${total} lines`}
              description={`${overview.clips} clips stored`}
              value={megabytes(overview.bytes)}
              stacked
              control={
                progress && !progress.finished ? (
                  <Downloading progress={progress} />
                ) : (
                  <Progress value={total - missing} max={Math.max(1, total)} tone="mint" />
                )
              }
            />
          </List>

          <Section title="Courses">
            <List>
              {overview.courses.map((course) => (
                <ListItem
                  key={course.topicId}
                  oneLine
                  title={course.titlePl}
                  leading={
                    course.stored === course.total ? (
                      <span className="tone-ok" aria-label="All downloaded">
                        <CheckIcon />
                      </span>
                    ) : (
                      <span className="audio-missing" aria-label="Not all downloaded" />
                    )
                  }
                  subtitle={`${course.stored === course.total ? "All" : `${course.stored} of`} ${course.total}${
                    course.bytes ? ` · ${megabytes(course.bytes)}` : ""
                  }`}
                  onClick={() => setOpen(course)}
                />
              ))}
              {overview.courses.length === 0 ? <ListEmpty>No courses with anything to read out.</ListEmpty> : null}
            </List>
          </Section>

          {overview.unusedClips > 0 ? (
            <Section
              title="Not used"
              info={
                <p>
                  Clips no course uses with your current voices: lines read in a voice you no
                  longer use, deleted words and sentences, and voice samples.
                </p>
              }
            >
              <List>
                <SettingRow
                  label={`${overview.unusedClips} clips`}
                  value={megabytes(overview.unusedBytes)}
                />
              </List>
              <ConfirmButton
                label="Delete unused clips"
                confirmLabel="Delete"
                question={`Delete ${overview.unusedClips} clips no course uses?`}
                block
                onConfirm={() =>
                  run(async () => {
                    const deleted = await api.deleteUnusedAudio();
                    return `Deleted ${deleted} unused clips.`;
                  })
                }
              />
            </Section>
          ) : null}

          <Section title="Everything">
            <Stack gap={2}>
              <Button
                block
                icon={<PlayerIcon.download />}
                loading={busy && Boolean(progress && !progress.finished)}
                disabled={busy || missing === 0}
                onClick={() =>
                  run(async () => {
                    const lines = [];
                    for (const course of overview.courses) {
                      if (course.stored < course.total) lines.push(...(await api.topicAudioLines(course.topicId)));
                    }
                    const result = await api.cacheAudio(lines);
                    return result.error
                      ? `Downloaded ${result.done}. Stopped after an error: ${result.error}`
                      : `Downloaded ${result.done} clips.`;
                  })
                }
              >
                {missing === 0 ? "Everything is downloaded" : `Download the missing ${missing}`}
              </Button>
              <ConfirmButton
                label="Delete all downloaded audio"
                confirmLabel="Delete"
                question="Delete every stored clip? Each is made again the next time it's played."
                block
                onConfirm={() =>
                  run(async () => {
                    await api.clearAudioCache();
                    return "Downloaded audio deleted.";
                  })
                }
              />
            </Stack>
          </Section>
        </>
      )}
    </Screen>
  );
}

type Filter = "all" | "missing" | "stored";

/** One course's lines, each marked downloaded or not. */
function CourseAudioPage({
  course,
  progress,
  onBack,
  onChanged,
}: {
  course: CourseAudio;
  progress: CacheProgress | null;
  onBack: () => void;
  onChanged: () => void;
}) {
  const [items, setItems] = useState<AudioItem[] | null>(null);
  const [filter, setFilter] = useState<Filter>("all");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .courseAudio(course.topicId)
      .then(setItems)
      .catch((cause) => setError(errorMessage(cause)));
  }, [course.topicId]);
  useEffect(load, [load]);
  // Tick items off as a download goes.
  useEffect(() => {
    if (progress) load();
  }, [progress, load]);

  const missing = items?.filter((item) => !item.stored) ?? [];
  const shown = (items ?? []).filter((item) =>
    filter === "all" ? true : filter === "missing" ? !item.stored : item.stored,
  );
  const stored = (items?.length ?? 0) - missing.length;

  return (
    <Screen title={course.titlePl} subtitle={course.title} onBack={onBack} nav={false}>
      {error ? <Banner tone="error">{error}</Banner> : null}
      {items === null ? (
        <Spinner />
      ) : (
        <>
          <List>
            <SettingRow
              label={`${stored} of ${items.length} downloaded`}
              value={megabytes(items.reduce((n, item) => n + item.bytes, 0))}
              stacked
              control={
                busy ? (
                  <Downloading progress={progress} />
                ) : (
                  <Progress value={stored} max={Math.max(1, items.length)} tone="mint" />
                )
              }
            />
          </List>
          <Button
            variant={missing.length > 0 ? "primary" : "secondary"}
            block
            icon={<PlayerIcon.download />}
            loading={busy}
            disabled={missing.length === 0}
            onClick={async () => {
              setBusy(true);
              setError(null);
              try {
                const result = await api.cacheAudio(missing.map(({ text, gender }) => ({ text, gender })));
                if (result.error) setError(`Stopped after an error: ${result.error}`);
              } catch (cause) {
                setError(errorMessage(cause));
              } finally {
                setBusy(false);
                load();
                onChanged();
              }
            }}
          >
            {missing.length === 0 ? "All downloaded" : `Download the missing ${missing.length}`}
          </Button>

          <Segmented
            value={filter}
            onChange={setFilter}
            options={[
              { value: "all", label: `All ${items.length}` },
              { value: "missing", label: `Missing ${missing.length}` },
              { value: "stored", label: `Downloaded ${stored}` },
            ]}
          />
          <List>
            {shown.map((item) => (
              <ListItem
                key={`${item.gender}:${item.text}`}
                selectable
                leading={
                  item.stored ? (
                    <span className="tone-ok" aria-label="Downloaded">
                      <CheckIcon />
                    </span>
                  ) : (
                    <span className="audio-missing" aria-label="Not downloaded" />
                  )
                }
                title={item.text}
                subtitle={`${KIND_NAMES[item.kind]}${item.gender === "m" ? " · male voice" : ""}${
                  item.stored ? ` · ${megabytes(item.bytes)}` : " · not downloaded"
                }`}
                trailing={
                  <SpeakButton
                    text={item.text}
                    gender={item.gender || undefined}
                    onPlayed={() => {
                      if (!item.stored) {
                        load();
                        onChanged();
                      }
                    }}
                  />
                }
              />
            ))}
            {shown.length === 0 ? (
              <ListEmpty>{filter === "missing" ? "Nothing missing." : "Nothing here yet."}</ListEmpty>
            ) : null}
          </List>
        </>
      )}
    </Screen>
  );
}
