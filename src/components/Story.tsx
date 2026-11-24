import { useEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode, type RefObject } from "react";

import { Button, Spinner, Text } from "../design";
import { fileUrl } from "../lib/files";
import { hapticTick } from "../lib/haptics";
import { tokens } from "../lib/singAlong";
import type { GlossWord, LessonId, StoryPanel } from "../lib/schemas";
import { PencilIcon } from "./icons";
import { SlowButton, SpeakButton } from "./ui";
import { WordChat } from "./WordChat";
import { logEvent } from "../lib/learnLog";
import { placeUnder, useDismiss, WordPopover, type Tapped } from "./WordPopover";

/*
 * The pieces of a story lesson, shared by its cards in practice
 * (exercises/story.tsx) and the picture book on its page (StoryBook): a
 * panel's picture — or, until it is drawn, a drawing placeholder that says
 * where it has got to — and the sentences under it, each to hear normally
 * or slowly in the voice of whoever says it, every word tappable for its
 * meaning (the same popover as the song transcript), with the tutor to ask.
 */

const PLACEHOLDER = {
  waiting: "Waiting to be drawn",
  drawing: "Being drawn…",
  ready: "",
  failed: "This picture could not be drawn",
} as const;

/** A panel's picture in its own shape (known before it loads, so nothing jumps). */
export function StoryPicture({
  panel,
  onRetry,
  retrying = false,
  small = false,
}: {
  panel: StoryPanel;
  /** A failed picture: draw it again. */
  onRetry?: () => void;
  retrying?: boolean;
  /** A thumbnail: no words on the placeholder. */
  small?: boolean;
}) {
  const [loaded, setLoaded] = useState(false);
  const ratio = panel.width > 0 && panel.height > 0 ? `${panel.width} / ${panel.height}` : "4 / 3";
  return (
    <div
      className="story-picture"
      style={{ "--story-ratio": ratio }}
      data-status={panel.status}
      data-loaded={loaded || undefined}
      data-small={small || undefined}
    >
      {panel.image ? (
        <img src={fileUrl(panel.image)} alt={panel.picture} onLoad={() => setLoaded(true)} draggable={false} />
      ) : (
        <div className="story-picture__sketch" role="img" aria-label={`${PLACEHOLDER[panel.status]}: ${panel.picture}`}>
          {panel.status === "drawing" ? <Spinner /> : <PencilIcon />}
          {small ? null : (
            <>
              <Text variant="small" weight="bold">
                {PLACEHOLDER[panel.status]}
              </Text>
              <Text variant="caption" tone="faint" className="story-picture__what">
                {panel.status === "failed" && panel.error ? panel.error : panel.picture}
              </Text>
              {panel.status === "failed" && onRetry ? (
                <Button size="sm" loading={retrying} onClick={onRetry}>
                  Draw it again
                </Button>
              ) : null}
            </>
          )}
        </div>
      )}
    </div>
  );
}

/** The sentences under a panel: hear them, tap a word for its meaning. */
export function StoryLines({
  panel,
  glossary,
  lessonId,
  english,
  speaking = -1,
}: {
  panel: StoryPanel;
  glossary: GlossWord[];
  /** For asking the tutor about a word that is not one of the learner's. */
  lessonId: LessonId;
  /** Each sentence's English under it. */
  english: boolean;
  /** The sentence being read aloud (Listen to the whole story), or -1. */
  speaking?: number;
}) {
  const box = useRef<HTMLDivElement>(null);
  const meanings = useMemo(() => new Map(glossary.map((g) => [g.form, g])), [glossary]);
  const [tapped, setTapped] = useState<Tapped | null>(null);
  const [talk, setTalk] = useState<Tapped | null>(null);
  useDismiss(tapped !== null, () => setTapped(null), ".story-word");
  useEffect(() => setTapped(null), [panel.id]);

  function tap(event: MouseEvent<HTMLElement>, key: string, word: string, form: string, line: string, gloss: string) {
    event.stopPropagation();
    const container = box.current;
    if (!container) return;
    if (tapped?.key === key) {
      setTapped(null);
      return;
    }
    hapticTick();
    logEvent({ kind: "meaning_tapped", details: { word: form.slice(0, 80), source: "story" } });
    setTapped({
      key,
      word,
      form,
      line,
      gloss,
      ...placeUnder(event.currentTarget, container),
    });
  }

  const known = talk ? meanings.get(talk.form) : undefined;
  const chat = !talk ? null : known?.wordId ? (
    <WordChat itemKind="word" itemId={known.wordId} title={known.pl} onClose={() => setTalk(null)} />
  ) : (
    <WordChat
      itemKind="story"
      itemId={lessonId}
      title={talk.word}
      meaning={`“${talk.line}”${talk.gloss ? ` (${talk.gloss})` : ""}`}
      onClose={() => setTalk(null)}
    />
  );

  return (
    <div className="story-lines" ref={box}>
      {panel.lines.map((line, i) => (
        <div key={i} className="story-line" data-speaking={i === speaking || undefined}>
          {line.speaker ? <div className="story-line__speaker">{line.speaker}</div> : null}
          <div className="story-line__row">
            <p className="story-line__pl" data-said={line.speaker ? true : undefined}>
              {tokens(line.pl).map((t, k) => {
                const key = `${i}-${k}`;
                if (!t.form) return <span key={key}>{t.before + t.word + t.after}</span>;
                return (
                  <span key={key}>
                    {t.before}
                    <span
                      className="story-word"
                      data-lesson={meanings.get(t.form)?.lesson || undefined}
                      data-open={tapped?.key === key || undefined}
                      onClick={(event) => tap(event, key, t.word, t.form, line.pl, line.en)}
                    >
                      {t.word}
                    </span>
                    {t.after}
                  </span>
                );
              })}
            </p>
            <span className="story-line__sound">
              <SpeakButton text={line.pl} gender={line.gender} size="sm" />
              <SlowButton text={line.pl} gender={line.gender} size="sm" />
            </span>
          </div>
          {english ? (
            <Text variant="small" tone="dim" className="story-line__en" selectable>
              {line.en}
            </Text>
          ) : null}
        </div>
      ))}
      {tapped ? (
        <WordPopover
          tapped={tapped}
          gloss={meanings.get(tapped.form) ?? null}
          onAsk={() => {
            setTalk(tapped);
            setTapped(null);
          }}
        />
      ) : null}
      {chat}
    </div>
  );
}

/** "Panel 2 of 5" as dots: where in the story the learner is. */
export function StoryDots({ at, total }: { at: number; total: number }) {
  return (
    <div className="story-dots" role="img" aria-label={`Page ${at} of ${total}`}>
      {Array.from({ length: total }, (_, i) => (
        <span key={i} data-state={i + 1 < at ? "read" : i + 1 === at ? "here" : undefined} />
      ))}
    </div>
  );
}

/** How far a finger must travel sideways to turn a page. */
const SWIPE_PX = 48;

/**
 * Turns pages by swiping across `ref` (left: on, right: back) and with the
 * arrow keys on a computer. A tap is not a swipe: words stay tappable.
 */
export function usePageTurns(ref: RefObject<HTMLElement | null>, onNext: () => void, onBack?: () => void) {
  const next = useRef(onNext);
  const back = useRef(onBack);
  next.current = onNext;
  back.current = onBack;
  useEffect(() => {
    const element = ref.current;
    if (!element) return undefined;
    let start: { x: number; y: number; id: number } | null = null;
    const down = (event: PointerEvent) => {
      start = { x: event.clientX, y: event.clientY, id: event.pointerId };
    };
    const up = (event: PointerEvent) => {
      if (!start || start.id !== event.pointerId) return;
      const dx = event.clientX - start.x;
      const dy = event.clientY - start.y;
      start = null;
      if (Math.abs(dx) < SWIPE_PX || Math.abs(dx) < Math.abs(dy) * 1.5) return;
      if (dx < 0) next.current();
      else back.current?.();
    };
    const key = (event: KeyboardEvent) => {
      const target = event.target;
      if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) return;
      if (document.querySelector(".sheet")) return;
      if (event.key === "ArrowRight") {
        event.preventDefault();
        next.current();
      } else if (event.key === "ArrowLeft" && back.current) {
        event.preventDefault();
        back.current();
      }
    };
    element.addEventListener("pointerdown", down);
    element.addEventListener("pointerup", up);
    window.addEventListener("keydown", key);
    return () => {
      element.removeEventListener("pointerdown", down);
      element.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", key);
    };
  }, [ref]);
}

/** A page of a story: the picture, then its sentences (side by side on a big screen). */
export function StoryPage({ picture, children }: { picture: ReactNode; children: ReactNode }) {
  return (
    <div className="story-page">
      <div className="story-page__picture">{picture}</div>
      <div className="story-page__text">{children}</div>
    </div>
  );
}
