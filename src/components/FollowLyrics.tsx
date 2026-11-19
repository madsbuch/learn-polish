import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";

import { Button, Switch, Text, useMediaQuery } from "../design";
import { api } from "../lib/api";
import { hapticRight, hapticTick, hapticWrong } from "../lib/haptics";
import { sectionLabel } from "../lib/lyrics";
import { livePosition, seek, usePlayer, type QueueItem } from "../lib/player";
import { chooseBlanks, fold, LEAD_S, tokens, type Blank } from "../lib/singAlong";
import type { GlossWord, SongId, TimedLine, TrackId } from "../lib/types";
import { WordChat } from "./WordChat";
import { placeUnder, useDismiss, WordPopover, type Tapped } from "./WordPopover";

/**
 * Lyrics that follow the music: the line being sung is highlighted and kept
 * in view, and tapping a line jumps there. Without Suno's timings the lyrics
 * are shown whole, so there is always something to read along with.
 *
 * Tapping a Polish word shows what it means (singalong.rs: the song's
 * course, what it was written from, the learner's words), or offers the
 * tutor when nothing is known. *Sing along* turns some words into blanks a
 * few seconds before their line, to fill before the line ends — from three
 * choices, or typed with a keyboard — and keeps the score.
 *
 * Everything inside the scrolling box changes colours, backgrounds and
 * shadows only — never opacity or transforms (DESIGN.md, Motion: WebKit
 * would scroll those a moment apart). The choices sit outside the box.
 */
/** Who spoke last before line `i` — a name shows only when the speaker changes. */
function lastSpeaker(lines: { speaker?: string | null; section: string | null }[], i: number): string | null {
  for (let j = i - 1; j >= 0; j--) {
    if (lines[j].section) return j === i - 1 ? null : (lines[j].speaker ?? null);
    if (lines[j].speaker) return lines[j].speaker ?? null;
  }
  return null;
}

/** A blank's line has gone by this long and it's still empty: missed. */
const GRACE_S = 0.3;

/** The meanings of a song's words, by sung form. Empty until they come (or if they don't). */
function useGlossary(songId: SongId): Map<string, GlossWord> {
  const [glossary, setGlossary] = useState<{
    id: SongId;
    words: Map<string, GlossWord>;
  } | null>(null);
  useEffect(() => {
    let cancelled = false;
    api
      .songGlossary(songId)
      .then(
        (words) =>
          !cancelled &&
          setGlossary({
            id: songId,
            words: new Map(words.map((w) => [w.form, w])),
          }),
      )
      .catch(() => !cancelled && setGlossary({ id: songId, words: new Map() }));
    return () => {
      cancelled = true;
    };
  }, [songId]);
  return useMemo(() => (glossary?.id === songId ? glossary.words : new Map<string, GlossWord>()), [glossary, songId]);
}

type Mark = "right" | "missed";

export function FollowLyrics({ item }: { item: QueueItem }) {
  const player = usePlayer();
  const trackId = item.track.id;
  const songId = item.song.id;
  const [fetched, setFetched] = useState<{
    id: TrackId;
    lines: TimedLine[] | null;
  } | null>(null);
  const [, tick] = useState(0);
  const box = useRef<HTMLDivElement>(null);
  const currentRef = useRef<HTMLButtonElement>(null);
  const glossary = useGlossary(songId);
  const [tapped, setTapped] = useState<Tapped | null>(null);
  const [talk, setTalk] = useState<Tapped | null>(null);
  const [singing, setSinging] = useState(false);
  const [marks, setMarks] = useState<Record<number, Mark>>({});
  const [typed, setTyped] = useState("");
  // The score of the song just sung, kept on show when the next one starts.
  const [score, setScore] = useState<{
    title: string;
    right: number;
    total: number;
  } | null>(null);
  // A keyboard to type with: a computer (a phone taps the choices).
  const keyboard = useMediaQuery("(hover: hover) and (pointer: fine)");

  // Older songs get their timings the first time they are played.
  useEffect(() => {
    if (item.track.lines || fetched?.id === trackId) return;
    let cancelled = false;
    api
      .songTrackLyrics(trackId)
      .then((lines) => !cancelled && setFetched({ id: trackId, lines }))
      .catch(() => !cancelled && setFetched({ id: trackId, lines: null }));
    return () => {
      cancelled = true;
    };
  }, [trackId, item.track.lines, fetched?.id]);

  // Re-render a few times a second so the highlight keeps up between reports.
  useEffect(() => {
    if (!player.playing) return;
    const timer = window.setInterval(() => tick((n) => n + 1), 200);
    return () => window.clearInterval(timer);
  }, [player.playing]);

  const lines = item.track.lines ?? (fetched?.id === trackId ? fetched.lines : null);
  const position = livePosition(player);
  let current = -1;
  if (lines) {
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].start <= position + 0.15) current = i;
      else break;
    }
  }

  // The blanks, once per song (and once its meanings are in).
  const blanks = useMemo(() => (lines ? chooseBlanks(lines, glossary) : new Map<number, Blank>()), [lines, glossary]);
  // A new take, or Sing along started again: a fresh score.
  useEffect(() => {
    setMarks({});
    setTyped("");
  }, [trackId, singing]);
  useEffect(() => setScore(null), [singing]);
  useEffect(() => setTapped(null), [trackId]);

  // The blanks open now (from a few seconds before their line), the first
  // of them to answer, and any whose line has gone by unanswered.
  const open: number[] = [];
  const overdue: number[] = [];
  if (singing && lines) {
    for (const [i] of blanks) {
      if (marks[i]) continue;
      if (position > lines[i].end + GRACE_S) overdue.push(i);
      else if (position >= lines[i].start - LEAD_S) open.push(i);
    }
  }
  const active = open[0] ?? null;
  const overdueKey = overdue.join(",");
  useEffect(() => {
    if (!overdueKey) return;
    setMarks((m) => ({
      ...m,
      ...Object.fromEntries(overdueKey.split(",").map((i) => [i, "missed" as const])),
    }));
  }, [overdueKey]);
  useEffect(() => setTyped(""), [active]);
  const allMarked = singing && blanks.size > 0 && Object.keys(marks).length === blanks.size;
  const rightCount = Object.values(marks).filter((m) => m === "right").length;
  useEffect(() => {
    if (allMarked)
      setScore({
        title: item.song.title,
        right: rightCount,
        total: blanks.size,
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [allMarked]);

  // Keep the sung line in the upper part of the box, without moving the page.
  useEffect(() => {
    const container = box.current;
    const line = currentRef.current;
    if (!container || !line) return;
    const target = Math.max(0, Math.round(line.offsetTop - container.clientHeight * 0.35));
    // One smooth scroll per line change, and none when it is already there.
    if (Math.abs(container.scrollTop - target) < 2) return;
    container.scrollTo({ top: target, behavior: "smooth" });
  }, [current, trackId]);

  // A tap anywhere but a word or its popover closes the popover.
  useDismiss(tapped !== null, () => setTapped(null), ".lyric-word");

  function tap(
    event: MouseEvent<HTMLElement>,
    key: string,
    word: string,
    form: string,
    line: string,
    gloss: string | null,
  ) {
    // The word, not the line: the music stays where it is.
    event.stopPropagation();
    const container = box.current;
    if (!container) return;
    if (tapped?.key === key) {
      setTapped(null);
      return;
    }
    hapticTick();
    setTapped({
      key,
      word,
      form,
      line,
      gloss,
      ...placeUnder(event.currentTarget, container),
    });
  }

  function answer(given: string) {
    if (active === null) return;
    const blank = blanks.get(active);
    if (!blank) return;
    const right = fold(given) === fold(blank.answer);
    if (right) hapticRight();
    else hapticWrong();
    setMarks((m) => ({ ...m, [active]: right ? "right" : "missed" }));
  }

  /** A Polish line's words, each tappable; in Sing along, its blank. */
  function words(text: string, i: number, polish: boolean, gloss: string | null) {
    if (!polish) return text;
    const blank = singing ? blanks.get(i) : undefined;
    const mark = marks[i];
    const shown = blank && lines && (mark || position >= lines[i].start - LEAD_S);
    return tokens(text).map((t, k) => {
      const key = `${i}-${k}`;
      if (shown && blank.token === k) {
        return (
          <span key={key}>
            {t.before}
            <span
              className="lyric-blank"
              data-state={mark ?? (active === i ? "active" : "open")}
              aria-label={mark ? t.word : "a word to fill in"}
            >
              {t.word}
            </span>
            {t.after}
          </span>
        );
      }
      if (!t.form) return <span key={key}>{t.before + t.word + t.after}</span>;
      return (
        <span key={key}>
          {t.before}
          <span
            className="lyric-word"
            data-open={tapped?.key === key || undefined}
            onClick={(event) => tap(event, key, t.word, t.form, text, gloss)}
          >
            {t.word}
          </span>
          {t.after}
        </span>
      );
    });
  }

  const popover = tapped ? (
    <WordPopover
      tapped={tapped}
      gloss={glossary.get(tapped.form) ?? null}
      onAsk={() => {
        setTalk(tapped);
        setTapped(null);
      }}
    />
  ) : null;
  // The tutor about a tapped word: the learner's own word when it is one,
  // else the word in its line.
  const known = talk ? glossary.get(talk.form) : undefined;
  const chat = !talk ? null : known?.wordId ? (
    <WordChat itemKind="word" itemId={known.wordId} title={known.pl} onClose={() => setTalk(null)} />
  ) : (
    <WordChat
      itemKind="lyric"
      itemId={songId}
      title={talk.word}
      meaning={`“${talk.line}”${talk.gloss ? ` (${talk.gloss})` : ""}`}
      onClose={() => setTalk(null)}
    />
  );

  const total = blanks.size;
  const right = rightCount;
  const done = Object.keys(marks).length;
  const finished = allMarked;
  const blank = active !== null ? blanks.get(active) : undefined;
  // The score shows from the last blank until the next song's first.
  const shownScore = finished ? { title: item.song.title, right, total } : done === 0 && active === null ? score : null;

  const tools = (
    <div className="follow-tools">
      <Text variant="small" weight="medium">
        Sing along
      </Text>
      <Text variant="caption" tone="faint" className="grow">
        {singing
          ? total > 0
            ? done > 0
              ? `${right} of ${done} so far`
              : "Fill in the gaps as they come"
            : "No words to fill in this song"
          : "Fill in words as they come"}
      </Text>
      <Switch checked={singing} onChange={setSinging} label="Sing along" />
    </div>
  );
  const bar = singing ? (
    <div className="sing-bar" aria-live="polite">
      {shownScore ? (
        <div className="sing-score pop" key="score">
          <Text variant="title" weight="bold">
            {shownScore.right} of {shownScore.total}
          </Text>
          <Text variant="small" tone="dim" className="grow" ellipsis>
            {shownScore.right === shownScore.total ? "Every word — bravo!" : `words filled in “${shownScore.title}”`}
          </Text>
          {finished ? (
            <Button
              size="sm"
              onClick={() => {
                setMarks({});
                setScore(null);
                seek(0);
              }}
            >
              Sing again
            </Button>
          ) : null}
        </div>
      ) : blank && active !== null ? (
        <div className="sing-choices pop" key={active}>
          {blank.choices.map((choice) => (
            <button key={choice} className="option sing-choice" onClick={() => answer(choice)}>
              {choice}
            </button>
          ))}
          {keyboard ? (
            <input
              className="sing-type"
              value={typed}
              placeholder="or type it"
              aria-label="Type the missing word"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              onChange={(event) => setTyped(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && typed.trim()) {
                  event.preventDefault();
                  answer(typed);
                }
              }}
            />
          ) : null}
        </div>
      ) : (
        <Text variant="small" tone="faint" align="center" as="div">
          {!lines?.length
            ? "This one has no timings yet to sing along to."
            : player.playing
              ? "Listen: a gap opens just before its line."
              : "Play the song to sing along."}
        </Text>
      )}
    </div>
  ) : null;

  if (!lines || lines.length === 0) {
    // Sing along needs the timings; still on, it says so (and shows the
    // last song's score) and can be turned off.
    return (
      <>
        {singing ? (
          <>
            {tools}
            {bar}
          </>
        ) : null}
        <div className="follow-lyrics static" ref={box}>
          {item.song.sections.map((section, i) => (
            <div key={i} className="follow-section">
              <div className="t-eyebrow">{sectionLabel(section.kind)}</div>
              {section.lines.map((line, j) => (
                <div key={j} className="follow-line" data-lang="pair">
                  {section.kind !== "chorus" && line.en ? <span className="en">{line.en}</span> : null}
                  <span className="pl">{words(line.pl, -1 - i * 1000 - j, true, line.en || null)}</span>
                  {section.kind === "chorus" && line.en ? <span className="gloss">{line.en}</span> : null}
                </div>
              ))}
            </div>
          ))}
          {popover}
        </div>
        {chat}
      </>
    );
  }

  return (
    <>
      {tools}
      {bar}
      <div className="follow-lyrics" ref={box} data-singing={singing || undefined}>
        {lines.map((line, i) => (
          <div key={i}>
            {line.section ? <div className="t-eyebrow follow-eyebrow">{line.section}</div> : null}
            {line.speaker && line.speaker !== lastSpeaker(lines, i) ? (
              <div className="follow-speaker">{line.speaker}</div>
            ) : null}
            <button
              ref={i === current ? currentRef : undefined}
              className="follow-line"
              data-lang={line.lang || "other"}
              data-state={i === current ? "current" : i < current ? "past" : "next"}
              onClick={() => seek(line.start)}
            >
              <span className={line.lang === "en" ? "en" : "pl"}>
                {words(line.text, i, line.lang === "pl", line.gloss)}
              </span>
              {line.gloss ? <span className="gloss">{line.gloss}</span> : null}
            </button>
          </div>
        ))}
        <div className="follow-tail" />
        {popover}
      </div>
      {chat}
    </>
  );
}
