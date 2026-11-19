import { useEffect, useRef, useState } from "react";

import { Composer } from "../components/Composer";
import { MemoryIcon, PlayerIcon } from "../components/icons";
import { playSong, useSongItem } from "../components/SongToListen";
import { Markdown } from "../components/Markdown";
import { ReadAloud, unsay } from "../components/ReadAloud";
import {
  Badge,
  Banner,
  Button,
  Card,
  IconButton,
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  Typing,
  Stack,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { toast } from "../lib/toast";
import { WordBank } from "../components/WordBank";
import { useAiStream } from "../lib/useAiStream";
import { useStickToBottom } from "../lib/stickToBottom";
import { JumpToLatest } from "../components/JumpToLatest";
import { currentItem, usePlayer } from "../lib/player";
import { dayToDate } from "../lib/lessons";
import type { Assessment, CheckinId, LessonId, PracticeContext, SongId } from "../lib/types";

/** A writing lesson's situation and goals. */
export interface Writing {
  situation: string;
  goals: string[];
  /** 0 guided (word bank, hints), 1 supported (hints), 2 free. */
  supportLevel: number;
}

const SUPPORT = ["Guided", "Supported", "Free"] as const;

const VERDICT = {
  pass: { title: "Passed", tone: "mint", line: "You know this." },
  partial: { title: "Partly there", tone: "gold", line: "Most of it is there." },
  fail: { title: "Not yet", tone: "crimson", line: "Worth another look." },
} as const;

interface Line {
  /**
   * "memory": something the examiner saved to long-term memory. "hint": a
   * writing hint, for the learner only.
   */
  role: "user" | "assistant" | "memory" | "hint";
  content: string;
}

/**
 * A study lesson's check-in: the examiner asks, the learner answers, and at
 * the end the examiner gives a verdict that schedules the next check-in.
 */
export function CheckIn({
  lessonId,
  from,
  songId,
  writing,
  title,
  onClose,
  onFinished,
}: {
  lessonId: LessonId;
  /** Where the lesson was opened from: its verdict is logged with it. */
  from: PracticeContext;
  /** For a listening lesson: its song, playable during the check-in. */
  songId: SongId | null;
  /** For a writing lesson: the conversation is with a partner, in Polish. */
  writing?: Writing | null;
  title: string;
  onClose: () => void;
  /** The verdict is in; `first` is true when it earned today's streak. */
  onFinished: (first: boolean, checkinId: CheckinId, assessment: Assessment) => void;
}) {
  const [checkinId, setCheckinId] = useState<CheckinId | null>(null);
  const [lines, setLines] = useState<Line[]>([]);
  const stream = useAiStream();
  const live = stream.text;
  // Busy from the start: the examiner opens with the first question.
  const busy = stream.streaming || (checkinId === null && !stream.error);
  const [draft, setDraft] = useState("");
  const error = stream.error;
  const [assessment, setAssessment] = useState<Assessment | null>(null);
  const [first, setFirst] = useState(false);
  const started = useRef(false);
  const song = useSongItem(songId).item;
  const player = usePlayer();
  const songCurrent = song !== null && currentItem(player)?.track.id === song.track.id;

  // Follows the examiner — until the learner scrolls up to read.
  const scroll = useStickToBottom([lines, live, assessment]);
  const endRef = scroll.endRef;

  async function exchange(run: (streamId: string) => ReturnType<typeof api.checkinStart>) {
    const turn = await stream.run(run);
    if (!turn) return false;
    stream.reset();
    setCheckinId(turn.checkinId);
    setLines((current) => [
      ...current,
      ...(turn.reply ? [{ role: "assistant" as const, content: turn.reply }] : []),
      ...turn.remembered.map((content) => ({ role: "memory" as const, content })),
    ]);
    if (turn.assessment) {
      setAssessment(turn.assessment);
      setFirst(turn.firstCompletionToday);
    }
    return true;
  }

  // The examiner opens with the first question.
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void exchange((streamId) => api.checkinStart(lessonId, streamId, from));
  }, [lessonId]);

  async function answer() {
    const text = draft.trim();
    if (!text || busy || checkinId === null) return;
    setDraft("");
    scroll.follow();
    setLines((current) => [...current, { role: "user", content: text }]);
    const ok = await exchange((streamId) => api.checkinSend(checkinId, text, streamId));
    if (!ok) {
      setLines((current) => current.slice(0, -1));
      setDraft(text);
    }
  }

  // Writing help: a hint for the next message (guided and supported).
  const [hinting, setHinting] = useState(false);
  async function hint() {
    if (checkinId === null || hinting) return;
    setHinting(true);
    try {
      const text = await api.writingHint(checkinId);
      setLines((current) => [...current, { role: "hint", content: text }]);
      scroll.follow();
    } catch (cause) {
      toast(errorMessage(cause), "error");
    } finally {
      setHinting(false);
    }
  }
  /** Adds a word or a hint's sentence to the message being written. */
  function use(text: string) {
    setDraft((current) => (current && !/\s$/.test(current) ? `${current} ${text}` : `${current}${text}`));
  }

  async function end() {
    if (busy || checkinId === null) return;
    await exchange((streamId) => api.checkinEnd(checkinId, streamId));
  }

  const verdict = assessment ? VERDICT[assessment.result] : null;

  return (
    <Screen
      title={title}
      subtitle={writing ? `Writing · ${SUPPORT[Math.min(2, writing.supportLevel)]}` : "Conversation"}
      onClose={onClose}
      nav={false}
      actions={
        writing ? (
          !assessment && checkinId !== null ? (
            <Inline gap={1}>
              {writing.supportLevel < 2 ? (
                <Button size="sm" variant="plain" loading={hinting} disabled={busy} onClick={() => void hint()}>
                  💡 Hint
                </Button>
              ) : null}
              <Button size="sm" disabled={busy} onClick={() => void end()}>
                End
              </Button>
            </Inline>
          ) : undefined
        ) : song ? (
          <IconButton
            variant="tinted"
            icon={songCurrent && player.playing ? <PlayerIcon.pause size={18} /> : <PlayerIcon.play size={18} />}
            label={songCurrent && player.playing ? "Pause the song" : "Play the song"}
            onClick={() => playSong(song, songCurrent)}
          />
        ) : undefined
      }
      footer={
        assessment ? (
          <Button
            variant="primary"
            size="lg"
            wide
            onClick={() => checkinId !== null && assessment && onFinished(first, checkinId, assessment)}
          >
            Done
          </Button>
        ) : (
          <Stack gap={2}>
            {writing && writing.supportLevel === 0 ? <WordBank lessonId={lessonId} draft={draft} onUse={use} /> : null}
            <Composer
              draft={draft}
              setDraft={setDraft}
              placeholder={writing ? "Write in Polish" : "Your answer"}
              disabled={checkinId === null}
              dictate
              canSend={!busy}
              onSend={() => void answer()}
            />
          </Stack>
        )
      }
    >
      {error ? (
        <Stack gap={2}>
          <Banner tone="error">{error}</Banner>
          {checkinId === null ? (
            <Button onClick={() => void exchange((streamId) => api.checkinStart(lessonId, streamId, from))}>Try again</Button>
          ) : null}
        </Stack>
      ) : null}

      {writing ? (
        <Card>
          <Stack gap={2}>
            <div className="selectable">
              <Markdown text={writing.situation} />
            </div>
            <Text variant="small" tone="dim">
              Get across: {writing.goals.join(" · ")}
            </Text>
          </Stack>
        </Card>
      ) : null}

      <div className="chat">
        {lines.map((line, i) =>
          line.role === "hint" ? (
            <button
              key={i}
              className="writing-hint"
              aria-label="Use this hint"
              onClick={() => setDraft(line.content.split("(")[0].trim())}
            >
              <Text variant="caption" tone="dim">
                💡 Hint — tap to start from it
              </Text>
              <span>{line.content}</span>
            </button>
          ) : line.role === "memory" ? (
            <div key={i} className="tutor-action">
              <Inline gap={2} align="top">
                <span className="tone-ok">
                  <MemoryIcon />
                </span>
                <Text variant="small" tone="dim">
                  {line.content}
                </Text>
              </Inline>
            </div>
          ) : (
            <div
              key={i}
              className="msg selectable"
              data-role={line.role}
              // The learner's answers are all sent from here: each rises into place.
              data-fresh={line.role === "user" || undefined}
            >
              {line.role !== "assistant" ? (
                line.content
              ) : writing ? (
                // The partner writes only Polish: one button reads the whole
                // message, instead of a player after every sentence.
                <>
                  <Markdown text={unsay(line.content)} />
                  <ReadAloud text={line.content} />
                </>
              ) : (
                <Markdown text={line.content} />
              )}
            </div>
          ),
        )}
        {busy ? (
          <div className="msg" data-role="assistant" data-fresh>
            {live ? (
              <div>
                <Markdown text={writing ? unsay(live) : live} />
                <span className="caret" />
              </div>
            ) : (
              <Inline gap={2}>
                <Typing />
                <Text variant="small" tone="faint">
                  {lines.length === 0
                    ? writing
                      ? "Setting the scene…"
                      : "Preparing your questions…"
                    : writing
                      ? "Writing…"
                      : "Thinking…"}
                </Text>
              </Inline>
            )}
          </div>
        ) : null}
        <div ref={endRef} />
      </div>
      {scroll.away ? <JumpToLatest onClick={scroll.follow} /> : null}

      {assessment && verdict ? (
        <Section title="Verdict">
          <List>
            <ListItem
              title={verdict.title}
              subtitle={verdict.line}
              trailing={
                <Inline gap={1}>
                  <Badge tone={verdict.tone}>{verdict.title}</Badge>
                  {assessment.xp > 0 ? <Badge tone="gold">+{assessment.xp} XP</Badge> : null}
                </Inline>
              }
            />
            <ListItem title={writing ? "How it went" : "What the examiner said"} subtitle={assessment.summary} />
            {assessment.strengths.length > 0 ? (
              <ListItem title="Strong" subtitle={assessment.strengths.join(" · ")} />
            ) : null}
            {assessment.toReview.length > 0 ? (
              <ListItem title={writing ? "Corrections" : "Go over again"} subtitle={assessment.toReview.join(" · ")} />
            ) : null}
            <ListItem title="Next check-in" subtitle={dayToDate(assessment.nextDueDay)} />
          </List>
        </Section>
      ) : null}
    </Screen>
  );
}
