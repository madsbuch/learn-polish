import { useEffect, useMemo, useState } from "react";

import { ChallengeMark, LessonRow } from "../components/LessonRow";
import type { CourseSection } from "../lib/navigation";
import { CourseClosedCelebration } from "../components/CourseClosedCelebration";
import { CourseOffline } from "../components/CourseOffline";
import { LevelInfo } from "../components/LevelInfo";
import { SparkIcon } from "../components/icons";
import { SlowButton, SpeakButton } from "../components/ui";
import {
  Badge,
  Banner,
  Button,
  ConfirmButton,
  Field,
  List,
  ListEmpty,
  ListItem,
  Screen,
  Progress,
  Section,
  Segmented,
  Sheet,
  Spinner,
  Stack,
  Text,
  Inline,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { fileUrl } from "../lib/files";
import { CHALLENGE_EXPLAINED, dayToDate, mastered, MASTERY_FULL, roughDuration, standing, standingText } from "../lib/lessons";
import type { CourseClosed, CoursePictureStatus, CourseProgress, FileId, Lesson, LessonId, Sentence, Word } from "../lib/types";
import { onEvent } from "../lib/events";
import { useSyncReload } from "../lib/sync";
import { z } from "zod";
import { CourseIdSchema, CoursePictureStatusSchema, FileIdSchema } from "../lib/types";

export type { CourseSection } from "../lib/navigation";

/**
 * A course: the lessons it lists, in its order. Words and sentences belong to
 * those lessons; the other tabs list them all, each leading to its lesson.
 */
export function CourseDetail({
  progress,
  section,
  onSection,
  today,
  onBack,
  onOpenLesson,
  onDeleted,
  onChanged,
}: {
  progress: CourseProgress;
  /** The section shown: the address's to say, so coming back keeps it. */
  section: CourseSection;
  onSection: (section: CourseSection) => void;
  today: number;
  onBack: () => void;
  onOpenLesson: (lessonId: LessonId) => void;
  onDeleted: () => void;
  /** The course was renamed; reload the lists that show it. */
  onChanged: () => void;
}) {
  const [lessons, setLessons] = useState<Lesson[] | null>(null);
  const [words, setWords] = useState<Word[] | null>(null);
  const [sentences, setSentences] = useState<Sentence[] | null>(null);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [names, setNames] = useState({
    title: progress.course.title,
    titlePl: progress.course.titlePl,
    summary: progress.course.summary,
  });
  const [renaming, setRenaming] = useState<typeof names | null>(null);
  // The picture's file id.
  const [picture, setPicture] = useState<FileId | null>(progress.imageFileId);
  const [drawing, setDrawing] = useState(false);
  // Drawn in the background (a new course from the tutor), or why the last
  // drawing failed.
  const [pictureStatus, setPictureStatus] = useState<CoursePictureStatus | null>(null);
  const [pictureError, setPictureError] = useState<string | null>(null);
  const [celebrating, setCelebrating] = useState<CourseClosed | null>(null);

  async function drawPicture() {
    setDrawing(true);
    setPictureError(null);
    try {
      setPicture(await api.coursePicture(courseId));
      onChanged();
    } catch (cause) {
      setPictureError(errorMessage(cause));
    } finally {
      setDrawing(false);
    }
  }

  const courseId = progress.course.id;
  const [closedAt, setClosedAt] = useState<number | null>(progress.closedAt);
  /** The level's explanation, open. */
  const [levelInfo, setLevelInfo] = useState(false);

  // Being drawn in the background, or failed there: say so, with a retry.
  useEffect(() => {
    let live = true;
    api
      .coursePictureStatus(courseId)
      .then((status) => {
        if (live) setPictureStatus(status);
      })
      .catch(() => undefined);
    const off = onEvent(CoursePictureStatusSchema, "pictures://status", (event) => {
      if (event.payload.courseId === courseId) setPictureStatus(event.payload);
    });
    return () => {
      live = false;
      void off.then((stop) => stop());
    };
  }, [courseId]);
  const drawingPicture = drawing || (pictureStatus?.drawing ?? false);
  const pictureFailed = drawingPicture ? null : (pictureError ?? pictureStatus?.error ?? null);

  // Drawn in the background for a new course: show it as soon as it is in.
  useEffect(() => {
    const off = onEvent(z.object({ courseId: CourseIdSchema, fileId: FileIdSchema }), "pictures://changed", (event) => {
      if (event.payload.courseId === courseId) setPicture(event.payload.fileId);
    });
    return () => {
      void off.then((stop) => stop());
    };
  }, [courseId]);

  async function act(action: () => Promise<void>) {
    setError(null);
    try {
      await action();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }

  // Bumped when another device's data arrives (sync): the lists load again.
  const [loads, setLoads] = useState(0);
  useSyncReload(() => setLoads((n) => n + 1));

  useEffect(() => {
    setError(null);
    Promise.all([api.lessonsByImportance(courseId), api.browseWords(courseId, "", 500), api.browseSentences(courseId)])
      .then(([lessonList, wordList, sentenceList]) => {
        setLessons(lessonList);
        setWords(wordList);
        setSentences(sentenceList);
      })
      .catch((cause) => setError(errorMessage(cause)));
  }, [courseId, loads]);

  const filtered = useMemo(() => {
    if (!words) return [];
    const needle = query.trim().toLowerCase();
    if (!needle) return words;
    return words.filter(
      (word) => word.pl.toLowerCase().includes(needle) || word.meaning.toLowerCase().includes(needle),
    );
  }, [words, query]);

  // A course of lessons has no words or sentences: then there are no tabs.
  const tabs = [
    { value: "lessons" as const, label: lessons ? `Lessons ${lessons.length}` : "Lessons" },
    ...(words && words.length > 0 ? [{ value: "words" as const, label: `Words ${words.length}` }] : []),
    ...(sentences && sentences.length > 0
      ? [{ value: "sentences" as const, label: `Sentences ${sentences.length}` }]
      : []),
  ];
  // Spec: SHELL-18
  const shown = tabs.some((t) => t.value === section) ? section : "lessons";

  return (
    <Screen
      title={names.titlePl}
      subtitle={names.title}
      onBack={onBack}
      nav={false}
      backdrop={picture !== null ? fileUrl(picture) : null}
    >
      {error ? <Banner tone="error">{error}</Banner> : null}
      {/* On a wide window: what the course is beside its lessons. On a
          phone the wrappers vanish (display: contents) and all is one column. */}
      <div className="page-split">
        <div className="page-split__info">
          {pictureFailed !== null ? (
            <Banner tone="error">
              <Stack gap={1}>
                <Text as="p">The picture couldn’t be drawn — try again.</Text>
                <Text variant="small" tone="dim" as="p">
                  {pictureFailed}
                </Text>
              </Stack>
            </Banner>
          ) : null}
          {picture !== null || progress.openEnded ? null : (
            <Button
              wide
              variant="primary"
              icon={<SparkIcon />}
              loading={drawingPicture}
              disabled={drawingPicture}
              onClick={() => void drawPicture()}
            >
              {drawingPicture ? "Drawing a picture…" : pictureFailed !== null ? "Try again" : "Draw a picture"}
            </Button>
          )}
          <Inline gap={2} wrap>
            {progress.course.level ? (
              <Badge tone="brand" label={`${progress.course.level}: what it means`} onClick={() => setLevelInfo(true)}>
                {`${progress.course.level} ⓘ`}
              </Badge>
            ) : null}
            {closedAt === null && progress.minutesToMaster > 0 ? (
              <Badge>{`${roughDuration(progress.minutesToMaster)} to master`}</Badge>
            ) : null}
          </Inline>
          {names.summary ? (
            <Text variant="small" tone="dim" as="p">
              {names.summary}
            </Text>
          ) : null}
          {levelInfo && progress.course.level ? (
            <LevelInfo level={progress.course.level} what="course" onClose={() => setLevelInfo(false)} />
          ) : null}

          {lessons && lessons.length > 0 ? (
            <CourseStanding
              lessons={lessons}
              openEnded={progress.openEnded}
              closedAt={closedAt}
              onClose={() =>
                act(async () => {
                  setCelebrating(await api.courseClose(courseId));
                  setClosedAt(Math.floor(Date.now() / 1000));
                  onChanged();
                })
              }
              onShowLeft={() => {
                onSection("lessons");
                window.requestAnimationFrame(() =>
                  document.getElementById("to-pass")?.scrollIntoView({ behavior: "smooth", block: "start" }),
                );
              }}
            />
          ) : null}
        </div>

        <div className="page-split__main">
          {tabs.length > 1 ? <Segmented value={shown} onChange={onSection} options={tabs} /> : null}

          {shown === "lessons" ? (
            lessons ? (
              lessons.length === 0 ? (
                <List>
                  <ListEmpty>No lessons yet. Ask the tutor to add some.</ListEmpty>
                </List>
              ) : (
                <CourseLessons lessons={lessons} today={today} closed={closedAt !== null} onOpen={onOpenLesson} />
              )
            ) : (
              <Spinner />
            )
          ) : null}

          {shown === "words" && words ? (
            <Stack gap={3}>
              <input
                type="search"
                value={query}
                placeholder="Search these words"
                onChange={(event) => setQuery(event.target.value)}
              />
              <List>
                {filtered.map((word) => (
                  <ListItem
                    key={word.id}
                    selectable
                    title={word.pl}
                    subtitle={`${word.meaning} · ${word.lesson}`}
                    label={`${word.pl}: open ${word.lesson}`}
                    onClick={() => onOpenLesson(word.lessonId)}
                    trailing={<SpeakButton text={word.pl} />}
                  />
                ))}
                {filtered.length === 0 ? <ListEmpty>No match.</ListEmpty> : null}
              </List>
            </Stack>
          ) : null}

          {shown === "sentences" && sentences ? (
            <List>
              {sentences.map((sentence) => (
                <ListItem
                  key={sentence.id}
                  selectable
                  title={sentence.pl}
                  subtitle={`${sentence.meaning} · ${sentence.lesson}`}
                  label={`${sentence.pl}: open ${sentence.lesson}`}
                  onClick={() => onOpenLesson(sentence.lessonId)}
                  trailing={
                    <Inline gap={1}>
                      <SpeakButton text={sentence.pl} />
                      <SlowButton text={sentence.pl} />
                    </Inline>
                  }
                />
              ))}
            </List>
          ) : null}
        </div>

        <div className="page-split__side">
          <CourseOffline courseId={courseId} />
          <Section title="Manage">
            <Stack gap={2}>
              <Button wide onClick={() => setRenaming(names)}>
                Rename
              </Button>
              {picture !== null ? (
                <Button
                  wide
                  icon={<SparkIcon />}
                  loading={drawingPicture}
                  disabled={drawingPicture}
                  onClick={() => void drawPicture()}
                >
                  {drawingPicture ? "Drawing…" : "Draw a new picture"}
                </Button>
              ) : null}
              <ConfirmButton
                label="Delete course"
                confirmLabel="Delete"
                question="Delete this course? Lessons only in this course go with it, with their words and your progress on them."
                wide
                onConfirm={() =>
                  act(async () => {
                    await api.courseDelete(courseId);
                    onDeleted();
                  })
                }
              />
            </Stack>
          </Section>
        </div>
      </div>

      {renaming ? (
        <Sheet title="Rename course" onClose={() => setRenaming(null)}>
          <Stack gap={4}>
            <Field label="Title in Polish">
              <input
                value={renaming.titlePl}
                onChange={(event) => setRenaming({ ...renaming, titlePl: event.target.value })}
              />
            </Field>
            <Field label="Title in English">
              <input
                value={renaming.title}
                onChange={(event) => setRenaming({ ...renaming, title: event.target.value })}
              />
            </Field>
            <Field label="What it is about">
              <textarea
                rows={2}
                value={renaming.summary}
                onChange={(event) => setRenaming({ ...renaming, summary: event.target.value })}
              />
            </Field>
            <Button
              variant="primary"
              size="lg"
              wide
              disabled={!renaming.title.trim() || !renaming.titlePl.trim()}
              onClick={() =>
                act(async () => {
                  await api.courseRename(courseId, renaming.title, renaming.titlePl, renaming.summary);
                  setNames({
                    title: renaming.title.trim(),
                    titlePl: renaming.titlePl.trim(),
                    summary: renaming.summary.trim(),
                  });
                  setRenaming(null);
                  onChanged();
                })
              }
            >
              Save
            </Button>
          </Stack>
        </Sheet>
      ) : null}
      {celebrating ? <CourseClosedCelebration closed={celebrating} onDone={() => setCelebrating(null)} /> : null}
    </Screen>
  );
}

/**
 * How far the course is: passing each core lesson once completes it; after
 * that, a bar of the mastery points earned (see lessons.rs).
 */
function CourseStanding({
  lessons,
  openEnded,
  closedAt,
  onShowLeft,
  onClose,
}: {
  lessons: Lesson[];
  openEnded: boolean;
  closedAt: number | null;
  onShowLeft: () => void;
  onClose: () => void;
}) {
  const core = lessons.filter((lesson) => !lesson.challenge);
  const passed = core.filter((lesson) => lesson.passed).length;
  const left = core.length - passed;
  const masteredCount = lessons.filter(mastered).length;
  if (closedAt !== null) {
    return (
      <Banner tone="ok">
        <Stack gap={2}>
          <Text variant="small" weight="bold" as="div">
            {`🏆 Mastered — closed ${dayToDate(Math.floor(closedAt / 86_400))}`}
          </Text>
          <Text variant="small" as="div">
            Closed for good: its lessons no longer come back. To revisit this theme, ask the tutor for a new course — it
            builds on this one.
          </Text>
        </Stack>
      </Banner>
    );
  }
  if (openEnded) {
    return (
      <Text variant="small" tone="dim" as="div">
        {`${passed} of ${core.length} passed · this course keeps growing as the tutor adds to it, so it is never finished.`}
      </Text>
    );
  }
  if (left > 0 || core.length === 0) {
    return (
      <Stack gap={2}>
        <Inline justify="between" gap={2}>
          <Text variant="small" tone="dim">
            {`${passed} of ${core.length} passed · ${left} to go`}
          </Text>
          {left > 0 ? (
            <Button size="sm" onClick={onShowLeft}>
              {`See the ${left} to pass`}
            </Button>
          ) : null}
        </Inline>
        <Progress value={passed} max={Math.max(1, core.length)} size="sm" />
      </Stack>
    );
  }
  if (masteredCount === lessons.length) {
    return (
      <Stack gap={2}>
        <Text variant="small" weight="bold" as="div">
          🏆 Ready to close — you mastered every lesson in it.
        </Text>
        <Text variant="small" tone="dim" as="div">
          Close it to celebrate and keep it as a trophy in Mastered. Closing is final — to come back to this theme, ask
          the tutor for a new course.
        </Text>
        <Button variant="primary" wide onClick={onClose}>
          Close as mastered 🏆
        </Button>
      </Stack>
    );
  }
  const points = lessons.reduce((sum, lesson) => sum + Math.min(Math.max(lesson.mastery, 0), MASTERY_FULL), 0);
  const max = lessons.length * MASTERY_FULL;
  const pct = Math.floor((points / Math.max(1, max)) * 100);
  return (
    <Stack gap={2}>
      <Text variant="small" tone="dim" as="div">
        {`✓ Completed · ${pct}% mastered`}
      </Text>
      <Progress value={points} max={Math.max(1, max)} size="sm" tone="gold" />
    </Stack>
  );
}

/**
 * A course's lessons, most important first — the order today's lessons are
 * chosen in (plan.rs): what slipped and is due, what is due, the next new
 * ones, then the rest. Mastered lessons never come back, so they wait folded
 * away at the bottom.
 */
function CourseLessons({
  lessons,
  today,
  closed,
  onOpen,
}: {
  /** Already in order of importance (lessons_by_importance). */
  lessons: Lesson[];
  today: number;
  /** Closed as mastered: its lessons no longer come back, so no dates. */
  closed: boolean;
  onOpen: (lessonId: LessonId) => void;
}) {
  const [showMastered, setShowMastered] = useState(false);
  const open = lessons.filter((lesson) => standing(lesson) !== "mastered");
  const masteredNow = lessons.filter((lesson) => standing(lesson) === "mastered");
  // Spec: CRS-20
  const explained = lessons.some((lesson) => lesson.challenge) ? (
    <p>
      <ChallengeMark /> {CHALLENGE_EXPLAINED}
    </p>
  ) : null;
  const row = (lesson: Lesson) => {
    const where = standing(lesson);
    return (
      <LessonRow
        key={lesson.id}
        lesson={lesson}
        today={today}
        status={
          closed
            ? lesson.passed
              ? "Mastered"
              : "Not passed"
            : standingText(lesson, today)
        }
        trailing={
          where === "mastered" ? (
            <Badge tone="gold">✓ Mastered</Badge>
          ) : where === "learning" ? (
            <Badge tone="mint">{`${Math.min(lesson.mastery, MASTERY_FULL)}%`}</Badge>
          ) : undefined
        }
        onOpen={(l) => onOpen(l.id)}
      />
    );
  };
  const info = (
    <>
      <p>
        Most important first: a lesson that slipped last time and is due again, then the ones whose time has come,
        then the next new ones, then the rest. Every pass adds mastery points, up to 100 (a go short of a pass, a share
        by its score; nothing takes them away); a mastered lesson never comes back.
      </p>
      {explained}
    </>
  );
  return (
    <Stack gap={4}>
      {open.length > 0 ? (
        <Section
          id="to-pass"
          title={`Lessons · ${open.length}`}
          info={info}
        >
          <List stagger>{open.map(row)}</List>
        </Section>
      ) : null}
      {masteredNow.length > 0 ? (
        <Section
          title={`Mastered · ${masteredNow.length}`}
          info={explained ?? undefined}
          action={
            <Button size="sm" onClick={() => setShowMastered((shown) => !shown)}>
              {showMastered ? "Hide" : "Show"}
            </Button>
          }
        >
          {showMastered ? <List>{masteredNow.map(row)}</List> : null}
        </Section>
      ) : null}
    </Stack>
  );
}
