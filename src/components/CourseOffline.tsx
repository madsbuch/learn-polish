import { useCallback, useEffect, useState } from "react";

import { Banner, Button, Progress, Section, SettingRow, Stack, Switch, Text } from "../design";
import { api, errorMessage } from "../lib/api";
import { onEvent } from "../lib/events";
import { askForNotificationsToFollowDownloads } from "../lib/notifications";
import { useSyncReload } from "../lib/sync";
import { z } from "zod";
import { CourseIdSchema } from "../lib/types";
import type { CourseId, OfflineStatus } from "../lib/types";

/** How often the count is read again while a download is on its way. */
const WATCH_MS = 3000;

function plural(count: number, one: string, many: string) {
  return `${count} ${count === 1 ? one : many}`;
}

/** What is stored and what is still to come, in a sentence. */
function summary(status: OfflineStatus): string {
  const parts = [`${status.clipsStored} of ${plural(status.clips, "audio clip", "audio clips")}`];
  if (status.examples > 0) parts.push(`example sentences for ${status.examplesStored} of ${plural(status.examples, "word", "words")}`);
  if (status.pictures > 0) parts.push(`${status.picturesStored} of ${plural(status.pictures, "picture", "pictures")}`);
  const last = parts.pop();
  return `${parts.length > 0 ? `${parts.join(", ")} and ${last}` : last} downloaded.`;
}

/**
 * Keeping a course ready for use without a connection (offline.rs): a
 * switch, how much of the course's audio and pictures is stored, and what
 * will still need a connection.
 */
// Spec: OFF-1, OFF-5, OFF-9, OFF-10, OFF-16
export function CourseOffline({ courseId }: { courseId: CourseId }) {
  const [status, setStatus] = useState<OfflineStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    api
      .courseOffline(courseId)
      .then(setStatus)
      .catch(() => undefined);
  }, [courseId]);

  useEffect(() => {
    reload();
    const off = onEvent(z.object({ courseId: CourseIdSchema }), "offline://changed", (event) => {
      if (event.payload.courseId === courseId) reload();
    });
    return () => {
      void off.then((stop) => stop());
    };
  }, [courseId, reload]);
  // What another device downloaded arrives by sync.
  useSyncReload(reload);

  // While it is kept offline and not all there, the count moves: pictures
  // land one by one, each from its own piece of background work.
  const watching = status !== null && status.enabled && !status.ready && status.canDownload;
  useEffect(() => {
    if (!watching) return;
    const timer = window.setInterval(reload, WATCH_MS);
    return () => window.clearInterval(timer);
  }, [watching, reload]);

  if (!status) return null;
  const total = status.clips + status.examples + status.pictures;
  const stored = status.clipsStored + status.examplesStored + status.picturesStored;

  async function act(action: () => Promise<OfflineStatus>, downloads: boolean) {
    setError(null);
    // The download's progress shows in a notification (Android): asked for
    // as the learner starts one, never in the way of it.
    if (downloads) void askForNotificationsToFollowDownloads();
    try {
      setStatus(await action());
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }

  return (
    <Section
      title="Offline"
      info={
        <Stack gap={2}>
          <p>
            A course kept offline has everything its lessons play and show downloaded: the audio of every word,
            sentence and line, at normal speed and slowly; the example sentences shown after a wrong answer; and
            every picture. New lessons are downloaded as they are added.
          </p>
          <p>
            Downloading uses your OpenRouter key, like tapping each speaker button would. Switching it off deletes
            nothing.
          </p>
          <p>
            Conversation lessons (study, listening and writing) talk to an AI model, so they need a connection
            whatever is downloaded.
          </p>
        </Stack>
      }
    >
      <Stack gap={2}>
        <SettingRow
          label="Keep this course offline"
          description={
            status.enabled && status.ready ? "Ready to use without a connection" : "Download its audio, examples and pictures"
          }
          control={
            <Switch
              label="Keep this course offline"
              checked={status.enabled}
              onChange={(on) => void act(() => api.courseOfflineSet(courseId, on), on)}
            />
          }
        />
        {status.enabled && total > 0 ? (
          <Stack gap={1}>
            <Progress value={stored} max={total} tone="mint" size="sm" />
            <Text variant="small" tone="dim">
              {status.working ? "Downloading… " : ""}
              {summary(status)}
            </Text>
          </Stack>
        ) : null}
        {status.enabled && !status.ready && !status.canDownload ? (
          <Banner>Add an OpenRouter key in Settings to download what is missing.</Banner>
        ) : null}
        {status.enabled && status.canDownload && status.needsModel ? (
          <Banner>Choose a chat model in Settings to have the example sentences written.</Banner>
        ) : null}
        {status.enabled && !status.ready && status.canDownload && !status.working ? (
          <Button wide onClick={() => void act(() => api.courseOfflineDownload(courseId), true)}>
            Download now
          </Button>
        ) : null}
        {status.enabled && status.onlineLessons > 0 ? (
          <Text variant="small" tone="dim">
            {plural(status.onlineLessons, "conversation lesson needs", "conversation lessons need")} a connection.
          </Text>
        ) : null}
        {error ? <Banner tone="error">{error}</Banner> : null}
      </Stack>
    </Section>
  );
}
