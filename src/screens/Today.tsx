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
  onStart,
  onOpenTutor,
}: {
  plan: SessionPlan;
  stats: Stats;
  onStart: () => void;
  onOpenTutor: () => void;
}) {
  const goalPct = Math.min(100, Math.round((stats.dayXp / Math.max(1, stats.dailyGoal)) * 100));
  const goalMet = stats.dayXp >= stats.dailyGoal;
  const items =
    plan.newWords + plan.reviewWords + plan.newSentences + plan.reviewSentences;

  return (
    <div className="scroll">
      <div className="row between" style={{ marginBottom: 14 }}>
        <div>
          <div className="eyebrow" style={{ marginBottom: 2 }}>
            {greeting()}
          </div>
          <h1 style={{ marginBottom: 0 }}>Today's practice</h1>
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
            <div className="eyebrow" style={{ color: "rgba(255,255,255,0.65)" }}>
              Conversation {plan.topic.ord} of {stats.dialoguesTotal}
            </div>
            <div style={{ fontSize: 19, fontWeight: 750, lineHeight: 1.25 }}>
              {plan.topic.titlePl}
            </div>
            <div style={{ fontSize: 13.5, opacity: 0.8, marginTop: 2 }}>
              {plan.topic.title}
            </div>
          </div>
        </div>

        <p style={{ color: "rgba(255,255,255,0.82)", margin: "16px 0 14px", fontSize: 13.5 }}>
          {plan.topic.summary}
        </p>

        <div className="row wrap" style={{ gap: 6, marginBottom: 16 }}>
          <span className="chip">💬 1 conversation</span>
          <span className="chip">🆕 {plan.newWords} new words</span>
          <span className="chip">🔁 {plan.reviewWords} to review</span>
          <span className="chip">📝 {plan.newSentences + plan.reviewSentences} sentences</span>
        </div>

        <button className="btn primary block" onClick={onStart}>
          {stats.completedToday ? "Practise again" : "Start today's check-in"}
          {items > 0 ? ` · ~${Math.max(3, Math.round(items * 0.4))} min` : ""}
        </button>

        {stats.completedToday ? (
          <div className="center tiny" style={{ marginTop: 10, opacity: 0.85 }}>
            ✓ Done for today — your streak is safe
          </div>
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
          <span style={{ display: "block", fontWeight: 700 }}>Talk to the AI tutor</span>
          <span className="small faint">
            Practise {plan.topic.title.toLowerCase()} in a free conversation
          </span>
        </span>
        <span className="faint">›</span>
      </button>
    </div>
  );
}
