import { Avatar, List, ListItem, Progress, Stack } from "../design";
import type { TopicProgress } from "../lib/types";

/** Course topics and programmes, each with how much of it has been met. */
export function TopicList({
  entries,
  focusTopicId,
  onOpen,
}: {
  entries: TopicProgress[];
  focusTopicId: number | null;
  onOpen: (progress: TopicProgress) => void;
}) {
  return (
    <List>
      {entries.map((entry) => (
        <ListItem
          key={entry.topic.id}
          leading={
            <Avatar muted={entry.seenWords === 0}>
              {entry.topic.kind === "course" ? entry.topic.ord : "✦"}
            </Avatar>
          }
          title={entry.topic.title}
          subtitle={
            <Stack gap={2}>
              <span>
                {entry.topic.titlePl} · {entry.seenWords}/{entry.totalWords} words
                {entry.topic.id === focusTopicId ? " · daily focus" : ""}
              </span>
              <Progress value={entry.seenWords} max={entry.totalWords} size="sm" />
            </Stack>
          }
          onClick={() => onOpen(entry)}
        />
      ))}
    </List>
  );
}
