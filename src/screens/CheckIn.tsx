import { useEffect, useRef, useState } from "react";

import { Composer } from "../components/Composer";
import { MemoryIcon, PlayerIcon } from "../components/icons";
import { playSong, useSongItem } from "../components/SongToListen";
import { Markdown } from "../components/Markdown";
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
  Spinner,
  Stack,
  Text,
} from "../design";
import { api } from "../lib/api";
import { useAiStream } from "../lib/useAiStream";
import { useStickToBottom } from "../lib/stickToBottom";
import { JumpToLatest } from "../components/JumpToLatest";
import { currentItem, usePlayer } from "../lib/player";
import { dayToDate } from "../lib/blocks";
import type { Assessment } from "../lib/types";


/** A writing block's situation and goals. */
export interface Writing {
  situation: string;
  goals: string[];
}

const VERDICT = {
  pass: { title: "Passed", tone: "mint", line: "You know this." },
  partial: { title: "Partly there", tone: "gold", line: "Most of it is there." },
  fail: { title: "Not yet", tone: "crimson", line: "Worth another look." },
} as const;

interface Line {
  /** "memory": something the examiner saved to long-term memory. */
  role: "user" | "assistant" | "memory";
  content: string;
}

/**
 * A study block's check-in: the examiner asks, the learner answers, and at
 * the end the examiner gives a verdict that schedules the next check-in.
 */
export function CheckIn({
  blockId,
  songId,
  writing,
  title,
  onClose,
  onFinished,
}: {
  blockId: number;
  /** For a listening block: its song, playable during the check-in. */
  songId: number | null;
  /** For a writing block: the conversation is with a partner, in Polish. */
  writing?: Writing | null;
  title: string;
  onClose: () => void;
  /** The verdict is in; `first` is true when it earned today's streak. */
  onFinished: (first: boolean, checkinId: number, assessment: Assessment) => void;
}) {
  const [checkinId, setCheckinId] = useState<number | null>(null);
  const [lines, setLines] = useState<Line[]>([]);
  const stream = useAiStream();
  const live = stream.text;
  // Busy from the start: the examiner opens with the first question.
  const busy = stream.streaming || checkinId === null && !stream.error;
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
    void exchange((streamId) => api.checkinStart(blockId, streamId));
  }, [blockId]);

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

  async function end() {
    if (busy || checkinId === null) return;
    await exchange((streamId) => api.checkinEnd(checkinId, streamId));
  }

  const verdict = assessment ? VERDICT[assessment.result] : null;

  return (
    <Screen
      title={title}
      subtitle={writing ? "Writing" : "Check-in"}
      onClose={onClose}
      nav={false}
      actions={
        writing ? (
          !assessment && checkinId !== null ? (
            <Button size="sm" disabled={busy} onClick={() => void end()}>
              End
            </Button>
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
          <Button variant="primary" size="lg" block onClick={() => checkinId !== null && assessment && onFinished(first, checkinId, assessment)}>
            Done
          </Button>
        ) : (
          <Composer
            draft={draft}
            setDraft={setDraft}
            placeholder={writing ? "Write in Polish" : "Your answer"}
            disabled={checkinId === null}
            canSend={!busy}
            onSend={() => void answer()}
          />
        )
      }
    >
      {error ? (
        <Stack gap={2}>
          <Banner tone="error">{error}</Banner>
          {checkinId === null ? (
            <Button onClick={() => void exchange((streamId) => api.checkinStart(blockId, streamId))}>
              Try again
            </Button>
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
          line.role === "memory" ? (
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
            <div key={i} className="msg selectable" data-role={line.role}>
              {line.role === "assistant" ? <Markdown text={line.content} /> : line.content}
            </div>
          ),
        )}
        {busy ? (
          <div className="msg" data-role="assistant">
            {live ? (
              <div>
                <Markdown text={live} />
                <span className="caret" />
              </div>
            ) : (
              <Inline gap={2}>
                <Spinner />
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
            <ListItem
              title={writing ? "How it went" : "What the examiner said"}
              subtitle={assessment.summary}
            />
            {assessment.strengths.length > 0 ? (
              <ListItem title="Strong" subtitle={assessment.strengths.join(" · ")} />
            ) : null}
            {assessment.toReview.length > 0 ? (
              <ListItem
                title={writing ? "Corrections" : "Go over again"}
                subtitle={assessment.toReview.join(" · ")}
              />
            ) : null}
            <ListItem title="Next check-in" subtitle={dayToDate(assessment.nextDueDay)} />
          </List>
        </Section>
      ) : null}
    </Screen>
  );
}
