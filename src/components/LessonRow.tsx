import type { ReactNode } from "react";

import { Avatar, Badge, ListItem } from "../design";
import { complexityTone, dueLabel, kindName, minutes } from "../lib/lessons";
import type { Lesson } from "../lib/types";
import { lessonType } from "../lib/lessonTypes";
import { DrillIcon, NavIcon, PictureIcon, StudyIcon, WriteIcon } from "./icons";

/**
 * A lesson in a list: what kind it is, how long it takes, and when it is due.
 * `showCourse` adds the course it belongs to, for mixed lists; `status`
 * replaces when it is due (a course's list says where it stands instead),
 * and `trailing` the time it takes.
 */
export function LessonRow({
  lesson,
  today,
  showCourse,
  status,
  trailing,
  onOpen,
}: {
  lesson: Lesson;
  today: number;
  showCourse?: boolean;
  status?: string;
  trailing?: ReactNode;
  onOpen: (lesson: Lesson) => void;
}) {
  const due = dueLabel(lesson, today);
  const parts = [
    showCourse && lesson.courseTitle ? lesson.courseTitle : kindName(lesson),
    status ?? (lesson.lastResult === "fail" ? "needs another go" : due),
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
      trailing={trailing ?? <Badge tone={complexityTone(lesson.complexity)}>{minutes(lesson.complexity)}</Badge>}
      chevron
      onClick={() => onOpen(lesson)}
    />
  );
}

/** Each type's icon, by the icon name it registers (lesson_types). */
const TYPE_ICONS: Record<string, () => ReactNode> = {
  drill: () => <DrillIcon />,
  study: () => <StudyIcon />,
  write: () => <WriteIcon />,
  song: () => <NavIcon.songs />,
  picture: () => <PictureIcon />,
};

/** What type of lesson it is, as an icon. */
export function LessonIcon({ lesson }: { lesson: Pick<Lesson, "type" | "engine"> }) {
  return (TYPE_ICONS[lessonType(lesson).icon] ?? TYPE_ICONS.drill)();
}
