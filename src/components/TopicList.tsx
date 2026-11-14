import { Avatar, List, ListItem, Progress, Stack } from "../design";
import { fileUrl } from "../lib/files";
import type { TopicProgress } from "../lib/types";

/** Courses, each with how many of its blocks have been started. */
export function TopicList({
  entries,
  onOpen,
}: {
  entries: TopicProgress[];
  onOpen: (progress: TopicProgress) => void;
}) {
  return (
    <List>
      {entries.map((entry) => (
        <ListItem
          key={entry.topic.id}
          leading={
            entry.imageFileId !== null ? (
              <img className="course-thumb" src={fileUrl(entry.imageFileId)} alt="" />
            ) : (
              <Avatar muted={entry.startedBlocks === 0}>
                {(entry.topic.titlePl || entry.topic.title).charAt(0).toUpperCase()}
              </Avatar>
            )
          }
          title={entry.topic.title}
          subtitle={
            <Stack gap={2}>
              <span>
                {entry.topic.titlePl} · {entry.startedBlocks}/{entry.totalBlocks} blocks started
              </span>
              <Progress value={entry.startedBlocks} max={entry.totalBlocks} size="sm" />
            </Stack>
          }
          onClick={() => onOpen(entry)}
        />
      ))}
    </List>
  );
}
