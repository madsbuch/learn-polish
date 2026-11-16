import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";

import { Avatar, Button, IconButton, Text } from "../design";
import {
  conversationKey,
  currentConversation,
  pauseConversation,
  playConversation,
  resumeConversation,
  stopConversation,
  subscribeConversation,
} from "../lib/speech";
import type { DialogueLine } from "../lib/types";
import { CloseIcon, PlayerIcon } from "./icons";
import { SlowButton, SpeakButton } from "./ui";
import { WordChat } from "./WordChat";

function toSpeech(lines: DialogueLine[]) {
  return lines.map((line) => ({ text: line.pl, gender: line.gender }));
}

/** The conversation being read out, if it is this one. */
function useThisConversation(lines: DialogueLine[]) {
  const conversation = useSyncExternalStore(subscribeConversation, currentConversation);
  const key = conversationKey(toSpeech(lines));
  return conversation?.key === key ? conversation : null;
}

/**
 * A conversation as chat bubbles. Translations start hidden so the learner
 * reads Polish first; tapping a bubble reveals its meaning.
 */
export function DialogueView({ lines, revealAll }: { lines: DialogueLine[]; revealAll?: boolean }) {
  const [revealed, setRevealed] = useState<Set<number>>(new Set());
  // A line being talked about with the tutor ("why is it said like this?").
  const [talking, setTalking] = useState<DialogueLine | null>(null);
  // The first speaker sits on the left, everyone else on the right.
  const firstSpeaker = lines[0]?.speaker;
  // While the conversation is read out, its line now is marked and kept in view.
  const conversation = useThisConversation(lines);
  const speaking = conversation ? conversation.index : -1;
  const lineRefs = useRef<(HTMLDivElement | null)[]>([]);
  useEffect(() => {
    const line = lineRefs.current[speaking];
    if (speaking < 0 || !conversation?.playing || !line) return;
    // Follow along only while the conversation is on screen: scrolled away
    // to read something else, the page stays where it is.
    const box = line.closest(".dialogue")?.getBoundingClientRect();
    if (box && box.bottom > 0 && box.top < window.innerHeight) {
      line.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
  }, [speaking, conversation?.playing]);

  return (
    <div className="dialogue">
      {lines.map((line, i) => {
        const isOpen = revealAll || revealed.has(line.ord);
        return (
          <div
            key={line.ord}
            ref={(element) => {
              lineRefs.current[i] = element;
            }}
            className="line"
            data-side={line.speaker === firstSpeaker ? "left" : "right"}
            data-speaking={i === speaking || undefined}
          >
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
                  <div className="fade-in">
                    <Text variant="small" tone="dim" as="div">
                      {line.meaning}
                    </Text>
                    <button
                      className="line-talk"
                      onClick={(event) => {
                        event.stopPropagation();
                        setTalking(line);
                      }}
                    >
                      ✨ Talk about it
                    </button>
                  </div>
                ) : null}
              </div>
              <span className="line-audio">
                <SpeakButton text={line.pl} gender={line.gender} size="sm" label={`Listen to ${line.speaker}`} />
                <SlowButton text={line.pl} gender={line.gender} size="sm" />
              </span>
            </div>
          </div>
        );
      })}
      {talking ? (
        <WordChat
          itemKind="line"
          itemId={0}
          title={talking.pl}
          meaning={talking.meaning}
          onClose={() => setTalking(null)}
        />
      ) : null}
    </div>
  );
}

/**
 * Show/hide every translation, and play the whole conversation. While it
 * plays, a control floats at the foot of the screen — the line it is on,
 * pause, resume, stop — however far the page is scrolled; leaving the page
 * stops it.
 */
export function DialogueToolbar({
  lines,
  revealAll,
  onToggleReveal,
}: {
  lines: DialogueLine[];
  revealAll: boolean;
  onToggleReveal: () => void;
}) {
  const conversation = useThisConversation(lines);
  const active = conversation !== null;

  // Leaving the page (or this exercise) ends the conversation.
  const key = conversationKey(toSpeech(lines));
  useEffect(
    () => () => {
      if (currentConversation()?.key === key) stopConversation();
    },
    [key],
  );

  return (
    <div className="inline gap-2">
      <div className="grow">
        <Button size="sm" wide onClick={onToggleReveal}>
          {revealAll ? "Hide translations" : "Show translations"}
        </Button>
      </div>
      <IconButton
        variant="tinted"
        size="sm"
        icon={active ? <CloseIcon /> : <PlayerIcon.play size={16} />}
        label={active ? "Stop" : "Play the whole conversation"}
        onClick={() => {
          if (active) stopConversation();
          else void playConversation(toSpeech(lines)).catch(() => stopConversation());
        }}
      />
      {conversation
        ? createPortal(
            <div className="conversation-control fade-in" role="group" aria-label="The conversation">
              <Text variant="small" weight="medium">
                {conversation.playing ? "Playing" : "Paused"} · line {conversation.index + 1} of{" "}
                {conversation.lines.length}
              </Text>
              <IconButton
                variant="primary"
                icon={conversation.playing ? <PlayerIcon.pause size={18} /> : <PlayerIcon.play size={18} />}
                label={conversation.playing ? "Pause" : "Resume"}
                onClick={conversation.playing ? pauseConversation : resumeConversation}
              />
              <IconButton icon={<CloseIcon />} label="Stop" onClick={stopConversation} />
            </div>,
            document.body,
          )
        : null}
    </div>
  );
}
