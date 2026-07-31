import { Stat } from "../components/ui";
import type { SessionResult } from "../lib/types";

export function SessionSummary({
  result,
  sessionXp,
  onDone,
}: {
  result: SessionResult;
  sessionXp: number;
  onDone: () => void;
}) {
  const { stats, newlyUnlocked } = result;
  const goalMet = stats.dayXp >= stats.dailyGoal;

  return (
    <div className="scroll no-nav">
      <div className="center" style={{ padding: "24px 0 8px" }}>
        <div className="pop" style={{ fontSize: 62 }}>
          {goalMet ? "🎉" : "👏"}
        </div>
        <h1>{goalMet ? "Goal reached!" : "Nice work"}</h1>
        <p>
          {result.firstCompletionToday
            ? "Today's conversation is done. Tomorrow brings the next one."
            : "Extra practice always counts."}
        </p>
      </div>

      <div className="hero center">
        <div style={{ fontSize: 42, fontWeight: 800, letterSpacing: "-0.03em" }}>
          +{sessionXp}
        </div>
        <div style={{ opacity: 0.8, fontWeight: 650 }}>XP this session</div>
        <div className="row" style={{ justifyContent: "center", gap: 8, marginTop: 16 }}>
          <span className="chip">🔥 {stats.currentStreak} day streak</span>
          <span className="chip">Level {stats.level}</span>
        </div>
      </div>

      <div className="stat-grid" style={{ marginBottom: 14 }}>
        <Stat value={stats.dayXp} label="XP today" />
        <Stat value={stats.wordsSeen} label="words met" />
        <Stat value={`${Math.round(stats.accuracy * 100)}%`} label="accuracy" />
      </div>

      {newlyUnlocked.length > 0 ? (
        <div className="card">
          <h3>New achievement{newlyUnlocked.length > 1 ? "s" : ""}</h3>
          <div className="stack">
            {newlyUnlocked.map((achievement) => (
              <div key={achievement.code} className="row pop">
                <span style={{ fontSize: 30 }}>{achievement.icon}</span>
                <span className="grow">
                  <span style={{ display: "block", fontWeight: 700 }}>
                    {achievement.title}
                  </span>
                  <span className="small faint">{achievement.description}</span>
                </span>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      <button className="btn primary block" onClick={onDone}>
        Done
      </button>
    </div>
  );
}
