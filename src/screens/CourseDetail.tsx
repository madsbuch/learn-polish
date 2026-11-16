import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { LessonRow } from "../components/LessonRow";
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
import { dayToDate, holds, standing, standingText, writingStep } from "../lib/lessons";
import { toast } from "../lib/toast";
import type { Lesson, Sentence, CourseProgress, Word } from "../lib/types";

export type CourseSection = "lessons" | "words" | "sentences";

/** How long after closing Undo is offered (courses.rs UNDO_CLOSE_SECS). */
const UNDO_CLOSE_SECS = 10 * 60;

/**
 * A course: the lessons it lists, in its order. Words and sentences belong to
 * those lessons; the other tabs list them all, each leading to its lesson.
 */
export function CourseDetail({
  progress,
  initialSection,
  today,
  onBack,
  onOpenLesson,
  onDeleted,
  onChanged,
}: {
  progress: CourseProgress;
  /** Which tab to land on. */
  initialSection?: CourseSection;
  today: number;
  onBack: () => void;
  onOpenLesson: (lessonId: number) => void;
  onDeleted: () => void;
  /** The course was renamed; reload the lists that show it. */
  onChanged: () => void;
}) {
  const [lessons, setLessons] = useState<Lesson[] | null>(null);
  const [tab, setTab] = useState<CourseSection>(initialSection ?? "lessons");
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
  const [picture, setPicture] = useState<number | null>(progress.imageFileId);
  const [drawing, setDrawing] = useState(false);

  async function drawPicture() {
    setDrawing(true);
    setError(null);
    try {
      setPicture(await api.coursePicture(courseId));
      onChanged();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setDrawing(false);
    }
  }

  const courseId = progress.course.id;
  const [closedAt, setClosedAt] = useState<number | null>(progress.closedAt);

  // Drawn in the background for a new course: show it as soon as it is in.
  useEffect(() => {
    const off = listen<{ courseId: number; fileId: number }>("pictures://changed", (event) => {
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

  useEffect(() => {
    setError(null);
    Promise.all([api.lessonsForCourse(courseId), api.browseWords(courseId, "", 500), api.browseSentences(courseId)])
      .then(([lessonList, wordList, sentenceList]) => {
        setLessons(lessonList);
        setWords(wordList);
        setSentences(sentenceList);
      })
      .catch((cause) => setError(errorMessage(cause)));
  }, [courseId]);

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
  const shown = tabs.some((t) => t.value === tab) ? tab : "lessons";

  return (
    <Screen
      title={names.titlePl}
      subtitle={names.title}
      onBack={onBack}
      nav={false}
      backdrop={picture !== null ? fileUrl(picture) : null}
    >
      {error ? <Banner tone="error">{error}</Banner> : null}
      {picture !== null ? null : (
        <Button wide icon={<SparkIcon />} loading={drawing} onClick={() => void drawPicture()}>
          {drawing ? "Drawing a picture…" : "Make a picture for this course"}
        </Button>
      )}
      {names.summary ? (
        <Text variant="small" tone="dim" as="p">
          {names.summary}
        </Text>
      ) : null}

      {lessons && lessons.length > 0 ? (
        <CourseStanding
          lessons={lessons}
          openEnded={progress.openEnded}
          closedAt={closedAt}
          onClose={() =>
            act(async () => {
              await api.courseClose(courseId);
              setClosedAt(Math.floor(Date.now() / 1000));
              toast("Closed as mastered — it is in Mastered now. 🏆");
              onChanged();
            })
          }
          onUndo={() =>
            act(async () => {
              await api.courseUndoClose(courseId);
              setClosedAt(null);
              onChanged();
            })
          }
          onShowLeft={() => {
            setTab("lessons");
            window.requestAnimationFrame(() =>
              document.getElementById("to-pass")?.scrollIntoView({ behavior: "smooth", block: "start" }),
            );
          }}
        />
      ) : null}

      {tabs.length > 1 ? <Segmented value={shown} onChange={setTab} options={tabs} /> : null}

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

      <Section title="Manage">
        <Stack gap={2}>
          <Button wide onClick={() => setRenaming(names)}>
            Rename
          </Button>
          {picture !== null ? (
            <Button wide icon={<SparkIcon />} loading={drawing} onClick={() => void drawPicture()}>
              {drawing ? "Drawing…" : "Draw a new picture"}
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
    </Screen>
  );
}

/**
 * How far the course is. Passing each core lesson once completes it, and the
 * header counts that; after that it counts towards mastering — everything,
 * challenges too, holding at long gaps — and says what stands in the way.
 */
function CourseStanding({
  lessons,
  openEnded,
  closedAt,
  onShowLeft,
  onClose,
  onUndo,
}: {
  lessons: Lesson[];
  openEnded: boolean;
  closedAt: number | null;
  onShowLeft: () => void;
  onClose: () => void;
  onUndo: () => void;
}) {
  const core = lessons.filter((lesson) => !lesson.challenge);
  const passed = core.filter((lesson) => lesson.passed).length;
  const left = core.length - passed;
  const holding = lessons.filter(holds).length;
  if (closedAt !== null) {
    // Undo is for a mis-tap: offered for the first minutes only (as the core allows).
    const fresh = Date.now() / 1000 - closedAt < UNDO_CLOSE_SECS;
    return (
      <Banner tone="ok">
        <Stack gap={2}>
          <Text variant="small" weight="bold" as="div">
            {`🏆 Mastered — closed ${dayToDate(Math.floor(closedAt / 86_400))}`}
          </Text>
          <Text variant="small" as="div">
            Its lessons no longer come back in your check-ins. To revisit this theme, ask the tutor
            for a new course — it builds on this one.
          </Text>
          {fresh ? (
            <Inline>
              <Button size="sm" onClick={onUndo}>
                Undo
              </Button>
            </Inline>
          ) : null}
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
  if (holding === lessons.length) {
    return (
      <Stack gap={2}>
        <Text variant="small" weight="bold" as="div">
          🏆 Mastered — everything holds.
        </Text>
        <Text variant="small" tone="dim" as="div">
          Close it to keep it as a trophy in Mastered: its lessons leave your check-ins. Closing is
          final — to come back to this theme, ask the tutor for a new course.
        </Text>
        <Button variant="primary" wide onClick={onClose}>
          Close as mastered 🏆
        </Button>
      </Stack>
    );
  }
  // What stands in the way of mastering: a writing lesson still done with
  // help, a challenge not passed; the rest only needs time.
  const blockers = [
    ...lessons.filter((lesson) => lesson.passed && writingStep(lesson)).map((l) => `“${l.title}”: ${writingStep(l)} once.`),
    ...lessons.filter((lesson) => lesson.challenge && !lesson.passed).map((l) => `Challenge “${l.title}”: pass it once.`),
  ];
  return (
    <Stack gap={2}>
      <Text variant="small" tone="dim" as="div">
        {`✓ Completed · mastery: ${holding} of ${lessons.length} holding`}
      </Text>
      <Progress value={holding} max={lessons.length} size="sm" tone="gold" />
      {blockers.slice(0, 3).map((line) => (
        <Text key={line} variant="small" as="div">
          {line}
        </Text>
      ))}
      {blockers.length === 0 ? (
        <Text variant="small" tone="dim" as="div">
          The rest needs only time: each check-in you pass spaces a lesson out further, until it comes back
          a month apart.
        </Text>
      ) : null}
    </Stack>
  );
}

/**
 * A course's lessons by what is left: to pass (what completing needs), passed
 * and being kept up, holding for good — then the challenges.
 */
function CourseLessons({
  lessons,
  today,
  closed,
  onOpen,
}: {
  lessons: Lesson[];
  today: number;
  /** Closed as mastered: its lessons no longer come back, so no dates. */
  closed: boolean;
  onOpen: (lessonId: number) => void;
}) {
  const core = lessons.filter((lesson) => !lesson.challenge);
  const challenges = lessons.filter((lesson) => lesson.challenge);
  const toPass = core.filter((lesson) => standing(lesson) === "toPass");
  const passed = core.filter((lesson) => standing(lesson) === "passed");
  const holdingNow = core.filter((lesson) => standing(lesson) === "holding");
  const row = (lesson: Lesson) => {
    const where = standing(lesson);
    return (
      <LessonRow
        key={lesson.id}
        lesson={lesson}
        today={today}
        status={closed ? (lesson.passed ? "Mastered" : "Not passed") : standingText(lesson, today)}
        trailing={
          where === "holding" ? (
            <Badge tone="gold">✓ Holds</Badge>
          ) : where === "passed" ? (
            <Badge tone="mint">✓ Passed</Badge>
          ) : undefined
        }
        onOpen={(l) => onOpen(l.id)}
      />
    );
  };
  return (
    <Stack gap={4}>
      {toPass.length > 0 ? (
        <Section id="to-pass" title={`To pass · ${toPass.length}`}>
          <List>{toPass.map(row)}</List>
        </Section>
      ) : null}
      {passed.length > 0 ? (
        <Section
          title={`Passed — keep it up · ${passed.length}`}
          info={
            <p>
              Passed once, and coming back in your check-ins further apart each time. A lesson holds
              for good once it comes back a month apart (writing: done without the word bank and
              hints).
            </p>
          }
        >
          <List>{passed.map(row)}</List>
        </Section>
      ) : null}
      {holdingNow.length > 0 ? (
        <Section title={`Holding · ${holdingNow.length}`}>
          <List>{holdingNow.map(row)}</List>
        </Section>
      ) : null}
      {challenges.length > 0 ? (
        <Section
          title={`Challenges · ${challenges.length}`}
          info={
            <p>
              Beyond this course's level: not needed to complete it, but needed to master it. Come
              back when the rest feels easy.
            </p>
          }
        >
          <List>{challenges.map(row)}</List>
        </Section>
      ) : null}
    </Stack>
  );
}
