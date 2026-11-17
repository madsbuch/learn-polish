import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";

import { LessonIcon } from "../components/LessonRow";
import { CloseIcon, PlayerIcon, PlusIcon } from "../components/icons";
import { DialogueToolbar, DialogueView } from "../components/DialogueView";
import { Markdown } from "../components/Markdown";
import { ArticleReader, Sources } from "../components/Sources";
import { SongToListen } from "../components/SongToListen";
import { WordPictures } from "../components/WordPictures";
import { LevelInfo } from "../components/LevelInfo";
import { SlowButton, SpeakButton } from "../components/ui";
import {
  Badge,
  Banner,
  Button,
  Card,
  Choice,
  ChoiceRow,
  ConfirmButton,
  IconButton,
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  SettingRow,
  Switch,
  Sheet,
  Spinner,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { lessonType } from "../lib/lessonTypes";
import { PERIOD_CHOICES, complexityName, complexityTone, dayToDate, dueLabel, kindName, minutes } from "../lib/lessons";
import type { LessonDetail, CourseProgress, WikiSource } from "../lib/types";

const RESULT_TEXT = {
  pass: "Passed",
  partial: "Partly there",
  fail: "Not yet",
} as const;

/**
 * One lesson: what it is, the lesson or the items it practises, and its
 * check-in schedule. The one primary action starts it.
 */
export function LessonPage({
  lessonId,
  today,
  onBack,
  onStartDrill,
  onCheckIn,
  onDeleted,
}: {
  lessonId: number;
  today: number;
  onBack: () => void;
  onStartDrill: (lessonId: number) => void;
  onCheckIn: (lessonId: number) => void;
  onDeleted: () => void;
}) {
  const [lesson, setLesson] = useState<LessonDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revealAll, setRevealAll] = useState(false);
  /** The level's explanation, open. */
  const [levelInfo, setLevelInfo] = useState(false);
  /** The course picker: add the lesson to one, or move it there. */
  const [picking, setPicking] = useState<{
    mode: "add" | "move";
    courses: CourseProgress[];
  } | null>(null);
  /** A source being read, in the app's reader. */
  const [reading, setReading] = useState<WikiSource | null>(null);
  /** Adding audio players to the lesson's Polish. */
  const [addingPlayers, setAddingPlayers] = useState(false);
  /** Bumped when a source goes, so the list reloads. */
  const [sourcesVersion, setSourcesVersion] = useState(0);

  const reload = () =>
    api
      .lessonDetail(lessonId)
      .then(setLesson)
      .catch((cause) => setError(errorMessage(cause)));
  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lessonId]);

  // Opening a lesson downloads its audio at normal speed in the background
  // (conversation first, in each speaker's voice), so every 🔊 here plays at
  // once. Slow versions are made only when 🐢 asks for one.
  const prefetched = useRef<number | null>(null);
  useEffect(() => {
    if (!lesson || prefetched.current === lesson.id) return;
    prefetched.current = lesson.id;
    const lines = [
      ...lesson.lines.map((line) => ({ text: line.pl, gender: line.gender })),
      ...lesson.sentences.map(([pl]) => ({ text: pl, gender: "" })),
      ...lesson.words.map(([pl]) => ({ text: pl, gender: "" })),
    ];
    if (lines.length) void api.prefetchAudio(lines).catch(() => {});
  }, [lesson]);

  async function openPicker(mode: "add" | "move") {
    try {
      setPicking({ mode, courses: await api.getCourseProgress() });
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }

  async function changeCourses(action: () => Promise<unknown>) {
    setError(null);
    try {
      await action();
      setPicking(null);
      await reload();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }

  if (!lesson) {
    return (
      <Screen title="Lesson" onBack={onBack} nav={false}>
        {error ? <Banner tone="error">{error}</Banner> : <Spinner />}
      </Screen>
    );
  }

  if (reading) {
    return (
      <ArticleReader
        source={reading}
        onBack={() => setReading(null)}
        onRemoved={() => {
          setReading(null);
          setSourcesVersion((n) => n + 1);
        }}
      />
    );
  }

  // Everything type-specific — the button, which sections show — comes
  // from the lesson's type (lib/lessonTypes.ts).
  const type = lessonType(lesson);
  const started = lesson.dueDay !== null;
  const period = String(lesson.periodDays);
  const periodOptions = PERIOD_CHOICES.some((choice) => choice.value === period)
    ? PERIOD_CHOICES
    : [...PERIOD_CHOICES, { value: period, label: `${period} days` }].sort((a, b) => Number(a.value) - Number(b.value));

  /** One part of the page, by the name a type lists it under. */
  function section(name: string): ReactNode {
    if (!lesson) return null;
    switch (name) {
      case "song":
        return lesson.songId !== null ? (
          <Section title="Listen">
            <SongToListen songId={lesson.songId} />
          </Section>
        ) : null;
      case "writing":
        return (
          <Text variant="small" tone="dim" as="p">
            Write only in Polish — your partner doesn't understand anything else. The conversation ends when you've got
            every goal across.{" "}
            {lesson.supportLevel === 0
              ? "Guided: a word bank and hints help you, and your partner asks simple questions. Pass it and the help steps back."
              : lesson.supportLevel === 1
                ? "Supported: hints if you need them. Pass it and you write on your own."
                : "Free: you write on your own."}
          </Text>
        );
      case "objectives":
      case "goals":
        return (
          <Section title={name === "goals" ? "Goals" : "You will be able to"}>
            <List>
              {lesson.objectives.map((objective) => (
                <ListItem key={objective} title={objective} />
              ))}
            </List>
          </Section>
        );
      case "text":
      case "situation":
      case "note":
        return (
          <Section title={name === "situation" ? "The situation" : name === "note" ? "Note" : "Lesson"}>
            <Card>
              <div className="selectable">
                <Markdown text={lesson.content} />
              </div>
            </Card>
            {name === "text" && lesson.content && !lesson.content.includes("[[say:") ? (
              <Button
                wide
                loading={addingPlayers}
                onClick={async () => {
                  setAddingPlayers(true);
                  try {
                    setLesson(await api.lessonAddPlayers(lesson.id));
                  } catch (cause) {
                    setError(errorMessage(cause));
                  } finally {
                    setAddingPlayers(false);
                  }
                }}
              >
                🔊 Add players to the Polish
              </Button>
            ) : null}
          </Section>
        );
      case "sources":
        return (
          <Sources
            key={sourcesVersion}
            lessonId={lesson.id}
            factNote={lesson.factNote}
            factCheckedAt={lesson.factCheckedAt}
            onRead={setReading}
            onChecked={() => void reload()}
          />
        );
      case "dialogue":
        return lesson.lines.length > 0 ? (
          <Section title="Conversation">
            <Stack gap={3}>
              <DialogueToolbar
                lines={lesson.lines}
                revealAll={revealAll}
                onToggleReveal={() => setRevealAll((value) => !value)}
              />
              <DialogueView lines={lesson.lines} revealAll={revealAll} />
            </Stack>
          </Section>
        ) : null;
      case "pictures":
        return <WordPictures lesson={lesson} onChanged={reload} />;
      case "words":
        return lesson.words.length > 0 ? (
          <Section title={`${lesson.words.length} words`}>
            <List>
              {lesson.words.map(([pl, en]) => (
                <ListItem key={pl} selectable title={pl} subtitle={en} trailing={<SpeakButton text={pl} />} />
              ))}
            </List>
          </Section>
        ) : null;
      case "sentences":
        return lesson.sentences.length > 0 ? (
          <Section title={`${lesson.sentences.length} sentences`}>
            <List>
              {lesson.sentences.map(([pl, en]) => (
                <ListItem
                  key={pl}
                  selectable
                  title={pl}
                  subtitle={en}
                  trailing={
                    <Inline gap={1}>
                      <SpeakButton text={pl} />
                      <SlowButton text={pl} />
                    </Inline>
                  }
                />
              ))}
            </List>
          </Section>
        ) : null;
      default:
        return null;
    }
  }

  return (
    <Screen title={lesson.title} subtitle={lesson.courseTitle ?? undefined} onBack={onBack} nav={false}>
      {error ? <Banner tone="error">{error}</Banner> : null}

      <Stack gap={3}>
        <Inline gap={2} wrap>
          <Badge tone="brand">
            <Inline gap={1}>
              <LessonIcon lesson={lesson} />
              {kindName(lesson)}
            </Inline>
          </Badge>
          <Badge tone={complexityTone(lesson.complexity)}>
            {complexityName(lesson.complexity)} · {minutes(lesson.complexity)}
          </Badge>
          {started ? <Badge>{dueLabel(lesson, today)}</Badge> : null}
          {lesson.challenge ? <Badge tone="gold">Challenge</Badge> : null}
          {lesson.level ? (
            <Badge tone="brand" label={`${lesson.level}: what it means`} onClick={() => setLevelInfo(true)}>
              {`${lesson.level} ⓘ`}
            </Badge>
          ) : null}
          {levelInfo && lesson.level ? (
            <LevelInfo level={lesson.level} what="lesson" onClose={() => setLevelInfo(false)} />
          ) : null}
        </Inline>
        {lesson.summary ? (
          <Text tone="dim" as="p">
            {lesson.summary}
          </Text>
        ) : null}
        <Button
          variant="primary"
          size="lg"
          wide
          icon={<PlayerIcon.play size={18} />}
          onClick={() => (type.engine === "conversation" ? onCheckIn(lesson.id) : onStartDrill(lesson.id))}
        >
          {started ? type.againLabel : type.startLabel}
        </Button>
      </Stack>

      {type.sections.map((name) => (
        <Fragment key={name}>{section(name)}</Fragment>
      ))}

      <Section title="Check-ins">
        <List>
          {lesson.lastResult ? (
            <SettingRow
              label={`Last time: ${RESULT_TEXT[lesson.lastResult]}`}
              description={lesson.lastFeedback ?? undefined}
            />
          ) : null}
          <SettingRow
            label={
              !started
                ? "Starts the first time you do it"
                : lesson.dueDay! <= today
                  ? "Due now"
                  : `Next check-in ${dayToDate(lesson.dueDay!)}`
            }
            description={
              started
                ? "Each pass pushes the next one further out; struggling brings it back sooner."
                : `Then it comes back about every ${lesson.periodDays} days, further apart as you pass.`
            }
          />
          <SettingRow
            label="Check in"
            stacked
            control={
              <ChoiceRow>
                {periodOptions.map((option) => (
                  <Choice
                    key={option.value}
                    selected={option.value === period}
                    onClick={async () => {
                      try {
                        await api.lessonSetPeriod(lesson.id, Number(option.value));
                        setLesson({
                          ...lesson,
                          periodDays: Number(option.value),
                        });
                      } catch (cause) {
                        setError(errorMessage(cause));
                      }
                    }}
                  >
                    {option.label}
                  </Choice>
                ))}
              </ChoiceRow>
            }
          />
        </List>
      </Section>

      <Section
        title={lesson.courses.length === 1 ? "Course" : "Courses"}
        info={
          <p>
            A lesson can be in several courses: it stays one lesson, with one schedule, shown in each. Its words and
            sentences belong to the lesson and go wherever it goes. Taking it out of its last course deletes it — use
            Delete lesson for that.
          </p>
        }
      >
        <Stack gap={2}>
          <List>
            {lesson.courses.map(([id, title, titlePl]) => (
              <ListItem
                key={id}
                title={titlePl}
                subtitle={title}
                trailing={
                  lesson.courses.length > 1 ? (
                    <IconButton
                      icon={<CloseIcon />}
                      label={`Take it out of ${title}`}
                      onClick={() => changeCourses(() => api.lessonRemoveFromCourse(lesson.id, id))}
                    />
                  ) : undefined
                }
              />
            ))}
          </List>
          <Inline gap={2}>
            <div className="grow">
              <Button wide icon={<PlusIcon />} onClick={() => openPicker("add")}>
                Also in…
              </Button>
            </div>
            {lesson.courses.length === 1 ? (
              <div className="grow">
                <Button wide onClick={() => openPicker("move")}>
                  Move to…
                </Button>
              </div>
            ) : null}
          </Inline>
        </Stack>
      </Section>

      {picking ? (
        <Sheet title={picking.mode === "add" ? "Also show it in" : "Move it to"} onClose={() => setPicking(null)}>
          <List>
            {picking.courses
              .filter((entry) => !lesson.courses.some(([id]) => id === entry.course.id))
              .map((entry) => (
                <ListItem
                  key={entry.course.id}
                  title={entry.course.titlePl}
                  subtitle={entry.course.title}
                  onClick={() =>
                    changeCourses(async () => {
                      await api.lessonAddToCourse(lesson.id, entry.course.id);
                      if (picking.mode === "move") {
                        await api.lessonRemoveFromCourse(lesson.id, lesson.courses[0][0]);
                      }
                    })
                  }
                />
              ))}
          </List>
        </Sheet>
      ) : null}

      <Section title="Manage">
        <List>
          <SettingRow
            label="Challenge"
            description="Beyond the course's level: not needed to complete it, only to master it"
            control={
              <Switch
                label="Challenge"
                checked={lesson.challenge}
                onChange={async (challenge) => {
                  try {
                    await api.lessonSetChallenge(lesson.id, challenge);
                    await reload();
                  } catch (cause) {
                    setError(errorMessage(cause));
                  }
                }}
              />
            }
          />
        </List>
        <ConfirmButton
          label="Delete lesson"
          confirmLabel="Delete"
          question="Delete this lesson, its words and its check-in history, from every course it is in?"
          wide
          onConfirm={async () => {
            try {
              await api.lessonDelete(lesson.id);
              onDeleted();
            } catch (cause) {
              setError(errorMessage(cause));
            }
          }}
        />
      </Section>
    </Screen>
  );
}
