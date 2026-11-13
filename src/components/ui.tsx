import { useState } from "react";

import { IconButton } from "../design";
import { speak } from "../lib/speech";
import { MutedIcon, SpeakerIcon } from "./icons";

/**
 * Plays a line of Polish. Always shown — a button that silently vanishes is
 * more confusing than one that explains why it cannot play. Spins while the
 * first play of a line is being made, and says why on failure.
 */
export function SpeakButton({
  text,
  gender,
  label,
  size = "md",
  variant,
}: {
  text: string;
  /** "f" or "m" for dialogue lines; omitted elsewhere. */
  gender?: string;
  label?: string;
  size?: "sm" | "md";
  variant?: "tinted";
}) {
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  return (
    <IconButton
      icon={failure ? <MutedIcon /> : <SpeakerIcon />}
      label={failure ?? label ?? `Listen to ${text}`}
      size={size}
      variant={variant}
      loading={busy}
      failed={Boolean(failure)}
      onClick={async () => {
        setFailure(null);
        setBusy(true);
        try {
          await speak(text, gender);
        } catch (cause) {
          setFailure(cause instanceof Error ? cause.message : "Could not play audio.");
        } finally {
          setBusy(false);
        }
      }}
    />
  );
}
