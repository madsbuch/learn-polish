import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { Button, IconButton, Text } from "../design";
import { useBackHandler } from "../lib/back";
import { hapticTick } from "../lib/haptics";
import {
  conversationKey,
  currentConversation,
  playConversation,
  stopConversation,
  subscribeConversation,
  type ConversationLine,
} from "../lib/speech";
import { toast } from "../lib/toast";
import type { LessonId, StoryInfo } from "../lib/types";
import { CloseIcon, PlayerIcon } from "./icons";
import { StoryDots, StoryLines, StoryPage, StoryPicture, usePageTurns } from "./Story";

/*
 * A story lesson's picture book: the story to read again from its page,
 * without the questions. A page at a time — turned with the buttons, a
 * swipe or the arrow keys, each page coming in like a page turned — and
 * "Listen to the whole story": every sentence read in turn (the narrator's
 * voice, or the voice of whoever says it), the pages turning with it.
 */

/** Pauses between sentences while the whole story is read. */
const GAP_MS = 450;

export function StoryBook({
  story,
  title,
  lessonId,
  listen = false,
  startAt = 0,
  onClose,
}: {
  story: StoryInfo;
  title: string;
  lessonId: LessonId;
  /** Opened with "Listen to the whole story": start reading at once. */
  listen?: boolean;
  /** The page to open at. */
  startAt?: number;
  onClose: () => void;
}) {
  const [page, setPage] = useState(Math.min(Math.max(0, startAt), story.panels.length - 1));
  const [direction, setDirection] = useState<"on" | "back">("on");
  const [english, setEnglish] = useState(false);
  const body = useRef<HTMLDivElement>(null);
  const total = story.panels.length;

  // Every sentence of the story, in order, and where each one is.
  const { lines, places } = useMemo(() => {
    const all: ConversationLine[] = [];
    const at: { page: number; line: number }[] = [];
    story.panels.forEach((panel, p) =>
      panel.lines.forEach((line, l) => {
        all.push({ text: line.pl, gender: line.gender });
        at.push({ page: p, line: l });
      }),
    );
    return { lines: all, places: at };
  }, [story.panels]);
  const key = conversationKey(lines);
  const playing = useSyncExternalStore(subscribeConversation, currentConversation);
  const narrating = playing?.key === key ? playing : null;
  const reading = narrating ? places[narrating.index] : undefined;

  // The pages follow the narration.
  useEffect(() => {
    if (reading && reading.page !== page) {
      setDirection(reading.page > page ? "on" : "back");
      setPage(reading.page);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reading?.page]);

  function listenFrom(p: number) {
    const first = places.findIndex((place) => place.page >= p);
    playConversation(lines, Math.max(0, first), GAP_MS).catch(() =>
      toast("The story could not be read aloud: check the voice in Settings → Voices & sound.", "error"),
    );
  }
  const started = useRef(false);
  useEffect(() => {
    if (listen && !started.current) {
      started.current = true;
      listenFrom(0);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [listen]);
  // Closing the book stops the reading.
  useEffect(() => () => stopConversation(), []);

  function close() {
    stopConversation();
    onClose();
  }
  useBackHandler(true, close);

  function turn(to: number) {
    if (to < 0 || to >= total || to === page) return;
    hapticTick();
    // Turning by hand while it reads: it carries on reading from there.
    if (narrating?.playing) listenFrom(to);
    setDirection(to > page ? "on" : "back");
    setPage(to);
  }
  usePageTurns(
    body,
    () => turn(page + 1),
    () => turn(page - 1),
  );

  const panel = story.panels[page];
  if (!panel) return null;
  return (
    <div className="story-book" role="dialog" aria-modal="true" aria-label={title}>
      <div className="story-book__top">
        <IconButton icon={<CloseIcon />} label="Close the book" onClick={close} />
        <div className="grow story-book__title">
          <Text weight="bold" ellipsis>
            {title}
          </Text>
          <StoryDots at={page + 1} total={total} />
        </div>
        <Button
          size="sm"
          variant={narrating?.playing ? "primary" : "secondary"}
          icon={narrating?.playing ? <PlayerIcon.stop size={14} /> : <PlayerIcon.play size={14} />}
          onClick={() => (narrating?.playing ? stopConversation() : listenFrom(page))}
        >
          {narrating?.playing ? "Stop" : "Listen"}
        </Button>
      </div>
      <div className="story-book__body" ref={body}>
        <div className="story-book__page page-turn-in" data-dir={direction} key={panel.id}>
          <StoryPage picture={<StoryPicture panel={panel} />}>
            <StoryLines
              panel={panel}
              glossary={story.glossary}
              lessonId={lessonId}
              english={english}
              speaking={reading?.page === page ? reading.line : -1}
            />
            <div className="story-book__tools">
              <Button size="sm" variant="plain" onClick={() => setEnglish((on) => !on)} aria-pressed={english}>
                {english ? "Hide English" : "Show English"}
              </Button>
            </div>
          </StoryPage>
        </div>
      </div>
      <div className="story-book__foot">
        <Button variant="secondary" disabled={page === 0} onClick={() => turn(page - 1)}>
          Back
        </Button>
        <Text variant="caption" tone="faint">
          {page + 1} of {total}
        </Text>
        {page + 1 < total ? (
          <Button variant="primary" onClick={() => turn(page + 1)}>
            Next page
          </Button>
        ) : (
          <Button variant="primary" onClick={close}>
            The end
          </Button>
        )}
      </div>
    </div>
  );
}
