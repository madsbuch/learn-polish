import { useState } from "react";

import { SpeakButton } from "./ui";
import { speakSequence, stop } from "../lib/speech";
import type { DialogueLine } from "../lib/types";

/**
 * Renders a conversation as chat bubbles. Translations start hidden so the
 * learner reads Polish first; tapping a bubble reveals its meaning.
 */
export function DialogueView({
  lines,
  revealAll,
  onExplain,
}: {
  lines: DialogueLine[];
  revealAll?: boolean;
  onExplain?: (line: DialogueLine) => void;
}) {
  const [revealed, setRevealed] = useState<Set<number>>(new Set());

  // The first speaker sits on the left, everyone else on the right.
  const firstSpeaker = lines[0]?.speaker;

  return (
    <div>
      {lines.map((line) => {
        const isOpen = revealAll || revealed.has(line.ord);
        return (
          <div
            key={line.ord}
            className={`line${line.speaker === firstSpeaker ? "" : " right"}`}
          >
            <div className="avatar">{line.speaker.slice(0, 2)}</div>
            <div
              className="bubble selectable"
              onClick={() =>
                setRevealed((current) => {
                  const next = new Set(current);
                  if (next.has(line.ord)) next.delete(line.ord);
                  else next.add(line.ord);
                  return next;
                })
              }
            >
              <div className="row" style={{ alignItems: "flex-start" }}>
                <div className="grow">
                  <div className="pl">{line.pl}</div>
                  {isOpen ? <div className="tr fade-in">{line.meaning}</div> : null}
                </div>
                <SpeakButton
                  text={line.pl}
                  gender={line.gender}
                  size={30}
                  label={`Listen to ${line.speaker}`}
                />
              </div>
              {isOpen && onExplain ? (
                <button
                  className="btn ghost small"
                  style={{ marginTop: 10 }}
                  onClick={(event) => {
                    event.stopPropagation();
                    onExplain(line);
                  }}
                >
                  ✨ Explain this line
                </button>
              ) : null}
            </div>
          </div>
        );
      })}
    </div>
  );
}

export function DialogueToolbar({
  lines,
  revealAll,
  onToggleReveal,
}: {
  lines: DialogueLine[];
  revealAll: boolean;
  onToggleReveal: () => void;
}) {
  const [playing, setPlaying] = useState(false);

  return (
    <div className="row" style={{ marginBottom: 14 }}>
      <button className="btn ghost small grow" onClick={onToggleReveal}>
        {revealAll ? "Hide translations" : "Show translations"}
      </button>
      <button
        className="icon-btn"
        aria-label={playing ? "Stop" : "Play the whole conversation"}
        onClick={async () => {
          if (playing) {
            stop();
            setPlaying(false);
            return;
          }
          setPlaying(true);
          try {
            await speakSequence(
              lines.map((line) => ({ text: line.pl, gender: line.gender })),
            );
          } catch {
            /* the per-line buttons report why */
          } finally {
            setPlaying(false);
          }
        }}
      >
        {playing ? "⏹" : "▶︎"}
      </button>
    </div>
  );
}
