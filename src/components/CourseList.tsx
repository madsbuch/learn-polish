import { Avatar, List, ListItem, Progress, Stack } from "../design";
import { fileUrl } from "../lib/files";
import { dayToDate } from "../lib/lessons";
import type { CourseProgress } from "../lib/types";

/** Where a course stands, in a line: what is left, and what comes next. */
function courseLine(entry: CourseProgress): string {
  const line = statusLine(entry);
  return entry.course.level && entry.closedAt === null ? `${entry.course.level} · ${line}` : line;
}

/** Where a course stands, without its level. */
function statusLine(entry: CourseProgress): string {
  if (entry.closedAt !== null) {
    const level = entry.course.level ? ` · ${entry.course.level}` : "";
    return `🏆 ${entry.course.titlePl}${level} · mastered ${dayToDate(Math.floor(entry.closedAt / 86_400))}`;
  }
  if (entry.status === "mastered") return "🏆 Mastered · ready to close";
  if (entry.status === "completed") {
    const pct = Math.floor((entry.masteryPoints / Math.max(1, entry.masteryMax)) * 100);
    return `✓ Completed · ${pct}% mastered · ${entry.masteredLessons} of ${entry.coreLessons + entry.challenges} lessons`;
  }
  const left = entry.coreLessons - entry.passedCore;
  if (entry.coreLessons === 0) return entry.course.titlePl;
  return `${left} to pass${entry.nextLesson ? ` · next: ${entry.nextLesson}` : ""}`;
}

/**
 * Courses, each with where it stands. Each is a card
 * of its own, so their pictures do not run into each other.
 */
export function CourseList({
  entries,
  onOpen,
}: {
  entries: CourseProgress[];
  onOpen: (progress: CourseProgress) => void;
}) {
  return (
    <div className="course-grid">
      {entries.map((entry) => (
        <List key={entry.course.id}>
          <ListItem
            backdrop={entry.imageFileId !== null ? fileUrl(entry.imageFileId) : null}
            leading={
              entry.imageFileId !== null ? undefined : (
                <Avatar muted={entry.startedLessons === 0 && entry.status === "active"}>
                  {(entry.course.titlePl || entry.course.title).charAt(0).toUpperCase()}
                </Avatar>
              )
            }
            title={entry.course.title}
            subtitle={
              <Stack gap={2}>
                <span>{courseLine(entry)}</span>
                {entry.status === "active" ? (
                  <Progress value={entry.passedCore} max={Math.max(1, entry.coreLessons)} size="sm" />
                ) : entry.status === "completed" && entry.closedAt === null ? (
                  <Progress value={entry.masteryPoints} max={Math.max(1, entry.masteryMax)} size="sm" tone="gold" />
                ) : null}
              </Stack>
            }
            onClick={() => onOpen(entry)}
          />
        </List>
      ))}
    </div>
  );
}
