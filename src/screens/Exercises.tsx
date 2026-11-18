import { useEffect, useState, type ReactNode } from "react";

import { LessonRow } from "../components/LessonRow";
import { Logo, SearchIcon } from "../components/icons";
import { CourseList } from "../components/CourseList";
import {
  Avatar,
  Badge,
  Button,
  CountUp,
  EmptyState,
  Hero,
  IconButton,
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  Stack,
  Text,
  recall,
  remember,
  useStagger,
} from "../design";
import { lessonMinutes, minutes } from "../lib/lessons";
import type { Lesson, LessonOverview, Stats, CourseProgress } from "../lib/types";

function greeting(): string {
  const hour = new Date().getHours();
  if (hour < 11) return "Dzień dobry";
  if (hour < 18) return "Cześć";
  return "Dobry wieczór";
}

/** What "Add levels" asks the tutor. */
const LEVEL_ASK =
  "Add a CEFR level to every course that doesn't have one yet: read each (get_course) and set its level with edit_course. Change nothing else.";

/** Rough minutes for a list of lessons, for the summary line. */
function estimate(lessons: Lesson[]): number {
  return lessons.reduce((sum, lesson) => sum + lessonMinutes(lesson), 0);
}

/**
 * Everything to practise, top to bottom: today (with the one thing to do
 * next), what is due for a check-in, and the courses every lesson lives in.
 */
/** The Mastered shelf's tally: "7 courses mastered · 86 lessons · 1,240 words for good". */
function masteredLine(stats: Stats): string {
  const { courses, lessons, words } = stats.mastered;
  const n = (count: number, one: string, many: string) => `${count.toLocaleString()} ${count === 1 ? one : many}`;
  return `${n(courses, "course", "courses")} mastered · ${n(lessons, "lesson", "lessons")} · ${n(words, "word", "words")} for good`;
}

/** A celebration for a finished day, varied by the day. */
function celebration(day: number): string {
  const lines = [
    "Today's lessons are done! 🎉",
    "Brilliant — all done! 🎉",
    "That's the day done! 🌟",
    "Well done! 🎉",
    "All done for today! 🙌",
  ];
  return lines[((day % lines.length) + lines.length) % lines.length];
}

/**
 * Today's ring. It fills from where it stood when last seen (empty on the
 * first visit) to where it stands now, and glows when this visit is the one
 * that filled it.
 */
function GoalRing({ pct, label, children }: { pct: number; label: string; children: ReactNode }) {
  const [from] = useState(() => recall("today:ring"));
  useEffect(() => remember("today:ring", pct), [pct]);
  const filled = pct >= 100 && from !== undefined && from < 100;
  return (
    <div
      className="goal-ring"
      data-filled={filled || undefined}
      style={{ "--ring": pct, "--ring-from": from ?? 0 }}
      role="img"
      aria-label={label}
    >
      {children}
    </div>
  );
}

export function Exercises({
  overview,
  stats,
  courses,
  onOpenLesson,
  onOpenCourse,
  onOpenTutor,
  onOpenProgress,
  onOpenXpDay,
  onOpenSearch,
}: {
  overview: LessonOverview;
  stats: Stats;
  courses: CourseProgress[];
  onOpenLesson: (lesson: Lesson) => void;
  onOpenCourse: (progress: CourseProgress) => void;
  /** Opens the tutor; with `ask`, a new conversation that starts with it. */
  onOpenTutor: (ask?: string) => void;
  onOpenProgress: () => void;
  /** What earned a day's XP. */
  onOpenXpDay: (day: number) => void;
  onOpenSearch: () => void;
}) {
  // Today's lessons fill the ring as they are done.
  const total = overview.planTotal;
  const doneCount = overview.planDone;
  const finished = total > 0 && overview.due.length === 0;
  const goalPct = finished || stats.completedToday ? 100 : Math.round((doneCount / Math.max(1, total)) * 100);
  // Courses closed as mastered leave the list for their own shelf; the rest
  // stay, completed and mastered ones after those still being worked on.
  // The folder of one-offs (Other lessons) comes last of all, and only
  // when it has lessons.
  const open = courses.filter((t) => t.closedAt === null && !t.openEnded);
  // Courses without a level: the tutor levels them when asked (nothing does
  // it behind the learner's back — PRINCIPLES.md).
  const unlevelled = open.filter((t) => !t.course.level).length;
  const folders = courses.filter((t) => t.openEnded && t.totalLessons > 0);
  const active = [
    ...open.filter((t) => t.status === "active"),
    ...open.filter((t) => t.status !== "active"),
    ...folders,
  ];
  const mastered = courses.filter((t) => t.closedAt !== null).sort((a, b) => (b.closedAt ?? 0) - (a.closedAt ?? 0));
  const [showMastered, setShowMastered] = useState(false);
  // The next of today's lessons; when they are done, nothing more.
  const upNext = overview.due[0] ?? null;
  const rise = useStagger();

  return (
    <Screen
      width="wide"
      header={
        <header className="screen-header">
          <Logo size={40} />
          <div className="screen-header__titles">
            <Text variant="eyebrow">{greeting()} · a little every day</Text>
            <h1>Polski</h1>
          </div>
          <IconButton icon={<SearchIcon />} label="Search words and sentences" onClick={onOpenSearch} />
        </header>
      }
    >
      <div className="practice-layout">
        {courses.length > 0 || stats.totalXp > 0 ? (
          <div className={rise ? `practice-today ${rise}` : "practice-today"}>
            {courses.length > 0 ? (
              <Hero className="practice-today__hero">
                <Stack gap={4}>
                  <Inline gap={4}>
                    <GoalRing pct={goalPct} label={`${doneCount} of ${total} of today's lessons done`}>
                      <div>
                        <Text variant="title" weight="bold">
                          {finished ? <span className="goal-ring__done">🎉</span> : <CountUp value={doneCount} memory="today:done" />}
                        </Text>
                        {finished ? null : (
                          <Text variant="caption" tone="dim">
                            / {total}
                          </Text>
                        )}
                      </div>
                    </GoalRing>
                    <div className="grow">
                      <Inline gap={2} wrap>
                        <Text variant="eyebrow">{finished ? "Today is done ✓" : "Today"}</Text>
                        {stats.currentStreak > 0 ? (
                          <Badge>
                            🔥 <CountUp value={stats.currentStreak} memory="streak" />
                          </Badge>
                        ) : null}
                        {stats.streakFreezes > 0 ? <Badge>🧊 {stats.streakFreezes}</Badge> : null}
                      </Inline>
                      <Text variant="title" as="h2">
                        {finished
                          ? `${celebration(overview.today)}`
                          : total > 0
                            ? `${overview.due.length} lesson${overview.due.length === 1 ? "" : "s"} to go today`
                            : "Nothing for today"}
                      </Text>
                      <Text variant="small" tone="dim" as="div">
                        {finished
                          ? `All ${total} of today's lessons done. See you tomorrow.`
                          : total > 0
                            ? `About ${estimate(overview.due)} min${doneCount > 0 ? ` · ${doneCount} of ${total} done` : ""}`
                            : "Every lesson is mastered or waiting — ask the tutor for a new course."}
                      </Text>
                    </div>
                  </Inline>
                  {upNext ? (
                    <Stack gap={2}>
                      <Button variant="inverse" size="lg" wide onClick={() => onOpenLesson(upNext)}>
                        {doneCount > 0 ? "Next lesson" : "Start today's lessons"}
                      </Button>
                      <Text variant="caption" tone="dim" align="center" as="div">
                        {upNext.title}
                        {upNext.courseTitle ? ` · ${upNext.courseTitle}` : ""} · {minutes(upNext)}
                      </Text>
                    </Stack>
                  ) : null}
                </Stack>
              </Hero>
            ) : null}

            {overview.due.length > 0 ? (
              <div className="practice-today__lessons">
                <Section
                  title="Today's lessons"
                  info={
                    <>
                      <p>
                        The most important lessons for today: one that slipped last time comes first, then the ones
                        whose time has come, then — while there is room — up to two new ones. Do these and the day is
                        done.
                      </p>
                      <p>
                        Want more? Open a course: its lessons are listed by what matters most. A mastered lesson never
                        comes back, and a mastered course moves to Mastered.
                      </p>
                    </>
                  }
                >
                  <List stagger>
                    {overview.due.map((lesson) => (
                      <LessonRow
                        key={lesson.id}
                        lesson={lesson}
                        today={overview.today}
                        showCourse
                        onOpen={onOpenLesson}
                      />
                    ))}
                  </List>
                </Section>
              </div>
            ) : null}

            {courses.length > 0 || stats.totalXp > 0 ? (
              <div className="practice-today__stats">
                <List>
                  <ListItem
                    leading={<Avatar>📈</Avatar>}
                    title="Your progress"
                    subtitle={
                      <>
                        <CountUp value={stats.wordsSeen} memory="words:seen" /> words · level{" "}
                        <CountUp value={stats.level} memory="level" />
                      </>
                    }
                    trailing={
                      <Inline gap={1}>
                        <Badge tone="crimson">
                          🔥 <CountUp value={stats.currentStreak} memory="streak" />
                        </Badge>
                      </Inline>
                    }
                    chevron
                    onClick={onOpenProgress}
                  />
                  <ListItem
                    leading={<Avatar>⭐</Avatar>}
                    title={
                      <>
                        <CountUp value={stats.dayXp} memory="xp:today" /> XP today
                      </>
                    }
                    subtitle="See exactly what earned it"
                    chevron
                    onClick={() => onOpenXpDay(overview.today)}
                  />
                </List>
              </div>
            ) : null}
          </div>
        ) : null}

        <div className="practice-courses">
          <Section
            title="Courses"
            info={
              <p>
                A course is a folder of lessons on one theme: drills on its words, sentences and dialogue, and
                conversations with the examiner. Ask the tutor for a new course on anything, or to add lessons and
                drills to one you have.
              </p>
            }
          >
            {courses.length === 0 ? (
              <List>
                <EmptyState
                  icon="✨"
                  title="Build your first Polish course"
                  action={
                    <Button variant="primary" size="sm" onClick={() => onOpenTutor()}>
                      Create a course
                    </Button>
                  }
                >
                  Tell the tutor what you want to learn — everyday conversation, life in Poland, or a course you enjoy.
                </EmptyState>
              </List>
            ) : (
              <Stack gap={3}>
                {unlevelled > 0 ? (
                  <div className="practice-courses__levels">
                    <Button variant="primary" wide onClick={() => onOpenTutor(LEVEL_ASK)}>
                      {`✨ Add levels to ${unlevelled === 1 ? "1 course" : `${unlevelled} courses`}`}
                    </Button>
                  </div>
                ) : null}
                <CourseList entries={active} onOpen={onOpenCourse} />
                <div className="practice-courses__new">
                  <List>
                    <ListItem
                      leading={<Avatar>✨</Avatar>}
                      title="New course or lesson"
                      subtitle="Ask the tutor for anything — a theme, a conjugation, a grammar point"
                      onClick={() => onOpenTutor()}
                    />
                  </List>
                </div>
              </Stack>
            )}
          </Section>

          {mastered.length > 0 ? (
            <Section
              title="Mastered"
              info={
                <p>
                  Courses you mastered and closed: kept here as trophies, readable any time. Their lessons no longer
                  come back in your check-ins. To revisit a theme, ask the tutor for a new course.
                </p>
              }
            >
              <Stack gap={3}>
                <Text variant="small" tone="dim" as="div">
                  {masteredLine(stats)}
                </Text>
                {showMastered ? (
                  <CourseList entries={mastered} onOpen={onOpenCourse} />
                ) : (
                  <List>
                    <ListItem
                      leading={<Avatar>🏆</Avatar>}
                      title={`${mastered.length} mastered course${mastered.length === 1 ? "" : "s"}`}
                      subtitle="Tap to see them"
                      onClick={() => setShowMastered(true)}
                    />
                  </List>
                )}
              </Stack>
            </Section>
          ) : null}
        </div>
      </div>
    </Screen>
  );
}
