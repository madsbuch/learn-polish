import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { SendIcon } from "../components/icons";
import { Markdown } from "../components/Markdown";
import {
  Badge,
  Banner,
  Button,
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
import { api, errorMessage } from "../lib/api";
import { dayToDate } from "../lib/blocks";
import type { Assessment, StreamEvent } from "../lib/types";

let nextStream = 0;

const VERDICT = {
  pass: { title: "Passed", tone: "mint", line: "You know this." },
  partial: { title: "Partly there", tone: "gold", line: "Most of it is there." },
  fail: { title: "Not yet", tone: "crimson", line: "Worth another look." },
} as const;

interface Line {
  role: "user" | "assistant";
  content: string;
}

/**
 * A study block's check-in: the examiner asks, the learner answers, and at
 * the end the examiner gives a verdict that schedules the next check-in.
 */
export function CheckIn({
  blockId,
  title,
  onClose,
  onFinished,
}: {
  blockId: number;
  title: string;
  onClose: () => void;
  /** The verdict is in; `first` is true when it earned today's streak. */
  onFinished: (first: boolean) => void;
}) {
  const [checkinId, setCheckinId] = useState<number | null>(null);
  const [lines, setLines] = useState<Line[]>([]);
  const [live, setLive] = useState("");
  const [busy, setBusy] = useState(true);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [assessment, setAssessment] = useState<Assessment | null>(null);
  const [first, setFirst] = useState(false);
  const activeStream = useRef<string | null>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const started = useRef(false);

  useEffect(() => {
    const off = listen<StreamEvent>("ai://stream", (event) => {
      if (event.payload.id === activeStream.current && event.payload.delta) {
        setLive((current) => current + event.payload.delta);
      }
    });
    return () => {
      void off.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [lines, live, assessment]);

  async function exchange(run: (streamId: string) => ReturnType<typeof api.checkinStart>) {
    const streamId = `checkin${nextStream++}`;
    activeStream.current = streamId;
    setLive("");
    setBusy(true);
    setError(null);
    try {
      const turn = await run(streamId);
      setCheckinId(turn.checkinId);
      if (turn.reply) setLines((current) => [...current, { role: "assistant", content: turn.reply }]);
      if (turn.assessment) {
        setAssessment(turn.assessment);
        setFirst(turn.firstCompletionToday);
      }
      return true;
    } catch (cause) {
      setError(errorMessage(cause));
      return false;
    } finally {
      activeStream.current = null;
      setLive("");
      setBusy(false);
    }
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
    setLines((current) => [...current, { role: "user", content: text }]);
    const ok = await exchange((streamId) => api.checkinSend(checkinId, text, streamId));
    if (!ok) {
      setLines((current) => current.slice(0, -1));
      setDraft(text);
    }
  }

  const verdict = assessment ? VERDICT[assessment.result] : null;

  return (
    <Screen
      title={title}
      subtitle="Check-in"
      onClose={onClose}
      nav={false}
      footer={
        assessment ? (
          <Button variant="primary" size="lg" block onClick={() => onFinished(first)}>
            Done
          </Button>
        ) : (
          <div className="composer">
            <textarea
              rows={1}
              value={draft}
              placeholder="Your answer"
              disabled={checkinId === null}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  void answer();
                }
              }}
            />
            <IconButton
              variant="primary"
              icon={<SendIcon />}
              label="Send"
              disabled={busy || draft.trim().length === 0}
              onClick={answer}
            />
          </div>
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

      <div className="chat">
        {lines.map((line, i) => (
          <div key={i} className="msg selectable" data-role={line.role}>
            {line.role === "assistant" ? <Markdown text={line.content} /> : line.content}
          </div>
        ))}
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
                  {lines.length === 0 ? "Preparing your questions…" : "Thinking…"}
                </Text>
              </Inline>
            )}
          </div>
        ) : null}
        <div ref={endRef} />
      </div>

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
            <ListItem title="What the examiner said" subtitle={assessment.summary} />
            {assessment.strengths.length > 0 ? (
              <ListItem title="Strong" subtitle={assessment.strengths.join(" · ")} />
            ) : null}
            {assessment.toReview.length > 0 ? (
              <ListItem title="Go over again" subtitle={assessment.toReview.join(" · ")} />
            ) : null}
            <ListItem title="Next check-in" subtitle={dayToDate(assessment.nextDueDay)} />
          </List>
        </Section>
      ) : null}
    </Screen>
  );
}
