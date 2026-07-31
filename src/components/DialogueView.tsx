import { useState } from "react";

import { SpeakButton } from "./ui";
import { speak } from "../lib/speech";
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
                <button
                  className="icon-btn"
                  style={{ width: 30, height: 30, fontSize: 14 }}
                  aria-label="Listen"
                  onClick={(event) => {
                    event.stopPropagation();
                    speak(line.pl);
                  }}
                >
                  🔊
                </button>
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
  return (
    <div className="row" style={{ marginBottom: 14 }}>
      <button className="btn ghost small grow" onClick={onToggleReveal}>
        {revealAll ? "Hide translations" : "Show translations"}
      </button>
      <SpeakButton
        text={lines.map((l) => l.pl).join(". ")}
        label="Listen to the whole conversation"
      />
    </div>
  );
}
