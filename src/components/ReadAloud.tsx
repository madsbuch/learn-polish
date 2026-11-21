import { Inline } from "../design";
import { SlowButton, SpeakButton } from "./ui";

const SAY = /\[\[say:\s*(?:([fm]):\s*)?([^\]\n]+?)\s*\]\]/g;

/** The message with its `[[say: …]]` players taken out, the words kept. */
export function unsay(text: string): string {
  return text.replace(SAY, "$2");
}

/** What is read out: the message as plain text, without markdown marks. */
function spoken(text: string): string {
  return unsay(text)
    .replace(/[*_`#>]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

/** The voice the message asked for in its players, if any. */
function voiceOf(text: string): string | undefined {
  for (const match of text.matchAll(SAY)) if (match[1]) return match[1];
  return undefined;
}

/** One pair of buttons that reads a whole message, normally or slowly. */
export function ReadAloud({ text }: { text: string }) {
  const line = spoken(text);
  if (!line) return null;
  const gender = voiceOf(text);
  return (
    <Inline gap={1} className="read-aloud">
      <SpeakButton text={line} gender={gender} size="sm" label="Listen to the whole message" />
      <SlowButton text={line} gender={gender} size="sm" />
    </Inline>
  );
}
