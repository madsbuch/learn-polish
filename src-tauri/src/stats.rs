//! XP, levels, streaks and achievements — the parts that make the daily
//! check-in worth showing up for.

use diesel::prelude::*;
use diesel::upsert::excluded;

use crate::db::{self, Conn};
use crate::schema::{achievements, blocks, course_blocks, course_sentences, course_words, day_log, reviews, sentences, topics, words};
use crate::error::Result;
use crate::models::{Achievement, DayStat, Stats, TopicProgress};
use crate::session;

/// About ten minutes: a drill and a check-in.
pub const DEFAULT_DAILY_GOAL: i64 = 150;
/// Streak freezes kept at most; one is earned with every seven-day run.
pub const MAX_FREEZES: i64 = 2;
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

/// XP for a check-in, paid by effort: a check-in takes as long as several
/// drills and is the deeper work, so it pays like it. A writing block adds
/// a bonus per goal got across. Failing earns none.
pub fn xp_for_checkin(complexity: &str, result: &str, writing_goals: usize) -> i64 {
    let base = match complexity {
        "low" => 30,
        "medium" => 80,
        _ => 160,
    } + 10 * writing_goals as i64;
    match result {
        "pass" => base,
        "partial" => base / 2,
        _ => 0,
    }
}

/// XP that is not an answer (a check-in's verdict): it counts toward the day
/// and the level, not toward accuracy.
pub fn add_xp(conn: &mut Conn, day: i64, xp: i64) -> Result<()> {
    diesel::insert_into(day_log::table)
        .values((day_log::day.eq(day), day_log::xp.eq(xp)))
        .on_conflict(day_log::day)
        .do_update()
        .set(day_log::xp.eq(day_log::xp + excluded(day_log::xp)))
        .execute(conn)?;
    Ok(())
}

pub fn total_xp(conn: &mut Conn) -> Result<i64> {
    Ok(day_log::table.select(day_log::xp).load::<i64>(conn)?.into_iter().sum())
}

pub fn record_answer(conn: &mut Conn, day: i64, xp: i64, correct: bool) -> Result<()> {
    diesel::insert_into(day_log::table)
        .values((
            day_log::day.eq(day),
            day_log::xp.eq(xp),
            day_log::answered.eq(1),
            day_log::correct.eq(i64::from(correct)),
        ))
        .on_conflict(day_log::day)
        .do_update()
        .set((
            day_log::xp.eq(day_log::xp + excluded(day_log::xp)),
            day_log::answered.eq(day_log::answered + 1),
            day_log::correct.eq(day_log::correct + excluded(day_log::correct)),
        ))
        .execute(conn)?;
    Ok(())
}

/// Whether a day is done: the daily goal reached, or — having practised —
/// nothing left due. One definition for the goal ring, the streak and the
/// reminder.
pub fn day_is_done(conn: &mut Conn, day: i64) -> Result<bool> {
    let xp: i64 = day_log::table.find(day).select(day_log::xp).first(conn).optional()?.unwrap_or(0);
    if xp >= db::get_setting_i64(conn, "daily_goal", DEFAULT_DAILY_GOAL) {
        return Ok(true);
    }
    let due: i64 = blocks::table.filter(blocks::due_day.le(day)).count().get_result(conn)?;
    Ok(xp > 0 && due == 0)
}

/// After a block: marks today done if it now is. Returns true the moment the
/// day becomes done — and then a seven-day run earns a streak freeze.
pub fn mark_practised(conn: &mut Conn, day: i64) -> Result<bool> {
    let already: Option<i64> =
        day_log::table.find(day).select(day_log::completed).first(conn).optional()?;
    if already == Some(1) || !day_is_done(conn, day)? {
        return Ok(false);
    }
    diesel::insert_into(day_log::table)
        .values((day_log::day.eq(day), day_log::completed.eq(1)))
        .on_conflict(day_log::day)
        .do_update()
        .set(day_log::completed.eq(1))
        .execute(conn)?;
    let (current, _) = streaks(conn, day)?;
    if current > 0 && current % 7 == 0 {
        let freezes = db::get_setting_i64(conn, "streak_freezes", 0);
        db::set_setting(conn, "streak_freezes", &(freezes + 1).min(MAX_FREEZES).to_string())?;
    }
    Ok(true)
}

/// Spends streak freezes on missed days, so a run survives them. Runs before
/// the streak is read: if the days missed since the last done day (today
/// not counted — it is not over) are no more than the freezes kept, each is
/// marked frozen. Frozen days keep a run going but do not add to it.
pub fn apply_freezes(conn: &mut Conn, today: i64) -> Result<()> {
    let last: Option<i64> = day_log::table
        .filter(day_log::completed.eq(1).or(day_log::frozen.eq(1)))
        .filter(day_log::day.lt(today))
        .select(diesel::dsl::max(day_log::day))
        .first(conn)?;
    let Some(last) = last else { return Ok(()) };
    let missed = today - 1 - last;
    let freezes = db::get_setting_i64(conn, "streak_freezes", 0);
    if missed < 1 || missed > freezes {
        return Ok(());
    }
    for day in last + 1..today {
        diesel::insert_into(day_log::table)
            .values((day_log::day.eq(day), day_log::frozen.eq(1)))
            .on_conflict(day_log::day)
            .do_update()
            .set(day_log::frozen.eq(1))
            .execute(conn)?;
    }
    db::set_setting(conn, "streak_freezes", &(freezes - missed).to_string())?;
    Ok(())
}

/// The current and longest streak, in done days. A frozen day keeps a run
/// going without adding to it.
pub fn streaks(conn: &mut Conn, today: i64) -> Result<(i64, i64)> {
    let rows: Vec<(i64, i64, i64)> = day_log::table
        .filter(day_log::completed.eq(1).or(day_log::frozen.eq(1)))
        .order(day_log::day)
        .select((day_log::day, day_log::completed, day_log::frozen))
        .load(conn)?;

    let mut longest = 0;
    let mut run = 0;
    let mut previous: Option<i64> = None;
    for &(day, completed, _) in &rows {
        if previous.is_some_and(|p| day != p + 1) {
            run = 0;
        }
        if completed == 1 {
            run += 1;
        }
        longest = longest.max(run);
        previous = Some(day);
    }

    // The current streak survives until the end of tomorrow: done yesterday
    // and not yet today still reads as a live streak.
    let kept: std::collections::HashMap<i64, bool> = rows.iter().map(|&(d, c, _)| (d, c == 1)).collect();
    let mut cursor = if kept.contains_key(&today) {
        today
    } else if kept.contains_key(&(today - 1)) {
        today - 1
    } else {
        return Ok((0, longest));
    };
    let mut current = 0;
    while let Some(&done) = kept.get(&cursor) {
        if done {
            current += 1;
        }
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

fn counts(conn: &mut Conn, today: i64) -> Result<Counts> {
    let dialogues = blocks::table.filter(blocks::format.eq("dialogue"));
    Ok(Counts {
        words_total: words::table.count().get_result(conn)?,
        words_seen: reviews::table.filter(reviews::kind.eq("word")).count().get_result(conn)?,
        words_mastered: reviews::table
            .filter(reviews::kind.eq("word"))
            .filter(reviews::reps.ge(MASTERY_REPS))
            .count()
            .get_result(conn)?,
        sentences_total: sentences::table.count().get_result(conn)?,
        sentences_seen: reviews::table.filter(reviews::kind.eq("sentence")).count().get_result(conn)?,
        dialogues_total: dialogues.count().get_result(conn)?,
        dialogues_seen: dialogues.filter(blocks::reps.gt(0)).count().get_result(conn)?,
        // What is due is counted in blocks, not single words.
        due_today: blocks::table.filter(blocks::due_day.le(today)).count().get_result(conn)?,
    })
}

pub fn collect(conn: &mut Conn) -> Result<Stats> {
    let today = db::today(conn);
    apply_freezes(conn, today)?;
    let c = counts(conn, today)?;
    let total = total_xp(conn)?;
    let level = level_from_xp(total);
    let (current_streak, longest_streak) = streaks(conn, today)?;

    let days: Vec<(i64, i64, i64, i64, i64, i64)> = day_log::table
        .order(day_log::day)
        .select((day_log::day, day_log::xp, day_log::answered, day_log::correct, day_log::completed, day_log::frozen))
        .load(conn)?;
    let (day_xp, completed_today) = days
        .iter()
        .find(|d| d.0 == today)
        .map_or((0, false), |d| (d.1, d.4 == 1));
    let answered: i64 = days.iter().map(|d| d.2).sum();
    let correct: i64 = days.iter().map(|d| d.3).sum();
    let history = days
        .iter()
        .filter(|d| d.0 > today - HISTORY_DAYS)
        .map(|&(day, xp, answered, correct, completed, frozen)| DayStat {
            day,
            xp,
            answered,
            correct,
            completed: completed == 1,
            frozen: frozen == 1,
        })
        .collect();

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
        streak_freezes: db::get_setting_i64(conn, "streak_freezes", 0),
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

pub fn topic_progress(conn: &mut Conn) -> Result<Vec<TopicProgress>> {
    let ids: Vec<i64> = topics::table.order(crate::blocks::course_order()).select(topics::id).load(conn)?;
    let seen = |kind: &'static str| reviews::table.filter(reviews::kind.eq(kind)).select(reviews::item_id);
    ids.into_iter()
        .map(|id| {
            let topic = session::load_topic(conn, id)?;
            let course_words = course_words::table.filter(course_words::topic_id.eq(id));
            let course_sentences = course_sentences::table.filter(course_sentences::topic_id.eq(id));
            let listed = course_blocks::table.filter(course_blocks::topic_id.eq(id));
            let image_file_id: Option<i64> = topics::table.find(id).select(topics::image_file_id).first(conn)?;
            Ok(TopicProgress {
                image_file_id,
                topic,
                total_words: course_words.count().get_result(conn)?,
                seen_words: course_words.filter(course_words::id.eq_any(seen("word"))).count().get_result(conn)?,
                mastered_words: course_words
                    .filter(course_words::id.eq_any(seen("word").filter(reviews::reps.ge(MASTERY_REPS))))
                    .count()
                    .get_result(conn)?,
                total_sentences: course_sentences.count().get_result(conn)?,
                seen_sentences: course_sentences
                    .filter(course_sentences::id.eq_any(seen("sentence")))
                    .count()
                    .get_result(conn)?,
                total_blocks: listed.count().get_result(conn)?,
                started_blocks: listed
                    .inner_join(blocks::table)
                    .filter(blocks::due_day.is_not_null())
                    .count()
                    .get_result(conn)?,
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
    Definition { code: "first_day", title: "Dzień dobry", description: "Finish your first block", icon: "🌅", target: 1 },
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
    Definition { code: "all_dialogues", title: "Ten conversations", description: "Work through ten conversations", icon: "💬", target: 10 },
    Definition { code: "level_5", title: "Level five", description: "Reach level five", icon: "⭐", target: 5 },
    Definition { code: "level_10", title: "Level ten", description: "Reach level ten", icon: "🌟", target: 10 },
    Definition { code: "xp_5000", title: "Five thousand", description: "Earn 5000 XP", icon: "⚡", target: 5000 },
];

fn progress_value(def: &Definition, c: &Counts, xp: i64, level: i64, longest_streak: i64) -> i64 {
    match def.code {
        "first_day" => i64::from(longest_streak > 0),
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
    conn: &mut Conn,
    c: &Counts,
    xp: i64,
    level: i64,
    longest_streak: i64,
) -> Result<Vec<Achievement>> {
    let unlocked: std::collections::HashSet<String> =
        achievements::table.select(achievements::code).load::<String>(conn)?.into_iter().collect();

    let today = db::today(conn);
    let mut result = Vec::with_capacity(DEFINITIONS.len());

    for def in DEFINITIONS {
        let value = progress_value(def, c, xp, level, longest_streak);
        let earned = value >= def.target;
        if earned && !unlocked.contains(def.code) {
            diesel::insert_or_ignore_into(achievements::table)
                .values((
                    achievements::code.eq(def.code),
                    achievements::unlocked_day.eq(today),
                    achievements::unlocked_at.eq(db::now_secs()),
                ))
                .execute(conn)?;
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
