import { Avatar, List, ListItem, Progress, Stack } from "../design";
import { fileUrl } from "../lib/files";
import type { TopicProgress } from "../lib/types";

/**
 * Courses, each with how many of its blocks have been started. Each is a card
 * of its own, so their pictures do not run into each other.
 */
export function TopicList({
  entries,
  onOpen,
}: {
  entries: TopicProgress[];
  onOpen: (progress: TopicProgress) => void;
}) {
  return (
    <Stack gap={3}>
      {entries.map((entry) => (
        <List key={entry.topic.id}>
          <ListItem
            backdrop={entry.imageFileId !== null ? fileUrl(entry.imageFileId) : null}
            leading={
              entry.imageFileId !== null ? undefined : (
                <Avatar muted={entry.startedBlocks === 0 && entry.status === "active"}>
                  {(entry.topic.titlePl || entry.topic.title).charAt(0).toUpperCase()}
                </Avatar>
              )
            }
            title={entry.topic.title}
            subtitle={
              <Stack gap={2}>
                <span>
                  {entry.status === "mastered"
                    ? `🏆 Mastered · ${entry.topic.titlePl}`
                    : entry.status === "completed"
                      ? `✓ Completed · ${entry.topic.titlePl}${
                          entry.challenges > 0 ? ` · ${entry.challenges} challenge${entry.challenges === 1 ? "" : "s"}` : ""
                        }`
                      : `${entry.topic.titlePl} · ${entry.passedCore}/${entry.coreBlocks} passed`}
                </span>
                {entry.status === "active" ? (
                  <Progress value={entry.passedCore} max={Math.max(1, entry.coreBlocks)} size="sm" />
                ) : null}
              </Stack>
            }
            onClick={() => onOpen(entry)}
          />
        </List>
      ))}
    </Stack>
  );
}
