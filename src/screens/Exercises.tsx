import { Logo } from "../components/icons";
import { TopicList } from "../components/TopicList";
import { Bar } from "../components/ui";
import type { SessionPlan, Stats, TopicProgress } from "../lib/types";

function greeting(): string {
  const hour = new Date().getHours();
  if (hour < 11) return "Dzień dobry";
  if (hour < 18) return "Cześć";
  return "Dobry wieczór";
}

/**
 * Everything to practise in one place: today's check-in on top, then your
 * programmes and the course. Progress and the word search open from here.
 */
export function Exercises({
  plan,
  stats,
  topics,
  focusTopicId,
  onStart,
  onOpenTopic,
  onOpenTutor,
  onOpenProgress,
  onOpenSearch,
  onBackToCourse,
}: {
  plan: SessionPlan;
  stats: Stats;
  topics: TopicProgress[];
  focusTopicId: number | null;
  onStart: () => void;
  onOpenTopic: (progress: TopicProgress) => void;
  onOpenTutor: () => void;
  onOpenProgress: () => void;
  onOpenSearch: () => void;
  /** Ends the daily focus on a programme. */
  onBackToCourse: () => void;
}) {
  const course = topics.filter((entry) => entry.topic.kind === "course");
  const programmes = topics.filter((entry) => entry.topic.kind === "programme");
  const courseSize = course.length;
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
            <h1 style={{ marginBottom: 0, fontSize: 23 }}>Exercises</h1>
          </div>
        </div>
        <div className="row" style={{ gap: 6 }}>
          <button className="chip crimson" onClick={onOpenProgress} aria-label="Your progress">
            🔥 {stats.currentStreak}
          </button>
          <button className="chip gold" onClick={onOpenProgress} aria-label="Your progress">
            Lv {stats.level}
          </button>
          <button className="icon-btn" onClick={onOpenSearch} aria-label="Search words and sentences">
            🔍
          </button>
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

      <button className="card row" style={{ width: "100%", textAlign: "left" }} onClick={onOpenProgress}>
        <span style={{ fontSize: 24 }}>📈</span>
        <span className="grow">
          <span style={{ display: "block", fontWeight: 700 }}>Your progress</span>
          <span className="small faint">
            {stats.wordsSeen} words met · {stats.wordsMastered} mastered · {stats.dueToday} due
          </span>
        </span>
        <span className="faint">›</span>
      </button>

      <h3 style={{ margin: "18px 0 8px" }}>Your programmes</h3>
      {programmes.length === 0 ? (
        <button className="card row" style={{ width: "100%", textAlign: "left" }} onClick={onOpenTutor}>
          <span style={{ fontSize: 22 }}>✨</span>
          <span className="grow small">
            None yet. Ask the tutor, e.g. <i>“Build me a programme for learning the colours”</i>.
          </span>
          <span className="faint">›</span>
        </button>
      ) : (
        <TopicList entries={programmes} focusTopicId={focusTopicId} onOpen={onOpenTopic} />
      )}

      <h3 style={{ margin: "18px 0 8px" }}>Course</h3>
      <TopicList entries={course} focusTopicId={focusTopicId} onOpen={onOpenTopic} />
    </div>
  );
}
