import { useEffect, useState } from "react";

import { CloseIcon, DrillIcon, NavIcon, PlayerIcon, PlusIcon, StudyIcon } from "../components/icons";
import { Markdown } from "../components/Markdown";
import { SongToListen } from "../components/SongToListen";
import { SpeakButton } from "../components/ui";
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
  Sheet,
  Spinner,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import {
  PERIOD_CHOICES,
  complexityName,
  complexityTone,
  dayToDate,
  dueLabel,
  kindName,
  minutes,
} from "../lib/blocks";
import type { BlockDetail, TopicProgress } from "../lib/types";

const RESULT_TEXT = { pass: "Passed", partial: "Partly there", fail: "Not yet" } as const;

/**
 * One block: what it is, the lesson or the items it practises, and its
 * check-in schedule. The one primary action starts it.
 */
export function BlockPage({
  blockId,
  today,
  onBack,
  onStartDrill,
  onCheckIn,
  onDeleted,
}: {
  blockId: number;
  today: number;
  onBack: () => void;
  onStartDrill: (blockId: number) => void;
  onCheckIn: (blockId: number) => void;
  onDeleted: () => void;
}) {
  const [block, setBlock] = useState<BlockDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The course picker: add the block to one, or move it there. */
  const [picking, setPicking] = useState<{ mode: "add" | "move"; courses: TopicProgress[] } | null>(null);

  const reload = () =>
    api.blockDetail(blockId).then(setBlock).catch((cause) => setError(errorMessage(cause)));
  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [blockId]);

  async function openPicker(mode: "add" | "move") {
    try {
      setPicking({ mode, courses: await api.getTopicProgress() });
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

  if (!block) {
    return (
      <Screen title="Block" onBack={onBack} nav={false}>
        {error ? <Banner tone="error">{error}</Banner> : <Spinner />}
      </Screen>
    );
  }

  const study = block.kind === "study";
  const listening = block.songId !== null;
  const started = block.dueDay !== null;
  const period = String(block.periodDays);
  const periodOptions = PERIOD_CHOICES.some((choice) => choice.value === period)
    ? PERIOD_CHOICES
    : [...PERIOD_CHOICES, { value: period, label: `${period} days` }].sort(
        (a, b) => Number(a.value) - Number(b.value),
      );

  return (
    <Screen title={block.title} subtitle={block.topicTitle ?? undefined} onBack={onBack} nav={false}>
      {error ? <Banner tone="error">{error}</Banner> : null}

      <Stack gap={3}>
        <Inline gap={2} wrap>
          <Badge tone="brand">
            <Inline gap={1}>
              {listening ? <NavIcon.songs /> : study ? <StudyIcon /> : <DrillIcon />}
              {kindName(block)}
            </Inline>
          </Badge>
          <Badge tone={complexityTone(block.complexity)}>
            {complexityName(block.complexity)} · {minutes(block.complexity)}
          </Badge>
          {started ? <Badge>{dueLabel(block, today)}</Badge> : null}
        </Inline>
        {block.summary ? (
          <Text tone="dim" as="p">
            {block.summary}
          </Text>
        ) : null}
        <Button
          variant="primary"
          size="lg"
          block
          icon={<PlayerIcon.play size={18} />}
          onClick={() => (study ? onCheckIn(block.id) : onStartDrill(block.id))}
        >
          {study
            ? started
              ? "Check in"
              : listening
                ? "I've listened — check me"
                : "I've studied it — check me"
            : started
              ? "Practise again"
              : "Start"}
        </Button>
      </Stack>

      {listening ? (
        <Section title="Listen">
          <SongToListen songId={block.songId!} />
        </Section>
      ) : null}

      {study ? (
        <>
          <Section title="You will be able to">
            <List>
              {block.objectives.map((objective) => (
                <ListItem key={objective} title={objective} />
              ))}
            </List>
          </Section>
          <Section title={listening ? "Note" : "Lesson"}>
            <Card>
              <div className="selectable">
                <Markdown text={block.lesson} />
              </div>
            </Card>
          </Section>
        </>
      ) : null}

      {!study && block.words.length > 0 ? (
        <Section title={`${block.words.length} words`}>
          <List>
            {block.words.map(([pl, en]) => (
              <ListItem key={pl} selectable title={pl} subtitle={en} trailing={<SpeakButton text={pl} />} />
            ))}
          </List>
        </Section>
      ) : null}
      {!study && block.sentences.length > 0 ? (
        <Section title={`${block.sentences.length} sentences`}>
          <List>
            {block.sentences.map(([pl, en]) => (
              <ListItem key={pl} selectable title={pl} subtitle={en} trailing={<SpeakButton text={pl} />} />
            ))}
          </List>
        </Section>
      ) : null}

      <Section title="Check-ins">
        <List>
          {block.lastResult ? (
            <SettingRow
              label={`Last time: ${RESULT_TEXT[block.lastResult]}`}
              description={block.lastFeedback ?? undefined}
            />
          ) : null}
          <SettingRow
            label={
              !started
                ? "Starts the first time you do it"
                : block.dueDay! <= today
                  ? "Due now"
                  : `Next check-in ${dayToDate(block.dueDay!)}`
            }
            description={
              started
                ? "Each pass pushes the next one further out; struggling brings it back sooner."
                : `Then it comes back about every ${block.periodDays} days, further apart as you pass.`
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
                        await api.blockSetPeriod(block.id, Number(option.value));
                        setBlock({ ...block, periodDays: Number(option.value) });
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
        title={block.courses.length === 1 ? "Course" : "Courses"}
        info={
          <p>
            A block can be in several courses: it stays one block, with one schedule, shown in each.
            Its words and sentences belong to the block and go wherever it goes. Taking it out of its
            last course deletes it — use Delete block for that.
          </p>
        }
      >
        <Stack gap={2}>
          <List>
            {block.courses.map(([id, title, titlePl]) => (
              <ListItem
                key={id}
                title={titlePl}
                subtitle={title}
                trailing={
                  block.courses.length > 1 ? (
                    <IconButton
                      icon={<CloseIcon />}
                      label={`Take it out of ${title}`}
                      onClick={() => changeCourses(() => api.blockRemoveFromCourse(block.id, id))}
                    />
                  ) : undefined
                }
              />
            ))}
          </List>
          <Inline gap={2}>
            <div className="grow">
              <Button block icon={<PlusIcon />} onClick={() => openPicker("add")}>
                Also in…
              </Button>
            </div>
            {block.courses.length === 1 ? (
              <div className="grow">
                <Button block onClick={() => openPicker("move")}>
                  Move to…
                </Button>
              </div>
            ) : null}
          </Inline>
        </Stack>
      </Section>

      {picking ? (
        <Sheet
          title={picking.mode === "add" ? "Also show it in" : "Move it to"}
          onClose={() => setPicking(null)}
        >
          <List>
            {picking.courses
              .filter((entry) => !block.courses.some(([id]) => id === entry.topic.id))
              .map((entry) => (
                <ListItem
                  key={entry.topic.id}
                  title={entry.topic.titlePl}
                  subtitle={entry.topic.title}
                  onClick={() =>
                    changeCourses(async () => {
                      await api.blockAddToCourse(block.id, entry.topic.id);
                      if (picking.mode === "move") {
                        await api.blockRemoveFromCourse(block.id, block.courses[0][0]);
                      }
                    })
                  }
                />
              ))}
          </List>
        </Sheet>
      ) : null}

      <Section title="Manage">
        <ConfirmButton
          label="Delete block"
          confirmLabel="Delete"
          question="Delete this block, its words and its check-in history, from every course it is in?"
          block
          onConfirm={async () => {
            try {
              await api.blockDelete(block.id);
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
