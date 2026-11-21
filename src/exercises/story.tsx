import { useRef, useState } from "react";

import { StoryDots, StoryLines, StoryPage, StoryPicture, usePageTurns } from "../components/Story";
import { SpeakButton } from "../components/ui";
import { Badge, Button, CountUp, Inline, Text } from "../design";
import { fileUrl } from "../lib/files";
import { LessonIdSchema } from "../lib/types";
import { Options, pickedAnswer, readingAnswer } from "./shared";
import { defineExercise, type ExerciseProps } from "./types";

/*
 * Story lessons: read the story a page at a time — the picture, its
 * sentences (hear each, tap any word for its meaning) — turning the page
 * with the button, a swipe or the arrow key; after each page a quick
 * question (who, what, where; which picture shows a sentence; which came
 * first); and at the end, the words met and the questions got right.
 */

/** One page of the story. */
function Panel(props: ExerciseProps) {
  const { exercise, onNext } = props;
  const card = exercise.story;
  const page = useRef<HTMLDivElement>(null);
  const [english, setEnglish] = useState(false);
  usePageTurns(page, onNext);
  const panel = card?.panel;
  const lessonId = LessonIdSchema.safeParse(exercise.itemId).data;
  if (!card || !panel || !lessonId) return null;
  return (
    <div className="story-card" ref={page}>
      <div className="story-card__top">
        <Text variant="eyebrow" ellipsis>
          {card.title}
        </Text>
        <StoryDots at={panel.number} total={card.total} />
      </div>
      <StoryPage picture={<StoryPicture panel={panel} />}>
        <StoryLines panel={panel} glossary={card.glossary} lessonId={lessonId} english={english} />
        <Inline gap={2} justify="between" wrap>
          <Text variant="caption" tone="faint">
            Tap a word for its meaning.
          </Text>
          <Button size="sm" variant="plain" onClick={() => setEnglish((on) => !on)} aria-pressed={english}>
            {english ? "Hide English" : "Show English"}
          </Button>
        </Inline>
      </StoryPage>
    </div>
  );
}

export const storyPanel = defineExercise({
  kinds: ["storyPanel"],
  reading: true,
  wide: true,
  pageTurn: true,
  checkLabel: "Next page",
  ...readingAnswer,
  render: (props) => <Panel {...props} />,
});

/** Two pictures from the story to choose between (by panel id, shown as pictures only). */
function PictureChoice(props: ExerciseProps) {
  const { exercise, picked, isChecked, setPicked } = props;
  return (
    <div className="picture-options story-picture-options">
      {exercise.options.map((option, i) => {
        const image = exercise.optionImages[i];
        const state = !isChecked
          ? picked === option
            ? "picked"
            : "idle"
          : option === exercise.answer
            ? "right"
            : option === picked
              ? "wrong"
              : "idle";
        return (
          <button
            key={option}
            className="picture-option"
            data-state={state}
            disabled={isChecked}
            aria-label={`Picture ${i + 1}`}
            onClick={() => setPicked(option)}
          >
            {image != null ? <img src={fileUrl(image)} alt="" draggable={false} /> : null}
          </button>
        );
      })}
    </div>
  );
}

/** The question after a page. */
function Question(props: ExerciseProps) {
  const { exercise } = props;
  const card = exercise.story;
  if (!card) return null;
  if (card.ask === "picture") {
    return (
      <>
        <Text variant="eyebrow">{exercise.hint}</Text>
        <div className="prompt-card">
          <Text variant="lead" weight="bold" selectable>
            {exercise.prompt}
          </Text>
          <SpeakButton text={exercise.prompt} size="sm" />
        </div>
        <PictureChoice {...props} />
      </>
    );
  }
  return (
    <>
      <Text variant="eyebrow">{card.ask === "order" ? exercise.hint : "A question about the story"}</Text>
      {card.panel ? (
        <div className="story-question__picture">
          <StoryPicture panel={card.panel} small />
        </div>
      ) : null}
      <Text variant="headline" align="center" as="p" selectable>
        {exercise.prompt}
      </Text>
      <Options {...props} two={card.ask === "choice" && exercise.options.every((o) => o.length <= 18)} />
    </>
  );
}

export const storyQuestion = defineExercise({
  kinds: ["storyQuestion"],
  wide: true,
  ...pickedAnswer,
  render: (props) => <Question {...props} />,
  // The sentence a picture or an order question was about, to read and hear.
  verdictNote: (exercise) =>
    exercise.pl ? (
      <Inline gap={2}>
        <Text variant="small" selectable>
          {exercise.pl} — {exercise.meaning}
        </Text>
        <SpeakButton text={exercise.pl} size="sm" />
      </Inline>
    ) : null,
});

/** The end: how the questions went, and the words met. */
function End(props: ExerciseProps) {
  const { exercise, plan, results, sessionXp } = props;
  const card = exercise.story;
  if (!card) return null;
  const questions = plan.exercises.filter((e) => e.kind === "storyQuestion");
  const right = questions.filter((q) => results[q.key]).length;
  const fresh = card.words.filter((w) => w.new).length;
  return (
    <div className="story-end">
      <div className="story-end__head pop">
        <Text variant="display" aria-hidden="true">
          📖
        </Text>
        <h2>Koniec!</h2>
        <Text tone="dim" align="center" as="p">
          You read “{card.title}” — {card.total} pages.
        </Text>
      </div>
      <div className="story-end__stats">
        <div className="stat-chip">
          <Text variant="title" weight="bold">
            <CountUp value={right} from={0} duration={600} /> of {questions.length}
          </Text>
          <Text variant="caption" tone="dim">
            questions right
          </Text>
        </div>
        <div className="stat-chip">
          <Text variant="title" weight="bold">
            {card.words.length}
          </Text>
          <Text variant="caption" tone="dim">
            {fresh > 0 ? `words, ${fresh} new` : "words met"}
          </Text>
        </div>
        <div className="stat-chip">
          <Text variant="title" weight="bold">
            +<CountUp value={sessionXp} from={0} duration={800} />
          </Text>
          <Text variant="caption" tone="dim">
            XP so far
          </Text>
        </div>
      </div>
      {card.words.length > 0 ? (
        <div className="story-end__words">
          <Text variant="eyebrow">Words met</Text>
          <ul className="story-words">
            {card.words.map((w) => (
              <li key={w.wordId}>
                <SpeakButton text={w.pl} size="sm" />
                <span className="story-words__pl">{w.pl}</span>
                <span className="story-words__en">{w.en}</span>
                {w.new ? <Badge tone="brand">new</Badge> : null}
              </li>
            ))}
          </ul>
          <Text variant="small" tone="faint">
            Next, a quick go at them — then the story stays on the lesson's page to read again.
          </Text>
        </div>
      ) : null}
    </div>
  );
}

export const storyEnd = defineExercise({
  kinds: ["storyEnd"],
  reading: true,
  pageTurn: true,
  checkLabel: "Practise the words",
  ...readingAnswer,
  render: (props) => <End {...props} />,
});
