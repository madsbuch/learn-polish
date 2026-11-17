import { useState, type CSSProperties } from "react";

import { LessonRow } from "../components/LessonRow";
import { Logo, SearchIcon } from "../components/icons";
import { CourseList } from "../components/CourseList";
import {
  Avatar,
  Badge,
  Button,
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
} from "../design";
import { minutes } from "../lib/lessons";
import type { Lesson, LessonOverview, Stats, CourseProgress } from "../lib/types";

function greeting(): string {
  const hour = new Date().getHours();
  if (hour < 11) return "Dzień dobry";
  if (hour < 18) return "Cześć";
  return "Dobry wieczór";
}

/** Rough minutes for a list of lessons, for the summary line. */
function estimate(lessons: Lesson[]): number {
  return lessons.reduce((sum, b) => sum + (b.complexity === "low" ? 2 : b.complexity === "medium" ? 7 : 20), 0);
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

export function Exercises({
  overview,
  stats,
  courses,
  onOpenLesson,
  onOpenCourse,
  onOpenTutor,
  onOpenProgress,
  onOpenSearch,
}: {
  overview: LessonOverview;
  stats: Stats;
  courses: CourseProgress[];
  onOpenLesson: (lesson: Lesson) => void;
  onOpenCourse: (progress: CourseProgress) => void;
  onOpenTutor: () => void;
  onOpenProgress: () => void;
  onOpenSearch: () => void;
}) {
  // A done day (the goal reached, or everything due cleared) fills the ring.
  const goalPct = stats.completedToday
    ? 100
    : Math.min(100, Math.round((stats.dayXp / Math.max(1, stats.dailyGoal)) * 100));
  // Courses closed as mastered leave the list for their own shelf; the rest
  // stay, completed and mastered ones after those still being worked on.
  const open = courses.filter((t) => t.closedAt === null);
  const active = [...open.filter((t) => t.status === "active"), ...open.filter((t) => t.status !== "active")];
  const mastered = courses.filter((t) => t.closedAt !== null).sort((a, b) => (b.closedAt ?? 0) - (a.closedAt ?? 0));
  const [showMastered, setShowMastered] = useState(false);
  // Today's list first; once it's done, more for a big day; else something new.
  const keepGoing = overview.due.length === 0 && overview.more.length > 0;
  const upNext = overview.due[0] ?? overview.more[0] ?? overview.next[0] ?? null;
  const balance =
    overview.behindMinutes > 0
      ? `About ${overview.behindMinutes} min more is due — it waits for another day, or keep going`
      : overview.aheadMinutes > 0
        ? `${overview.aheadMinutes} min ahead this week`
        : null;

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
        {courses.length > 0 || stats.totalXp > 0 ? <div className="practice-today">
          {courses.length > 0 ? <Hero>
            <Stack gap={4}>
              <Inline gap={4}>
                <div
                  className="goal-ring"
                  style={{ "--value": goalPct } as CSSProperties}
                  role="img"
                  aria-label={`${stats.dayXp} of ${stats.dailyGoal} XP today`}
                >
                  <div>
                    <Text variant="title" weight="bold">
                      {stats.dayXp}
                    </Text>
                    <Text variant="caption" tone="dim">
                      / {stats.dailyGoal} XP
                    </Text>
                  </div>
                </div>
                <div className="grow">
                  <Inline gap={2} wrap>
                    <Text variant="eyebrow">{stats.completedToday ? "Today is done ✓" : "Today"}</Text>
                    {stats.currentStreak > 0 ? <Badge>🔥 {stats.currentStreak}</Badge> : null}
                    {stats.streakFreezes > 0 ? (
                      <Badge>
                        🧊 {stats.streakFreezes}
                      </Badge>
                    ) : null}
                  </Inline>
                  <Text variant="title" as="h2">
                    {overview.due.length > 0
                      ? `${overview.due.length} check-in${overview.due.length === 1 ? "" : "s"} today`
                      : overview.planTotal > 0
                        ? "Today's list is done"
                        : "All caught up"}
                  </Text>
                  <Text variant="small" tone="dim" as="div">
                    {overview.due.length > 0
                      ? `About ${estimate(overview.due)} min${
                          overview.planDone > 0 ? ` · ${overview.planDone} of ${overview.planTotal} done` : ""
                        }`
                      : overview.doneToday > 0
                        ? `${overview.doneToday} lesson${overview.doneToday === 1 ? "" : "s"} done today`
                        : "Start something new below"}
                  </Text>
                  {balance ? (
                    <Text variant="caption" tone="dim" as="div">
                      {balance}
                    </Text>
                  ) : null}
                </div>
              </Inline>
              {upNext ? (
                <Stack gap={2}>
                  <Button variant="inverse" size="lg" wide onClick={() => onOpenLesson(upNext)}>
                    {overview.due.length > 0
                      ? "Start the first check-in"
                      : keepGoing
                        ? "Keep going"
                        : "Start something new"}
                  </Button>
                  <Text variant="caption" tone="dim" align="center" as="div">
                    {upNext.title}
                    {upNext.courseTitle ? ` · ${upNext.courseTitle}` : ""} · {minutes(upNext.complexity)}
                  </Text>
                </Stack>
              ) : null}
            </Stack>
          </Hero> : null}

          {overview.due.length > 0 ? (
            <Section
              title="Today's check-ins"
              info={
                <>
                  <p>
                    The lessons most at risk of being forgotten, as many as fit today. Clear these and
                    the day counts for your streak.
                  </p>
                  <p>
                    How much goes on the list follows what you usually do, never less than the quiet-day
                    minimum in Settings. Anything else that is due waits for another day without counting
                    as missed.
                  </p>
                </>
              }
            >
              <List>
                {overview.due.map((lesson) => (
                  <LessonRow key={lesson.id} lesson={lesson} today={overview.today} showCourse onOpen={onOpenLesson} />
                ))}
              </List>
            </Section>
          ) : null}

          {keepGoing ? (
            <Section
              title="Keep going"
              info={
                <p>
                  Today's list is done. These are next: the rest of what is due, then what comes due
                  soonest. Doing one early is a real check-in — it moves out from the time that has
                  passed, and comes off the day it was due.
                </p>
              }
            >
              <List>
                {overview.more.map((lesson) => (
                  <LessonRow key={lesson.id} lesson={lesson} today={overview.today} showCourse onOpen={onOpenLesson} />
                ))}
              </List>
            </Section>
          ) : null}

          {courses.length > 0 || stats.totalXp > 0 ? <List>
            <ListItem
              leading={<Avatar>📈</Avatar>}
              title="Your progress"
              subtitle={`${stats.wordsSeen} words · level ${stats.level}`}
              trailing={
                <Inline gap={1}>
                  <Badge tone="crimson">🔥 {stats.currentStreak}</Badge>
                </Inline>
              }
              chevron
              onClick={onOpenProgress}
            />
          </List> : null}

        </div> : null}

        <div className="practice-courses">
          <Section
            title="Courses"
            info={
              <p>
                A course is a folder of lessons on one theme: drills on its words, sentences and
                dialogue, and lessons with a check-in. Ask the tutor for a new course on anything, or to
                add lessons and drills to one you have.
              </p>
            }
          >
            {courses.length === 0 ? (
              <List>
                <EmptyState
                  icon="✨"
                  title="Build your first Polish course"
                  action={
                    <Button variant="primary" size="sm" onClick={onOpenTutor}>
                      Create a course
                    </Button>
                  }
                >
                  Tell the tutor what you want to learn — everyday conversation, life in Poland, or a course you enjoy.
                </EmptyState>
              </List>
            ) : (
              <Stack gap={3}>
                <CourseList entries={active} onOpen={onOpenCourse} />
                <List>
                  <ListItem
                    leading={<Avatar>✨</Avatar>}
                    title="New course or lesson"
                    subtitle="Ask the tutor for anything — a theme, a conjugation, a grammar point"
                    onClick={onOpenTutor}
                  />
                </List>
              </Stack>
            )}
          </Section>

          {mastered.length > 0 ? (
            <Section
              title="Mastered"
              info={
                <p>
                  Courses you mastered and closed: kept here as trophies, readable any time. Their
                  lessons no longer come back in your check-ins. To revisit a theme, ask the tutor for a
                  new course.
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
