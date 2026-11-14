import { useEffect, useRef, useState } from "react";

import { Banner, Inline, Sheet, Spinner, Stack, Text } from "../design";
import { api } from "../lib/api";
import { useAiStream } from "../lib/useAiStream";
import { Composer } from "./Composer";
import { Markdown } from "./Markdown";

interface Turn {
  role: "user" | "assistant";
  content: string;
}

/**
 * A short chat about a word or sentence just practised: how it is really
 * used, and the learner trying it out — not a dictionary entry. It opens
 * itself; nothing is saved when it closes.
 */
export function WordChat({
  itemKind,
  itemId,
  title,
  onClose,
}: {
  itemKind: string;
  itemId: number;
  /** The Polish, shown as the sheet's title. */
  title: string;
  onClose: () => void;
}) {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [draft, setDraft] = useState("");
  const stream = useAiStream();
  const endRef = useRef<HTMLDivElement>(null);

  async function exchange(history: Turn[]) {
    const reply = await stream.run((streamId) => api.aiExplainStream(itemKind, itemId, streamId, history));
    if (reply) {
      stream.reset();
      setTurns([...history, { role: "assistant", content: reply }]);
    }
    return Boolean(reply);
  }

  // The tutor opens the conversation.
  const opened = useRef(false);
  useEffect(() => {
    if (opened.current) return;
    opened.current = true;
    void exchange([]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [turns, stream.text]);

  async function send(text: string) {
    const message = text.trim();
    if (!message || stream.streaming) return;
    setDraft("");
    const history: Turn[] = [...turns, { role: "user", content: message }];
    setTurns(history);
    if (!(await exchange(history))) {
      setTurns(turns);
      setDraft(message);
    }
  }

  return (
    <Sheet title={title} onClose={onClose}>
      <Stack gap={3}>
        <div className="chat word-chat">
          {turns.map((turn, i) => (
            <div key={i} className="msg selectable" data-role={turn.role}>
              {turn.role === "assistant" ? <Markdown text={turn.content} /> : turn.content}
            </div>
          ))}
          {stream.streaming ? (
            <div className="msg" data-role="assistant">
              {stream.text ? (
                <div>
                  <Markdown text={stream.text} />
                  <span className="caret" />
                </div>
              ) : (
                <Inline gap={2}>
                  <Spinner />
                  <Text variant="small" tone="faint">
                    Thinking…
                  </Text>
                </Inline>
              )}
            </div>
          ) : null}
          <div ref={endRef} />
        </div>
        {stream.error ? <Banner tone="error">{stream.error}</Banner> : null}
        <Composer
          draft={draft}
          setDraft={setDraft}
          placeholder="Try it, or ask about it"
          canSend={!stream.streaming && turns.length > 0}
          onSend={(text) => void send(text)}
        />
      </Stack>
    </Sheet>
  );
}
