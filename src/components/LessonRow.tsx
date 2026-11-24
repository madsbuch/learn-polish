import type { ReactNode } from "react";

import { Avatar, Badge, ListItem } from "../design";
import { CHALLENGE_LABEL, complexityTone, dueLabel, kindName, minutes } from "../lib/lessons";
import type { Lesson } from "../lib/schemas";
import { lessonType } from "../lib/lessonTypes";
import { ChallengeIcon, DrillIcon, NavIcon, PictureIcon, SceneIcon, StoryIcon, StudyIcon, WriteIcon } from "./icons";

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
  // Spec: LSN-12
  const title = lesson.challenge ? (
    <>
      {lesson.title} <ChallengeMark />
    </>
  ) : (
    lesson.title
  );
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
      title={title}
      subtitle={parts.join(" · ")}
      trailing={trailing ?? <Badge tone={complexityTone(lesson.complexity)}>{minutes(lesson)}</Badge>}
      chevron
      onClick={() => onOpen(lesson)}
    />
  );
}

/** The mark on a challenge, wherever a lesson is listed or shown. */
// Spec: LSN-12, LSN-13
export function ChallengeMark() {
  return (
    <Badge tone="gold" outline icon={<ChallengeIcon />}>
      {CHALLENGE_LABEL}
    </Badge>
  );
}

/** Each type's icon, by the icon name it registers (lesson_types). */
const TYPE_ICONS: Record<string, () => ReactNode> = {
  drill: () => <DrillIcon />,
  study: () => <StudyIcon />,
  write: () => <WriteIcon />,
  song: () => <NavIcon.songs />,
  picture: () => <PictureIcon />,
  scene: () => <SceneIcon />,
  story: () => <StoryIcon />,
};

/** What type of lesson it is, as an icon. */
export function LessonIcon({ lesson }: { lesson: Pick<Lesson, "type" | "engine"> }) {
  return (TYPE_ICONS[lessonType(lesson).icon] ?? TYPE_ICONS.drill)();
}
