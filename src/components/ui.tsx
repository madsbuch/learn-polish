import { useState, type ReactNode } from "react";

import { speak } from "../lib/speech";

export function Banner({ kind, children }: { kind: "error" | "info"; children: ReactNode }) {
  return <div className={`banner ${kind}`}>{children}</div>;
}

export function Spinner() {
  return <span className="spinner" />;
}

export function Loading({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="center-screen">
      <div className="stack" style={{ alignItems: "center" }}>
        <Spinner />
        <span className="faint small">{label}</span>
      </div>
    </div>
  );
}

export function Bar({
  value,
  max,
  tone,
}: {
  value: number;
  max: number;
  tone?: "gold" | "mint";
}) {
  const pct = max > 0 ? Math.min(100, (value / max) * 100) : 0;
  return (
    <div className={`bar${tone ? ` ${tone}` : ""}`}>
      <span style={{ width: `${pct}%` }} />
    </div>
  );
}

/**
 * Speaker button. Always rendered — a button that silently vanishes is more
 * confusing than one that explains why it cannot play. Shows a spinner while
 * the first play of a line is being synthesised, and surfaces the reason on
 * failure instead of doing nothing.
 */
export function SpeakButton({
  text,
  label,
  size,
}: {
  text: string;
  label?: string;
  size?: number;
}) {
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const dimension = size ?? 38;

  return (
    <button
      className="icon-btn"
      style={size ? { width: size, height: size, fontSize: size * 0.42 } : undefined}
      aria-label={failure ?? label ?? `Listen to ${text}`}
      title={failure ?? undefined}
      data-failed={failure ? "true" : undefined}
      onClick={async (event) => {
        event.stopPropagation();
        if (busy) return;
        setFailure(null);
        setBusy(true);
        try {
          await speak(text);
        } catch (cause) {
          setFailure(cause instanceof Error ? cause.message : "Could not play audio.");
        } finally {
          setBusy(false);
        }
      }}
    >
      {busy ? (
        <span
          className="spinner"
          style={{ width: dimension * 0.4, height: dimension * 0.4 }}
        />
      ) : failure ? (
        "🔇"
      ) : (
        "🔊"
      )}
    </button>
  );
}

export function Stat({ value, label }: { value: ReactNode; label: string }) {
  return (
    <div className="stat">
      <b>{value}</b>
      <span>{label}</span>
    </div>
  );
}
