import { useState } from "react";

import { Avatar, Button, IconButton, Text } from "../design";
import { speakSequence, stop } from "../lib/speech";
import type { DialogueLine } from "../lib/types";
import { PlayerIcon } from "./icons";
import { SpeakButton } from "./ui";

/**
 * A conversation as chat bubbles. Translations start hidden so the learner
 * reads Polish first; tapping a bubble reveals its meaning.
 */
export function DialogueView({ lines, revealAll }: { lines: DialogueLine[]; revealAll?: boolean }) {
  const [revealed, setRevealed] = useState<Set<number>>(new Set());
  // The first speaker sits on the left, everyone else on the right.
  const firstSpeaker = lines[0]?.speaker;

  return (
    <div className="dialogue">
      {lines.map((line) => {
        const isOpen = revealAll || revealed.has(line.ord);
        return (
          <div key={line.ord} className="line" data-side={line.speaker === firstSpeaker ? "left" : "right"}>
            <Avatar>{line.speaker.slice(0, 2)}</Avatar>
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
              <div className="grow">
                <Text weight="medium" as="div">
                  {line.pl}
                </Text>
                {isOpen ? (
                  <Text variant="small" tone="dim" as="div" className="fade-in">
                    {line.meaning}
                  </Text>
                ) : null}
              </div>
              <SpeakButton text={line.pl} gender={line.gender} size="sm" label={`Listen to ${line.speaker}`} />
            </div>
          </div>
        );
      })}
    </div>
  );
}

/** Show/hide every translation, and play the whole conversation. */
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
    <div className="inline gap-2">
      <div className="grow">
        <Button size="sm" block onClick={onToggleReveal}>
          {revealAll ? "Hide translations" : "Show translations"}
        </Button>
      </div>
      <IconButton
        variant="tinted"
        size="sm"
        icon={playing ? <PlayerIcon.pause size={16} /> : <PlayerIcon.play size={16} />}
        label={playing ? "Stop" : "Play the whole conversation"}
        onClick={async () => {
          if (playing) {
            stop();
            setPlaying(false);
            return;
          }
          setPlaying(true);
          try {
            await speakSequence(lines.map((line) => ({ text: line.pl, gender: line.gender })));
          } catch {
            /* the per-line buttons report why */
          } finally {
            setPlaying(false);
          }
        }}
      />
    </div>
  );
}
