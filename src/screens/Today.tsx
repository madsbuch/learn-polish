import { Logo } from "../components/icons";
import { Bar, Stat } from "../components/ui";
import type { SessionPlan, Stats } from "../lib/types";

function greeting(): string {
  const hour = new Date().getHours();
  if (hour < 11) return "Dzień dobry";
  if (hour < 18) return "Cześć";
  return "Dobry wieczór";
}

export function Today({
  plan,
  stats,
  courseSize,
  onStart,
  onOpenTutor,
  onBackToCourse,
}: {
  plan: SessionPlan;
  stats: Stats;
  /** How many built-in topics the course rotates through. */
  courseSize: number;
  onStart: () => void;
  onOpenTutor: () => void;
  /** Ends the daily focus on a programme. */
  onBackToCourse: () => void;
}) {
  const programme = plan.topic.kind === "programme";
  const goalPct = Math.min(100, Math.round((stats.dayXp / Math.max(1, stats.dailyGoal)) * 100));
  const goalMet = stats.dayXp >= stats.dailyGoal;
  const items =
    plan.newWords + plan.reviewWords + plan.newSentences + plan.reviewSentences;

  return (
    <div className="scroll">
      <div className="row between" style={{ marginBottom: 16 }}>
        <div className="row" style={{ gap: 11 }}>
          <Logo size={40} />
          <div>
            <div className="eyebrow" style={{ marginBottom: 1 }}>
              {greeting()}
            </div>
            <h1 style={{ marginBottom: 0, fontSize: 23 }}>Today's practice</h1>
          </div>
        </div>
        <div className="row" style={{ gap: 6 }}>
          <span className="chip crimson">🔥 {stats.currentStreak}</span>
          <span className="chip gold">Lv {stats.level}</span>
        </div>
      </div>

      <div className="hero">
        <div className="row">
          <div
            className="ring"
            style={{ ["--value" as string]: String(goalPct) }}
            role="img"
            aria-label={`${goalPct}% of today's goal`}
          >
            <div>
              <div style={{ fontSize: 20, fontWeight: 780 }}>{stats.dayXp}</div>
              <div style={{ fontSize: 10, opacity: 0.75 }}>/ {stats.dailyGoal} XP</div>
            </div>
          </div>
          <div className="grow">
            <div className="eyebrow">
              {programme ? "Your programme" : `Course · topic ${plan.topic.ord} of ${courseSize}`}
            </div>
            <div style={{ fontSize: 19, fontWeight: 750, lineHeight: 1.25 }}>
              {plan.topic.titlePl}
            </div>
            <div style={{ fontSize: 13.5, opacity: 0.8, marginTop: 2 }}>
              {plan.topic.title}
            </div>
          </div>
        </div>

        {plan.topic.summary ? (
          <p style={{ margin: "16px 0 14px", fontSize: 13.5 }}>{plan.topic.summary}</p>
        ) : (
          <div style={{ height: 16 }} />
        )}

        <div className="row wrap" style={{ gap: 6, marginBottom: 16 }}>
          {plan.dialogue.length > 0 ? <span className="chip">💬 1 conversation</span> : null}
          <span className="chip">🆕 {plan.newWords} new words</span>
          <span className="chip">🔁 {plan.reviewWords} to review</span>
          <span className="chip">📝 {plan.newSentences + plan.reviewSentences} sentences</span>
        </div>

        <button className="btn on-hero block" onClick={onStart}>
          {stats.completedToday ? "Practise again" : "Start today's check-in"}
          {items > 0 ? ` · ~${Math.max(3, Math.round(items * 0.4))} min` : ""}
        </button>

        {stats.completedToday ? (
          <div className="center tiny" style={{ marginTop: 10, opacity: 0.85 }}>
            ✓ Done for today — your streak is safe
          </div>
        ) : null}

        {programme ? (
          <button
            className="btn ghost small block"
            style={{ marginTop: 10, color: "inherit" }}
            onClick={onBackToCourse}
          >
            Back to the course
          </button>
        ) : null}
      </div>

      {!goalMet && stats.dayXp > 0 ? (
        <div className="card">
          <div className="row between" style={{ marginBottom: 8 }}>
            <h3 style={{ margin: 0 }}>Daily goal</h3>
            <span className="small faint">
              {stats.dailyGoal - stats.dayXp} XP to go
            </span>
          </div>
          <Bar value={stats.dayXp} max={stats.dailyGoal} tone="gold" />
        </div>
      ) : null}

      <div className="stat-grid" style={{ marginBottom: 14 }}>
        <Stat value={stats.wordsSeen} label="words met" />
        <Stat value={stats.wordsMastered} label="mastered" />
        <Stat value={stats.dueToday} label="due today" />
        <Stat value={`${Math.round(stats.accuracy * 100)}%`} label="accuracy" />
      </div>

      <div className="card">
        <h3>Level {stats.level}</h3>
        <Bar value={stats.xpIntoLevel} max={stats.xpForNextLevel} />
        <div className="row between tiny faint" style={{ marginTop: 8 }}>
          <span>{stats.totalXp} XP total</span>
          <span>{stats.xpForNextLevel - stats.xpIntoLevel} XP to level {stats.level + 1}</span>
        </div>
      </div>

      <button className="card row" style={{ width: "100%", textAlign: "left" }} onClick={onOpenTutor}>
        <span style={{ fontSize: 26 }}>✨</span>
        <span className="grow">
          <span style={{ display: "block", fontWeight: 700 }}>Ask the tutor</span>
          <span className="small faint">
            Get a practice programme on any theme, or ask anything about Polish
          </span>
        </span>
        <span className="faint">›</span>
      </button>
    </div>
  );
}
