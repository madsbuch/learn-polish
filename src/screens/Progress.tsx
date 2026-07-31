import { Bar, Stat } from "../components/ui";
import type { Stats, TopicProgress } from "../lib/types";

/** 18 weeks of activity, oldest first, in Sunday-aligned columns. */
function Heatmap({ stats }: { stats: Stats }) {
  const byDay = new Map(stats.history.map((entry) => [entry.day, entry]));
  const cells = [];
  for (let offset = 125; offset >= 0; offset--) {
    const day = stats.day - offset;
    const entry = byDay.get(day);
    const level = !entry || entry.xp === 0 ? 0 : entry.xp >= stats.dailyGoal ? 3 : entry.xp >= stats.dailyGoal / 2 ? 2 : 1;
    cells.push(<i key={day} data-level={level} />);
  }
  return <div className="heat">{cells}</div>;
}

export function Progress({
  stats,
  topics,
  onOpenTopic,
}: {
  stats: Stats;
  topics: TopicProgress[];
  onOpenTopic: (progress: TopicProgress) => void;
}) {
  return (
    <div className="scroll">
      <h1>Progress</h1>
      <p className="small">Everything you have learned so far.</p>

      <div className="stat-grid" style={{ marginBottom: 14 }}>
        <Stat value={stats.currentStreak} label="day streak" />
        <Stat value={stats.longestStreak} label="best streak" />
        <Stat value={stats.totalXp} label="total XP" />
        <Stat value={stats.level} label="level" />
      </div>

      <div className="card">
        <h3>Last 18 weeks</h3>
        <Heatmap stats={stats} />
      </div>

      <div className="card">
        <h3>Vocabulary</h3>
        <div className="row between small muted" style={{ marginBottom: 6 }}>
          <span>{stats.wordsSeen} met</span>
          <span>{stats.wordsTotal} total</span>
        </div>
        <Bar value={stats.wordsSeen} max={stats.wordsTotal} />
        <div className="row between small muted" style={{ margin: "14px 0 6px" }}>
          <span>{stats.wordsMastered} mastered</span>
          <span>{Math.round((stats.wordsMastered / Math.max(1, stats.wordsTotal)) * 100)}%</span>
        </div>
        <Bar value={stats.wordsMastered} max={stats.wordsTotal} tone="mint" />
        <div className="row between small muted" style={{ margin: "14px 0 6px" }}>
          <span>{stats.sentencesSeen} sentences practised</span>
          <span>{stats.sentencesTotal} total</span>
        </div>
        <Bar value={stats.sentencesSeen} max={stats.sentencesTotal} tone="gold" />
      </div>

      <h2 style={{ marginTop: 22 }}>Conversations</h2>
      <div className="card flush">
        {topics.map((entry) => {
          const pct = Math.round((entry.seenWords / Math.max(1, entry.totalWords)) * 100);
          return (
            <button
              key={entry.topic.id}
              className="list-row"
              onClick={() => onOpenTopic(entry)}
            >
              <span
                className="avatar"
                style={{ background: pct > 0 ? "var(--brand)" : "var(--line)" }}
              >
                {entry.topic.ord}
              </span>
              <span className="grow">
                <span className="pl">{entry.topic.titlePl}</span>
                <span className="tr">{entry.topic.title}</span>
                <span style={{ display: "block", marginTop: 7 }}>
                  <Bar value={entry.seenWords} max={entry.totalWords} />
                </span>
              </span>
              <span className="small faint" style={{ flex: "none" }}>
                {pct}%
              </span>
            </button>
          );
        })}
      </div>

      <h2 style={{ marginTop: 22 }}>Achievements</h2>
      <div className="badge-grid">
        {stats.achievements.map((achievement) => (
          <div
            key={achievement.code}
            className={`badge${achievement.unlocked ? "" : " locked"}`}
            title={achievement.description}
          >
            <span className="glyph">{achievement.icon}</span>
            <b>{achievement.title}</b>
            {!achievement.unlocked ? (
              <span style={{ display: "block", marginTop: 6 }}>
                <Bar value={achievement.progress} max={1} />
              </span>
            ) : null}
          </div>
        ))}
      </div>
    </div>
  );
}
