//! XP, levels, streaks and achievements — the parts that make the daily
//! check-in worth showing up for.

use rusqlite::Connection;

use crate::db;
use crate::error::Result;
use crate::models::{Achievement, DayStat, Stats, TopicProgress};
use crate::session;

pub const DEFAULT_DAILY_GOAL: i64 = 60;
/// Correct reps after which a word counts as "mastered" on the progress screen.
const MASTERY_REPS: i64 = 4;
const HISTORY_DAYS: i64 = 120;

/// Cumulative XP required to reach a level: 0, 100, 300, 600, 1000, …
fn xp_for_level(level: i64) -> i64 {
    50 * level * (level - 1)
}

pub fn level_from_xp(xp: i64) -> i64 {
    let mut level = 1;
    while xp_for_level(level + 1) <= xp {
        level += 1;
    }
    level
}

/// XP for one answer. Harder exercise types pay more, and only correct answers
/// score — otherwise the goal ring would fill by guessing.
pub fn xp_for_answer(kind: &str, correct: bool, is_new: bool) -> i64 {
    if !correct {
        return 0;
    }
    let base = match kind {
        "intro" | "sentenceIntro" => 2,
        "recognise" => 6,
        "produce" => 8,
        "dialogueGap" => 10,
        "wordOrder" => 12,
        "type" => 14,
        "memory" => 12,
        "gapFill" => 16,
        _ => 5,
    };
    base + if is_new { 2 } else { 0 }
}

pub fn total_xp(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COALESCE(SUM(xp), 0) FROM day_log", [], |r| r.get(0))?)
}

pub fn record_answer(conn: &Connection, day: i64, xp: i64, correct: bool) -> Result<()> {
    conn.execute(
        "INSERT INTO day_log (day, xp, answered, correct) VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(day) DO UPDATE SET
             xp = xp + excluded.xp,
             answered = answered + 1,
             correct = correct + excluded.correct",
        rusqlite::params![day, xp, i64::from(correct)],
    )?;
    Ok(())
}

/// Marks the day finished with the topic that was practised. Finishing the
/// course topic due today advances the rotation, so tomorrow brings the next
/// conversation; finishing a focused programme whose words have all been met
/// hands the daily check-in back to the course. Calling it twice on one day is
/// a no-op.
pub fn complete_day(conn: &Connection, day: i64, topic_id: i64) -> Result<bool> {
    let already: i64 = conn
        .query_row("SELECT completed FROM day_log WHERE day = ?1", [day], |r| {
            r.get(0)
        })
        .unwrap_or(0);
    if already == 1 {
        return Ok(false);
    }

    conn.execute(
        "INSERT INTO day_log (day, completed, topic_id) VALUES (?1, 1, ?2)
         ON CONFLICT(day) DO UPDATE SET completed = 1, topic_id = excluded.topic_id",
        rusqlite::params![day, topic_id],
    )?;

    match session::focus_topic_id(conn) {
        Some(focus) if focus == topic_id => {
            let unseen: i64 = conn.query_row(
                "SELECT COUNT(*) FROM words w WHERE w.topic_id = ?1
                 AND NOT EXISTS (SELECT 1 FROM reviews r WHERE r.kind = 'word' AND r.item_id = w.id)",
                [topic_id],
                |r| r.get(0),
            )?;
            if unseen == 0 {
                conn.execute("DELETE FROM settings WHERE key = 'focus_topic_id'", [])?;
            }
        }
        Some(_) => {}
        None => {
            if session::rotation_topic_id(conn)? == topic_id {
                let next = db::get_setting_i64(conn, "lesson_index", 0) + 1;
                db::set_setting(conn, "lesson_index", &next.to_string())?;
            }
        }
    }
    Ok(true)
}

pub fn streaks(conn: &Connection, today: i64) -> Result<(i64, i64)> {
    let mut stmt =
        conn.prepare("SELECT day FROM day_log WHERE completed = 1 ORDER BY day")?;
    let days: Vec<i64> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut longest = 0;
    let mut run = 0;
    let mut previous: Option<i64> = None;
    for day in &days {
        run = match previous {
            Some(p) if *day == p + 1 => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        previous = Some(*day);
    }

    // The current streak survives until the end of tomorrow: finishing
    // yesterday and not yet having practised today should still read as a
    // live streak.
    let set: std::collections::HashSet<i64> = days.into_iter().collect();
    let mut cursor = if set.contains(&today) {
        today
    } else if set.contains(&(today - 1)) {
        today - 1
    } else {
        return Ok((0, longest));
    };
    let mut current = 0;
    while set.contains(&cursor) {
        current += 1;
        cursor -= 1;
    }
    Ok((current, longest))
}

struct Counts {
    words_total: i64,
    words_seen: i64,
    words_mastered: i64,
    sentences_total: i64,
    sentences_seen: i64,
    dialogues_total: i64,
    dialogues_seen: i64,
    due_today: i64,
}

fn counts(conn: &Connection, today: i64) -> Result<Counts> {
    let one = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> Result<i64> {
        Ok(conn.query_row(sql, params, |r| r.get(0))?)
    };
    Ok(Counts {
        words_total: one("SELECT COUNT(*) FROM words", &[])?,
        words_seen: one("SELECT COUNT(*) FROM reviews WHERE kind = 'word'", &[])?,
        words_mastered: one(
            "SELECT COUNT(*) FROM reviews WHERE kind = 'word' AND reps >= ?1",
            &[&MASTERY_REPS],
        )?,
        sentences_total: one("SELECT COUNT(*) FROM sentences", &[])?,
        sentences_seen: one("SELECT COUNT(*) FROM reviews WHERE kind = 'sentence'", &[])?,
        dialogues_total: one("SELECT COUNT(DISTINCT topic_id) FROM conversation_lines", &[])?,
        dialogues_seen: one(
            "SELECT COUNT(DISTINCT d.topic_id) FROM day_log d
             WHERE d.completed = 1
               AND EXISTS (SELECT 1 FROM conversation_lines c WHERE c.topic_id = d.topic_id)",
            &[],
        )?,
        due_today: one(
            "SELECT COUNT(*) FROM reviews WHERE due_day <= ?1",
            &[&today],
        )?,
    })
}

pub fn collect(conn: &Connection) -> Result<Stats> {
    let today = db::today(conn);
    let c = counts(conn, today)?;
    let total = total_xp(conn)?;
    let level = level_from_xp(total);
    let (current_streak, longest_streak) = streaks(conn, today)?;

    let (day_xp, completed_today): (i64, bool) = conn
        .query_row(
            "SELECT xp, completed FROM day_log WHERE day = ?1",
            [today],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)? == 1)),
        )
        .unwrap_or((0, false));

    let (answered, correct): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(answered), 0), COALESCE(SUM(correct), 0) FROM day_log",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    let mut stmt = conn.prepare(
        "SELECT day, xp, answered, correct, completed FROM day_log
         WHERE day > ?1 ORDER BY day",
    )?;
    let history = stmt
        .query_map([today - HISTORY_DAYS], |r| {
            Ok(DayStat {
                day: r.get(0)?,
                xp: r.get(1)?,
                answered: r.get(2)?,
                correct: r.get(3)?,
                completed: r.get::<_, i64>(4)? == 1,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    let achievements = evaluate_achievements(conn, &c, total, level, longest_streak)?;

    Ok(Stats {
        day: today,
        total_xp: total,
        day_xp,
        daily_goal: db::get_setting_i64(conn, "daily_goal", DEFAULT_DAILY_GOAL),
        level,
        xp_into_level: total - xp_for_level(level),
        xp_for_next_level: xp_for_level(level + 1) - xp_for_level(level),
        current_streak,
        longest_streak,
        words_seen: c.words_seen,
        words_mastered: c.words_mastered,
        words_total: c.words_total,
        sentences_seen: c.sentences_seen,
        sentences_total: c.sentences_total,
        dialogues_seen: c.dialogues_seen,
        dialogues_total: c.dialogues_total,
        due_today: c.due_today,
        accuracy: if answered > 0 {
            correct as f64 / answered as f64
        } else {
            0.0
        },
        completed_today,
        history,
        achievements,
    })
}

pub fn topic_progress(conn: &Connection) -> Result<Vec<TopicProgress>> {
    // The course first, in order, then programmes newest first.
    let mut stmt = conn.prepare(
        "SELECT id FROM topics
         ORDER BY kind = 'programme', CASE WHEN kind = 'course' THEN ord ELSE -created_at END, id DESC",
    )?;
    let ids: Vec<i64> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    ids.into_iter()
        .map(|id| {
            let topic = session::load_topic(conn, id)?;
            let one = |sql: &str| -> Result<i64> {
                Ok(conn.query_row(sql, [id], |r| r.get(0))?)
            };
            Ok(TopicProgress {
                topic,
                total_words: one("SELECT COUNT(*) FROM words WHERE topic_id = ?1")?,
                seen_words: one(
                    "SELECT COUNT(*) FROM words w JOIN reviews r
                     ON r.kind = 'word' AND r.item_id = w.id WHERE w.topic_id = ?1",
                )?,
                mastered_words: conn.query_row(
                    "SELECT COUNT(*) FROM words w JOIN reviews r
                     ON r.kind = 'word' AND r.item_id = w.id
                     WHERE w.topic_id = ?1 AND r.reps >= ?2",
                    rusqlite::params![id, MASTERY_REPS],
                    |r| r.get(0),
                )?,
                total_sentences: one("SELECT COUNT(*) FROM sentences WHERE topic_id = ?1")?,
                seen_sentences: one(
                    "SELECT COUNT(*) FROM sentences s JOIN reviews r
                     ON r.kind = 'sentence' AND r.item_id = s.id WHERE s.topic_id = ?1",
                )?,
            })
        })
        .collect()
}

struct Definition {
    code: &'static str,
    title: &'static str,
    description: &'static str,
    icon: &'static str,
    target: i64,
}

const DEFINITIONS: &[Definition] = &[
    Definition { code: "first_day", title: "Dzień dobry", description: "Finish your first daily check-in", icon: "🌅", target: 1 },
    Definition { code: "streak_3", title: "Three in a row", description: "Practise three days in a row", icon: "🔥", target: 3 },
    Definition { code: "streak_7", title: "Full week", description: "Practise seven days in a row", icon: "📅", target: 7 },
    Definition { code: "streak_30", title: "Miesiąc", description: "Practise thirty days in a row", icon: "🏅", target: 30 },
    Definition { code: "streak_100", title: "Sto dni", description: "Practise a hundred days in a row", icon: "💯", target: 100 },
    Definition { code: "words_50", title: "Fifty words", description: "Meet fifty Polish words", icon: "🌱", target: 50 },
    Definition { code: "words_250", title: "Getting by", description: "Meet 250 Polish words", icon: "🌿", target: 250 },
    Definition { code: "words_1000", title: "Halfway", description: "Meet a thousand Polish words", icon: "🌳", target: 1000 },
    Definition { code: "words_2000", title: "Whole vocabulary", description: "Meet every word in the app", icon: "🏆", target: 2000 },
    Definition { code: "mastered_100", title: "Solid hundred", description: "Master a hundred words", icon: "💎", target: 100 },
    Definition { code: "sentences_100", title: "Sentence builder", description: "Practise a hundred sentences", icon: "🧱", target: 100 },
    Definition { code: "all_dialogues", title: "Ten conversations", description: "Work through every conversation", icon: "💬", target: 10 },
    Definition { code: "level_5", title: "Level five", description: "Reach level five", icon: "⭐", target: 5 },
    Definition { code: "level_10", title: "Level ten", description: "Reach level ten", icon: "🌟", target: 10 },
    Definition { code: "xp_5000", title: "Five thousand", description: "Earn 5000 XP", icon: "⚡", target: 5000 },
];

fn progress_value(def: &Definition, c: &Counts, xp: i64, level: i64, longest_streak: i64) -> i64 {
    match def.code {
        "first_day" => i64::from(c.dialogues_seen > 0),
        code if code.starts_with("streak_") => longest_streak,
        code if code.starts_with("words_") => c.words_seen,
        "mastered_100" => c.words_mastered,
        "sentences_100" => c.sentences_seen,
        "all_dialogues" => c.dialogues_seen,
        code if code.starts_with("level_") => level,
        "xp_5000" => xp,
        _ => 0,
    }
}

fn evaluate_achievements(
    conn: &Connection,
    c: &Counts,
    xp: i64,
    level: i64,
    longest_streak: i64,
) -> Result<Vec<Achievement>> {
    let mut stmt = conn.prepare("SELECT code FROM achievements")?;
    let unlocked: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);

    let today = db::today(conn);
    let mut result = Vec::with_capacity(DEFINITIONS.len());

    for def in DEFINITIONS {
        let value = progress_value(def, c, xp, level, longest_streak);
        let earned = value >= def.target;
        if earned && !unlocked.contains(def.code) {
            conn.execute(
                "INSERT OR IGNORE INTO achievements (code, unlocked_day, unlocked_at)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![def.code, today, db::now_secs()],
            )?;
        }
        result.push(Achievement {
            code: def.code.to_string(),
            title: def.title.to_string(),
            description: def.description.to_string(),
            icon: def.icon.to_string(),
            unlocked: earned || unlocked.contains(def.code),
            progress: (value as f64 / def.target as f64).clamp(0.0, 1.0),
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_the_curve() {
        assert_eq!(level_from_xp(0), 1);
        assert_eq!(level_from_xp(99), 1);
        assert_eq!(level_from_xp(100), 2);
        assert_eq!(level_from_xp(299), 2);
        assert_eq!(level_from_xp(300), 3);
        assert_eq!(level_from_xp(1000), 5);
    }

    #[test]
    fn wrong_answers_earn_nothing() {
        assert_eq!(xp_for_answer("type", false, true), 0);
        assert!(xp_for_answer("type", true, false) > xp_for_answer("recognise", true, false));
    }
}
