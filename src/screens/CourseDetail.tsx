import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { LessonRow } from "../components/LessonRow";
import { SparkIcon } from "../components/icons";
import { SlowButton, SpeakButton } from "../components/ui";
import {
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
import type { Lesson, Sentence, CourseProgress, Word } from "../lib/types";

export type CourseSection = "lessons" | "words" | "sentences";

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

      {lessons && lessons.length > 0 ? <CourseStanding lessons={lessons} /> : null}

      {tabs.length > 1 ? <Segmented value={shown} onChange={setTab} options={tabs} /> : null}

      {shown === "lessons" ? (
        lessons ? (
          <Stack gap={4}>
            <List>
              {lessons
                .filter((lesson) => !lesson.challenge)
                .map((lesson) => (
                  <LessonRow key={lesson.id} lesson={lesson} today={today} onOpen={(b) => onOpenLesson(b.id)} />
                ))}
              {lessons.length === 0 ? <ListEmpty>No lessons yet. Ask the tutor to add some.</ListEmpty> : null}
            </List>
            {lessons.some((lesson) => lesson.challenge) ? (
              <Section
                title="Challenges"
                info={
                  <p>
                    Beyond this course's level: not needed to complete it, but needed to master it.
                    Come back when the rest feels easy.
                  </p>
                }
              >
                <List>
                  {lessons
                    .filter((lesson) => lesson.challenge)
                    .map((lesson) => (
                      <LessonRow key={lesson.id} lesson={lesson} today={today} onOpen={(b) => onOpenLesson(b.id)} />
                    ))}
                </List>
              </Section>
            ) : null}
          </Stack>
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
 * How far the course is: passing each core lesson once completes it; keeping
 * everything up (challenges too) masters it.
 */
function CourseStanding({ lessons }: { lessons: Lesson[] }) {
  const core = lessons.filter((lesson) => !lesson.challenge);
  const passed = core.filter((lesson) => lesson.passed).length;
  const completed = core.length > 0 && passed === core.length;
  const mastered =
    completed &&
    lessons.every(
      (lesson) => lesson.passed && lesson.intervalDays >= 30 && (lesson.format !== "writing" || lesson.supportLevel >= 2),
    );
  return (
    <Stack gap={2}>
      <Text variant="small" tone="dim" as="div">
        {mastered
          ? "🏆 Mastered — everything holds."
          : completed
            ? "✓ Completed. Keep its lessons up in your check-ins to master it."
            : `${passed} of ${core.length} lessons passed — pass each once to complete the course.`}
      </Text>
      {completed ? null : <Progress value={passed} max={Math.max(1, core.length)} size="sm" />}
    </Stack>
  );
}
