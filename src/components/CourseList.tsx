import { Avatar, List, ListItem, Progress, Stack } from "../design";
import { fileUrl } from "../lib/files";
import type { CourseProgress } from "../lib/types";

/**
 * Courses, each with how many of its lessons have been started. Each is a card
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
                <span>
                  {entry.status === "mastered"
                    ? `🏆 Mastered · ${entry.course.titlePl}`
                    : entry.status === "completed"
                      ? `✓ Completed · ${entry.course.titlePl}${
                          entry.challenges > 0 ? ` · ${entry.challenges} challenge${entry.challenges === 1 ? "" : "s"}` : ""
                        }`
                      : `${entry.course.titlePl} · ${entry.passedCore}/${entry.coreLessons} passed`}
                </span>
                {entry.status === "active" ? (
                  <Progress value={entry.passedCore} max={Math.max(1, entry.coreLessons)} size="sm" />
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
