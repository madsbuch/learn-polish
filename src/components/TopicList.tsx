import { Bar } from "./ui";
import type { TopicProgress } from "../lib/types";

/** Course topics and programmes, each with how much of it has been met. */
export function TopicList({
  entries,
  focusTopicId,
  onOpen,
}: {
  entries: TopicProgress[];
  focusTopicId: number | null;
  onOpen: (progress: TopicProgress) => void;
}) {
  return (
    <div className="card flush">
      {entries.map((entry) => (
        <button key={entry.topic.id} className="list-row" onClick={() => onOpen(entry)}>
          <span
            className="avatar"
            style={{ background: entry.seenWords > 0 ? "var(--brand)" : "var(--line)" }}
          >
            {entry.topic.kind === "course" ? entry.topic.ord : "✦"}
          </span>
          <span className="grow">
            <span className="pl">{entry.topic.title}</span>
            <span className="tr">
              {entry.topic.titlePl} · {entry.seenWords}/{entry.totalWords} words
              {entry.topic.id === focusTopicId ? " · daily focus" : ""}
            </span>
            <span style={{ display: "block", marginTop: 7 }}>
              <Bar value={entry.seenWords} max={entry.totalWords} />
            </span>
          </span>
          <span className="faint">›</span>
        </button>
      ))}
    </div>
  );
}
