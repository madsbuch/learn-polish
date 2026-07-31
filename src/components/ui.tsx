import type { ReactNode } from "react";

import { canSpeak, speak } from "../lib/speech";

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

/** Speaker button. Renders nothing when the platform has no speech engine. */
export function SpeakButton({ text, label }: { text: string; label?: string }) {
  if (!canSpeak()) return null;
  return (
    <button
      className="icon-btn"
      aria-label={label ?? `Listen to ${text}`}
      onClick={(event) => {
        event.stopPropagation();
        speak(text);
      }}
    >
      🔊
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
