import { Avatar, Badge, ListItem } from "../design";
import { complexityTone, dueLabel, kindName, minutes } from "../lib/blocks";
import type { Block } from "../lib/types";
import { DrillIcon, NavIcon, StudyIcon } from "./icons";

/**
 * A block in a list: what kind it is, how long it takes, and when it is due.
 * `showTopic` adds the topic it belongs to, for mixed lists.
 */
export function BlockRow({
  block,
  today,
  showTopic,
  onOpen,
}: {
  block: Block;
  today: number;
  showTopic?: boolean;
  onOpen: (block: Block) => void;
}) {
  const due = dueLabel(block, today);
  const parts = [
    showTopic && block.topicTitle ? block.topicTitle : kindName(block),
    block.lastResult === "fail" ? "needs another go" : due,
  ];
  return (
    <ListItem
      leading={
        <Avatar muted={block.reps === 0}>
          {block.format === "song" ? <NavIcon.songs /> : block.kind === "study" ? <StudyIcon /> : <DrillIcon />}
        </Avatar>
      }
      title={block.title}
      subtitle={parts.join(" · ")}
      trailing={<Badge tone={complexityTone(block.complexity)}>{minutes(block.complexity)}</Badge>}
      chevron
      onClick={() => onOpen(block)}
    />
  );
}
