import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";

import { ChallengeMark, LessonIcon } from "../components/LessonRow";
import { PlayerIcon } from "../components/icons";
import { DialogueToolbar, DialogueView } from "../components/DialogueView";
import { Markdown } from "../components/Markdown";
import { Sources } from "../components/Sources";
import { SongToListen } from "../components/SongToListen";
import { WordPictures } from "../components/WordPictures";
import { SceneHeader } from "../components/SceneHeader";
import { StoryHeader } from "../components/StoryHeader";
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
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  SettingRow,
  Switch,
  Spinner,
  Stack,
  Text,
} from "../design";
import { logEvent, setPracticeContext } from "../lib/learnLog";
import { api, errorMessage } from "../lib/api";
import { lessonType } from "../lib/lessonTypes";
import { useSyncReload } from "../lib/sync";
import { MasteryPoints } from "../components/MasteryPoints";
import { CHALLENGE_EXPLAINED, CHALLENGE_LABEL, PERIOD_CHOICES, complexityName, complexityTone, dayToDate, dueLabel, kindName, minutes } from "../lib/lessons";
import type { LessonDetail, LessonId, PracticeContext, WikiSource } from "../lib/schemas";

/** Sections that head the page, above its columns: a scene, a story. */
const HEADERS = ["scene", "story"];
/** Sections whose pictures or recording a model makes, with the learner's key. */
const MEDIA_SECTIONS = ["pictures", "scene", "story", "song"];

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
  from,
  today,
  onBack,
  onStartDrill,
  onCheckIn,
  onDeleted,
  onRead,
  onGone,
}: {
  lessonId: LessonId;
  /** Where it was opened from (a course page, the daily five…), for the learning log. */
  from: PracticeContext;
  today: number;
  onBack: () => void;
  onStartDrill: (lessonId: LessonId, challenge: boolean) => void;
  onCheckIn: (lessonId: LessonId) => void;
  onDeleted: () => void;
  /** Opens one of its Wikipedia sources in the reader. */
  onRead: (source: WikiSource) => void;
  /** It was deleted elsewhere (another device, the tutor). */
  onGone: () => void;
}) {
  const [lesson, setLesson] = useState<LessonDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revealAll, setRevealAll] = useState(false);
  /** The level's explanation, open. */
  const [levelInfo, setLevelInfo] = useState(false);
  /** Adding audio players to the lesson's Polish. */
  const [addingPlayers, setAddingPlayers] = useState(false);
  /** Whether a key and a chat model are set, for what this lesson needs. */
  const [ai, setAi] = useState<{ key: boolean; model: boolean } | null>(null);

  const reload = () =>
    api
      .lessonDetail(lessonId)
      .then(setLesson)
      .catch((cause) => setError(errorMessage(cause)));
  useEffect(() => {
    api
      .getSettings()
      .then((settings) => setAi({ key: settings.hasApiKey, model: settings.aiModel !== "" }))
      .catch(() => setAi(null));
  }, []);
  useEffect(() => {
    void reload();
    // Everything done from this page is logged as practised from where it was opened.
    setPracticeContext(from);
    logEvent({ kind: "lesson_opened", lessonId, itemKind: "lesson", itemId: lessonId, ...from });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lessonId]);
  // The lesson as another device changed it (sync); gone, the page gives way.
  // Spec: SHELL-26
  useSyncReload(() =>
    void api
      .lessonDetail(lessonId)
      .then(setLesson)
      .catch(() => onGone()),
  );

  // Opening a lesson downloads its audio at normal speed in the background
  // (conversation first, in each speaker's voice), so every 🔊 here plays at
  // once. Slow versions are made only when 🐢 asks for one.
  const prefetched = useRef<LessonId | null>(null);
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

  if (!lesson) {
    return (
      <Screen title="Lesson" onBack={onBack} nav={false}>
        {error ? <Banner tone="error">{error}</Banner> : <Spinner />}
      </Screen>
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

  // What the lesson needs that is not set yet, from what its type is (a
  // conversation talks with a model) and shows (pictures, a recording): it
  // says so until then (DEMO-10).
  const makesMedia = type.sections.some((name) => MEDIA_SECTIONS.includes(name));
  const needs: string[] = [];
  if (ai && !ai.key && makesMedia) {
    needs.push(
      type.sections.includes("song")
        ? "Its recording is made once an OpenRouter key is set in Settings → AI."
        : "Its pictures are drawn once an OpenRouter key is set in Settings → AI.",
    );
  }
  if (ai && type.engine === "conversation" && (!ai.key || !ai.model)) {
    needs.push("It is practised as a conversation with an AI model: set an OpenRouter key and pick a model in Settings → AI.");
  }

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
            lessonId={lesson.id}
            factNote={lesson.factNote}
            factCheckedAt={lesson.factCheckedAt}
            onRead={onRead}
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
      case "scene":
        return <SceneHeader lesson={lesson} onChanged={reload} />;
      case "story":
        return <StoryHeader lesson={lesson} onChanged={reload} />;
      case "where":
        return lesson.scene && lesson.scene.statements.length > 0 ? (
          <Section title="Where is it?">
            <List>
              {lesson.scene.statements.map(([pl, en]) => (
                <ListItem key={pl} selectable title={pl} subtitle={en} trailing={<SpeakButton text={pl} />} />
              ))}
            </List>
          </Section>
        ) : null;
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

  // Spec: LSN-12
  const challengeMark = lesson.challenge ? <ChallengeMark /> : null;

  return (
    <Screen title={lesson.title} subtitle={lesson.courseTitle ?? undefined} onBack={onBack} nav={false}>
      {error ? <Banner tone="error">{error}</Banner> : null}
      {needs.length > 0 ? <Banner>{needs.join(" ")}</Banner> : null}

      {/* A picture that is the lesson (a scene) heads the page, across it. */}
      {type.sections
        .filter((name) => HEADERS.includes(name))
        .map((name) => (
          <Fragment key={name}>{section(name)}</Fragment>
        ))}

      {/* On a wide window: about the lesson on the left, what it teaches on
          the right. On a phone the wrappers vanish (display: contents). */}
      <div className="page-split">
        <div className="page-split__info">
          <Stack gap={3}>
            <Inline gap={2} wrap>
              {challengeMark}
              <Badge tone="brand">
                <Inline gap={1}>
                  <LessonIcon lesson={lesson} />
                  {kindName(lesson)}
                </Inline>
              </Badge>
              <Badge tone={complexityTone(lesson.complexity)}>
                {complexityName(lesson.complexity)} · {minutes(lesson)}
              </Badge>
              {lesson.mastered ? (
                <Badge tone="gold">🏆 Mastered</Badge>
              ) : started ? (
                <Badge>{dueLabel(lesson, today)}</Badge>
              ) : null}
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
              onClick={() => (type.engine === "conversation" ? onCheckIn(lesson.id) : onStartDrill(lesson.id, lesson.challenge))}
            >
              {started ? type.againLabel : type.startLabel}
            </Button>
            {started ? <MasteryPoints lesson={lesson} /> : null}
          </Stack>
        </div>

        <div className="page-split__main">
          {type.sections
            .filter((name) => !HEADERS.includes(name))
            .map((name) => (
              <Fragment key={name}>{section(name)}</Fragment>
            ))}
        </div>

        <div className="page-split__side">
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
                  lesson.mastered
                    ? "Mastered — it is never due again"
                    : lesson.dueDay === null
                      ? "Starts the first time you do it"
                      : lesson.dueDay <= today
                        ? "Due now"
                        : `Next check-in ${dayToDate(lesson.dueDay)}`
                }
                description={
                  lesson.mastered
                    ? "Mastery is for good. Practise it whenever you like; now and then its words come back as a light review in today's lessons."
                    : started
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

          {lesson.courses.length > 0 ? (
            <Section
              title="Course"
              info={
                <p>
                  A lesson is in one course, and its words and sentences belong to it. For the same practice in another
                  course, ask the tutor for a new lesson there.
                </p>
              }
            >
              <List>
                {lesson.courses.map(([id, title, titlePl]) => (
                  <ListItem key={id} title={titlePl} subtitle={title} />
                ))}
              </List>
            </Section>
          ) : null}

          <Section title="Manage">
            <List>
              <SettingRow
                label={CHALLENGE_LABEL}
                description={CHALLENGE_EXPLAINED}
                control={
                  <Switch
                    label={CHALLENGE_LABEL}
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
              question="Delete this lesson, its words and its check-in history?"
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
        </div>
      </div>
    </Screen>
  );
}
