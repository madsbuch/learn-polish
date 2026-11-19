import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { Button, Text } from "../design";
import type { GlossWord } from "../lib/types";

/*
 * What a tapped Polish word means, in a small popover under it (or over it
 * when there is no room below): its dictionary form when it was a form of
 * it, its meaning, and the tutor to talk to about it — or, when nothing is
 * known, the tutor to ask. Shared by the music player's transcript
 * (FollowLyrics) and story lessons (StoryLines). It lives inside the box
 * that scrolls with the words, so it moves with its word.
 */

/** A word tapped: where its popover goes (inside the box, so it scrolls with the word). */
export interface Tapped {
  key: string;
  word: string;
  form: string;
  line: string;
  gloss: string | null;
  /** The word's top and bottom in the box's content, and the bottom of what shows of it. */
  top: number;
  bottom: number;
  visibleBottom: number;
  left: number;
}

/** Where a tapped word is inside its box (the popover's offset parent). */
export function placeUnder(
  word: Element,
  box: HTMLElement,
): Pick<Tapped, "top" | "bottom" | "visibleBottom" | "left"> {
  const at = word.getBoundingClientRect();
  const frame = box.getBoundingClientRect();
  return {
    top: at.top - frame.top + box.scrollTop,
    bottom: at.bottom - frame.top + box.scrollTop,
    visibleBottom: box.scrollTop + box.clientHeight,
    left: Math.max(0, Math.min(at.left - frame.left, frame.width - 240)),
  };
}

/** Closes an open popover on a tap anywhere but a word (`words`, a selector) or the popover, or on Escape. */
export function useDismiss(open: boolean, close: () => void, words: string) {
  const closeRef = useRef(close);
  closeRef.current = close;
  useEffect(() => {
    if (!open) return undefined;
    const outside = (event: PointerEvent) => {
      const target = event.target;
      if (
        target instanceof Element &&
        target.closest(`.word-popover, ${words}`)
      )
        return;
      closeRef.current();
    };
    const escape = (event: KeyboardEvent) =>
      event.key === "Escape" && closeRef.current();
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [open, words]);
}

/** Room between a word and its popover. */
const GAP_PX = 4;

/** What a tapped word means — or, when nothing is known, the tutor to ask. */
export function WordPopover({
  tapped,
  gloss,
  onAsk,
}: {
  tapped: Tapped;
  gloss: GlossWord | null;
  onAsk: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // Under the word, or over it when there is no room below in what shows.
  const [top, setTop] = useState(tapped.bottom + GAP_PX);
  useLayoutEffect(() => {
    const height = ref.current?.offsetHeight ?? 0;
    const below = tapped.bottom + GAP_PX;
    const fitsBelow = below + height <= tapped.visibleBottom;
    setTop(
      fitsBelow || tapped.top - GAP_PX - height < 0
        ? below
        : tapped.top - GAP_PX - height,
    );
  }, [tapped, gloss]);
  return (
    <div
      ref={ref}
      className="word-popover"
      role="dialog"
      aria-label={`“${tapped.word}”`}
      style={{ "--top": `${top}px`, "--left": `${tapped.left}px` }}
    >
      <Text variant="lead" weight="bold" tone="polish" selectable>
        {tapped.word}
      </Text>
      {gloss ? (
        <>
          {gloss.pl.toLowerCase() !== tapped.form ? (
            <Text variant="caption" tone="faint">
              a form of {gloss.pl}
            </Text>
          ) : null}
          <Text variant="small">{gloss.en}</Text>
          {gloss.wordId ? (
            <Button size="sm" variant="plain" onClick={onAsk}>
              Talk about it
            </Button>
          ) : null}
        </>
      ) : (
        <>
          <Text variant="small" tone="dim">
            No meaning saved for this word yet.
          </Text>
          <Button size="sm" onClick={onAsk}>
            Ask the tutor
          </Button>
        </>
      )}
    </div>
  );
}
