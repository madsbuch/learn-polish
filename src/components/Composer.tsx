import { useLayoutEffect, useRef } from "react";

import { IconButton } from "../design";
import { SendIcon } from "./icons";

/**
 * The box a chat message is written in, with its send button. Enter makes a
 * new line — on a phone keyboard that is what the return key is for; the
 * button sends. The box grows with what is written, up to a few lines.
 */
export function Composer({
  draft,
  setDraft,
  placeholder,
  disabled,
  canSend = true,
  onSend,
}: {
  draft: string;
  setDraft: (text: string) => void;
  placeholder: string;
  /** No writing at all (nothing to answer yet). */
  disabled?: boolean;
  /** Writing is fine but sending is not (a reply is still coming). */
  canSend?: boolean;
  onSend: (text: string) => void;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);

  useLayoutEffect(() => {
    const box = ref.current;
    if (!box) return;
    box.style.height = "auto";
    box.style.height = `${box.scrollHeight + box.offsetHeight - box.clientHeight}px`;
  }, [draft]);

  return (
    <div className="composer">
      <textarea
        ref={ref}
        rows={1}
        value={draft}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(event) => setDraft(event.target.value)}
      />
      <IconButton
        variant="primary"
        icon={<SendIcon />}
        label="Send"
        disabled={disabled || !canSend || draft.trim().length === 0}
        onClick={() => onSend(draft)}
      />
    </div>
  );
}
