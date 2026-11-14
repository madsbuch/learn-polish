import { useCallback, useEffect, useRef, useState, type DependencyList } from "react";

/** How close to the end still counts as "at the end", in pixels. */
const SLACK = 48;

/**
 * Keeps a chat scrolled to its newest line — but only while the reader is
 * there. Scrolling up to read lets go; scrolling back to the end (or
 * `follow`, e.g. on sending) takes hold again. `away` says the reader is
 * scrolled up, for a "back to the latest" button.
 *
 * Put `endRef` on an element at the end of the chat; `deps` are what grows
 * it (messages, streamed text).
 */
export function useStickToBottom(deps: DependencyList) {
  const endRef = useRef<HTMLDivElement>(null);
  const stuck = useRef(true);
  const [away, setAway] = useState(false);

  useEffect(() => {
    const scroller = endRef.current?.closest(".screen");
    if (!scroller) return;
    const onScroll = () => {
      const fromEnd = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
      stuck.current = fromEnd < SLACK;
      setAway(!stuck.current);
    };
    scroller.addEventListener("scroll", onScroll, { passive: true });
    return () => scroller.removeEventListener("scroll", onScroll);
  }, []);

  // New text: follow it only while held at the end. Instant, not smooth — a
  // smooth scroll's in-between positions would read as the reader scrolling.
  useEffect(() => {
    if (stuck.current) endRef.current?.scrollIntoView({ block: "end" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  const follow = useCallback(() => {
    stuck.current = true;
    setAway(false);
    endRef.current?.scrollIntoView({ block: "end" });
  }, []);

  return { endRef, away, follow };
}
