import { Avatar, Badge, ListItem } from "../design";
import { complexityTone, dueLabel, kindName, minutes } from "../lib/lessons";
import type { Lesson } from "../lib/types";
import { DrillIcon, NavIcon, StudyIcon, WriteIcon } from "./icons";

/**
 * A lesson in a list: what kind it is, how long it takes, and when it is due.
 * `showCourse` adds the course it belongs to, for mixed lists.
 */
export function LessonRow({
  lesson,
  today,
  showCourse,
  onOpen,
}: {
  lesson: Lesson;
  today: number;
  showCourse?: boolean;
  onOpen: (lesson: Lesson) => void;
}) {
  const due = dueLabel(lesson, today);
  const parts = [
    showCourse && lesson.courseTitle ? lesson.courseTitle : kindName(lesson),
    lesson.lastResult === "fail" ? "needs another go" : due,
  ];
  return (
    <ListItem
      leading={
        <Avatar muted={lesson.reps === 0}>
          <LessonIcon lesson={lesson} />
        </Avatar>
      }
      title={lesson.title}
      subtitle={parts.join(" · ")}
      trailing={<Badge tone={complexityTone(lesson.complexity)}>{minutes(lesson.complexity)}</Badge>}
      chevron
      onClick={() => onOpen(lesson)}
    />
  );
}

/** What kind of lesson it is, as an icon. */
export function LessonIcon({ lesson }: { lesson: Pick<Lesson, "kind" | "format"> }) {
  if (lesson.format === "song") return <NavIcon.songs />;
  if (lesson.format === "writing") return <WriteIcon />;
  return lesson.kind === "study" ? <StudyIcon /> : <DrillIcon />;
}
