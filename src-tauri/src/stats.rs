//! XP, levels, streaks and achievements — the parts that make the daily
//! check-in worth showing up for.

use diesel::prelude::*;
use diesel::upsert::excluded;

use crate::db::{self, Conn};
use crate::schema::{achievements, lessons, checkins, course_lessons, course_sentences, course_words, day_log, day_totals, reviews, sentences, songs, courses, words};
use crate::error::Result;
use crate::models::{Achievement, DayStat, Stats, CourseProgress};
use crate::session;
#[allow(unused_imports)]
use crate::ids::*;

/// About ten minutes: a drill and a conversation.
/// Streak freezes kept at most; one is earned with every seven-day run.
pub const MAX_FREEZES: i64 = 2;
/// A word is mastered once its review interval has reached three weeks:
/// remembered across real spacing, not just answered a few times in a row.
const MASTERY_INTERVAL: i64 = 21;
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
        "intro" | "sentenceIntro" | "pictureIntro" => 2,
        "recognise" | "wordPick" | "picturePick" => 6,
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
/// drills and is the deeper work, so it pays like it. A writing lesson adds
/// a bonus per goal got across. Failing earns none.
pub fn xp_for_checkin(complexity: &str, result: &str, writing_goals: usize) -> i64 {
    let base = match complexity {
        "low" => 30,
        "medium" => 80,
        _ => 160,
    } + 10 * crate::convert::count(writing_goals);
    match result {
        "pass" => base,
        "partial" => base / 2,
        _ => 0,
    }
}

/// XP that is not an answer (a check-in's verdict): it counts toward the day
/// and the level, not toward accuracy.
pub fn add_xp(conn: &mut Conn, day: i64, xp: i64) -> Result<()> {
    let device = crate::sync::device_id(conn)?;
    diesel::insert_into(day_log::table)
        .values((day_log::day.eq(day), day_log::device.eq(device), day_log::xp.eq(xp)))
        .on_conflict((day_log::day, day_log::device))
        .do_update()
        .set(day_log::xp.eq(day_log::xp + excluded(day_log::xp)))
        .execute(conn)?;
    Ok(())
}

/// Every device's XP, every day.
pub fn total_xp(conn: &mut Conn) -> Result<i64> {
    Ok(day_log::table.select(day_log::xp).load::<i64>(conn)?.into_iter().sum())
}

/// A day's XP, over every device that practised it.
pub fn day_xp(conn: &mut Conn, day: i64) -> Result<i64> {
    Ok(day_totals::table.find(day).select(day_totals::xp).first(conn).optional()?.unwrap_or(0))
}

pub fn record_answer(conn: &mut Conn, day: i64, xp: i64, correct: bool) -> Result<()> {
    let device = crate::sync::device_id(conn)?;
    diesel::insert_into(day_log::table)
        .values((
            day_log::day.eq(day),
            day_log::device.eq(device),
            day_log::xp.eq(xp),
            day_log::answered.eq(1),
            day_log::correct.eq(i64::from(correct)),
        ))
        .on_conflict((day_log::day, day_log::device))
        .do_update()
        .set((
            day_log::xp.eq(day_log::xp + excluded(day_log::xp)),
            day_log::answered.eq(day_log::answered + 1),
            day_log::correct.eq(day_log::correct + excluded(day_log::correct)),
        ))
        .execute(conn)?;
    Ok(())
}

/// Whether a day is done: today's lessons all done (see plan.rs) — or, on a
/// day with none to give, having practised at all. One definition for the
/// ring, the streak and the reminder.
pub fn day_is_done(conn: &mut Conn, day: i64) -> Result<bool> {
    let practised = day_xp(conn, day)?;
    Ok(practised > 0 && crate::plan::remaining(conn, day)? == 0)
}

/// After a lesson: marks today done if it now is. Returns true the moment the
/// day becomes done — and then a seven-day run earns a streak freeze.
pub fn mark_practised(conn: &mut Conn, day: i64) -> Result<bool> {
    let already: Option<i64> =
        day_totals::table.find(day).select(day_totals::completed).first(conn).optional()?;
    if already == Some(1) || !day_is_done(conn, day)? {
        return Ok(false);
    }
    let device = crate::sync::device_id(conn)?;
    diesel::insert_into(day_log::table)
        .values((day_log::day.eq(day), day_log::device.eq(device), day_log::completed.eq(1)))
        .on_conflict((day_log::day, day_log::device))
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
    let last: Option<i64> = day_totals::table
        .filter(day_totals::completed.eq(1).or(day_totals::frozen.eq(1)))
        .filter(day_totals::day.lt(today))
        .select(diesel::dsl::max(day_totals::day))
        .first(conn)?;
    let Some(last) = last else { return Ok(()) };
    let missed = today - 1 - last;
    let freezes = db::get_setting_i64(conn, "streak_freezes", 0);
    if missed < 1 || missed > freezes {
        return Ok(());
    }
    let device = crate::sync::device_id(conn)?;
    for day in last + 1..today {
        diesel::insert_into(day_log::table)
            .values((day_log::day.eq(day), day_log::device.eq(device), day_log::frozen.eq(1)))
            .on_conflict((day_log::day, day_log::device))
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
    let rows: Vec<(i64, i64, i64)> = day_totals::table
        .filter(day_totals::completed.eq(1).or(day_totals::frozen.eq(1)))
        .order(day_totals::day)
        .select((day_totals::day, day_totals::completed, day_totals::frozen))
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
    days_done: i64,
    checkins_passed: i64,
    writing_passed: i64,
    listening_passed: i64,
    songs_made: i64,
    tutor_lessons: i64,
    courses_finished: i64,
}

fn counts(conn: &mut Conn, today: i64) -> Result<Counts> {
    let dialogues = lessons::table.filter(lessons::lesson_type.eq("dialogue"));
    Ok(Counts {
        words_total: words::table.count().get_result(conn)?,
        words_seen: reviews::table.filter(reviews::kind.eq("word")).count().get_result(conn)?,
        words_mastered: reviews::table
            .filter(reviews::kind.eq("word"))
            .filter(reviews::interval.ge(MASTERY_INTERVAL))
            .count()
            .get_result(conn)?,
        sentences_total: sentences::table.count().get_result(conn)?,
        sentences_seen: reviews::table.filter(reviews::kind.eq("sentence")).count().get_result(conn)?,
        dialogues_total: dialogues.count().get_result(conn)?,
        dialogues_seen: dialogues.filter(lessons::reps.gt(0)).count().get_result(conn)?,
        // What is due is counted in lessons, not single words.
        due_today: crate::plan::remaining(conn, today)?,
        days_done: day_totals::table.filter(day_totals::completed.eq(1)).count().get_result(conn)?,
        checkins_passed: passed(conn, None)?,
        writing_passed: passed(conn, Some("writing"))?,
        listening_passed: passed(conn, Some("listening"))?,
        songs_made: songs::table.filter(songs::status.eq("ready")).count().get_result(conn)?,
        tutor_lessons: lessons::table.filter(lessons::origin.eq("tutor")).count().get_result(conn)?,
        courses_finished: courses_finished(conn)?,
    })
}

/// Check-ins passed, of lessons in one format (None: any).
fn passed(conn: &mut Conn, format: Option<&str>) -> Result<i64> {
    let mut query = checkins::table
        .inner_join(lessons::table)
        .filter(checkins::result.eq("pass"))
        .into_boxed();
    if let Some(format) = format {
        query = query.filter(lessons::lesson_type.eq(format));
    }
    Ok(query.count().get_result(conn)?)
}

/// Courses completed: every core lesson passed (courses::status). The
/// Other lessons folder is never completed.
fn courses_finished(conn: &mut Conn) -> Result<i64> {
    Ok(courses::table
        .filter(courses::completed_at.is_not_null())
        .filter(courses::slug.ne_all(crate::courses::OPEN_ENDED))
        .count()
        .get_result(conn)?)
}

pub fn collect(conn: &mut Conn) -> Result<Stats> {
    let today = db::today(conn);
    apply_freezes(conn, today)?;
    let c = counts(conn, today)?;
    let total = total_xp(conn)?;
    let level = level_from_xp(total);
    let (current_streak, longest_streak) = streaks(conn, today)?;

    let days: Vec<(i64, i64, i64, i64, i64, i64)> = day_totals::table
        .order(day_totals::day)
        .select((
            day_totals::day,
            day_totals::xp,
            day_totals::answered,
            day_totals::correct,
            day_totals::completed,
            day_totals::frozen,
        ))
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

    let achievements = evaluate_achievements(conn, &c, longest_streak)?;

    Ok(Stats {
        day: today,
        total_xp: total,
        day_xp,
        daily_lessons: crate::plan::daily_count(conn),
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
            crate::convert::real(correct) / crate::convert::real(answered)
        } else {
            0.0
        },
        completed_today,
        history,
        achievements,
        mastered: mastered_summary(conn)?,
    })
}

/// Courses closed as mastered, and the lessons and words in them, each once
/// (a lesson two closed courses list counts once).
pub fn mastered_summary(conn: &mut Conn) -> Result<crate::models::MasteredSummary> {
    let closed = courses::table.filter(courses::closed_at.is_not_null());
    let lesson_ids: std::collections::HashSet<LessonId> = course_lessons::table
        .inner_join(courses::table)
        .filter(courses::closed_at.is_not_null())
        .select(course_lessons::lesson_id)
        .load::<LessonId>(conn)?
        .into_iter()
        .collect();
    let ids: Vec<LessonId> = lesson_ids.iter().copied().collect();
    Ok(crate::models::MasteredSummary {
        courses: closed.count().get_result(conn)?,
        lessons: crate::convert::count(ids.len()),
        words: words::table.filter(words::lesson_id.eq_any(&ids)).count().get_result(conn)?,
    })
}

pub fn course_progress(conn: &mut Conn) -> Result<Vec<CourseProgress>> {
    let ids: Vec<CourseId> = courses::table.order(crate::lessons::course_order()).select(courses::id).load(conn)?;
    // Has a review of this kind, at least this many days apart. (A review's
    // item is a word's or a sentence's id: compared as text, in SQL.)
    let seen = |kind: &'static str, interval: i64| {
        diesel::dsl::sql::<diesel::sql_types::Bool>(&format!(
            "id IN (SELECT item_id FROM reviews WHERE kind = '{kind}' AND interval >= {interval})"
        ))
    };
    ids.into_iter()
        .map(|id| {
            let course = session::load_course(conn, id)?;
            let course_words = course_words::table.filter(course_words::course_id.eq(id));
            let course_sentences = course_sentences::table.filter(course_sentences::course_id.eq(id));
            let listed = course_lessons::table.filter(course_lessons::course_id.eq(id));
            let image_file_id: Option<FileId> = courses::table.find(id).select(courses::image_file_id).first(conn)?;
            Ok(CourseProgress {
                status: crate::courses::status(conn, id)?,
                image_file_id,
                course,
                total_words: course_words.count().get_result(conn)?,
                seen_words: course_words.filter(seen("word", 0)).count().get_result(conn)?,
                mastered_words: course_words.filter(seen("word", MASTERY_INTERVAL)).count().get_result(conn)?,
                total_sentences: course_sentences.count().get_result(conn)?,
                seen_sentences: course_sentences
                    .filter(seen("sentence", 0))
                    .count()
                    .get_result(conn)?,
                total_lessons: listed.count().get_result(conn)?,
                started_lessons: listed
                    .inner_join(lessons::table)
                    .filter(lessons::due_day.is_not_null())
                    .count()
                    .get_result(conn)?,
            })
        })
        .collect()
}

struct Definition {
    code: &'static str,
    title: &'static str,
    /// What it is about.
    description: &'static str,
    /// What to do to earn it.
    requirement: &'static str,
    icon: &'static str,
    /// What `measure` counts, singular.
    unit: &'static str,
}

/// What there is to earn — the habits and the milestones of this app:
/// showing up, words that stick, and the deeper work (check-ins, writing,
/// listening). Levels and raw XP are on the progress screen already. What
/// each one counts, and how many it takes, is `measure`.
const DEFINITIONS: &[Definition] = &[
    Definition { code: "first_day", title: "Dzień dobry", description: "Your first whole day of Polish.", requirement: "Finish all of a day's lessons once.", icon: "🌅", unit: "day" },
    Definition { code: "streak_3", title: "Three in a row", description: "A habit is taking shape.", requirement: "Finish your lessons three days in a row.", icon: "🔥", unit: "day" },
    Definition { code: "streak_7", title: "Full week", description: "A whole week without a gap.", requirement: "Finish your lessons seven days in a row. A streak freeze keeps a run going over a missed day.", icon: "📅", unit: "day" },
    Definition { code: "streak_30", title: "Miesiąc", description: "A month of Polish, every day.", requirement: "Finish your lessons 30 days in a row.", icon: "🏅", unit: "day" },
    Definition { code: "streak_100", title: "Sto dni", description: "A hundred days: Polish is part of your life now.", requirement: "Finish your lessons 100 days in a row.", icon: "💯", unit: "day" },
    Definition { code: "words_50", title: "Fifty words", description: "Enough words to start making sentences.", requirement: "Meet 50 Polish words in your lessons.", icon: "🌱", unit: "word" },
    Definition { code: "words_250", title: "Getting by", description: "Enough words for everyday situations.", requirement: "Meet 250 Polish words in your lessons.", icon: "🌿", unit: "word" },
    Definition { code: "words_all", title: "Every word", description: "Every word your courses teach.", requirement: "Meet every word in your courses (at least 50).", icon: "🌳", unit: "word" },
    Definition { code: "mastered_25", title: "Taking root", description: "Words that have stayed with you.", requirement: "Remember 25 words in reviews three weeks apart.", icon: "🪴", unit: "word" },
    Definition { code: "mastered_100", title: "Solid hundred", description: "A hundred words you really know.", requirement: "Remember 100 words in reviews three weeks apart.", icon: "💎", unit: "word" },
    Definition { code: "sentences_100", title: "Sentence builder", description: "Whole sentences, not just words.", requirement: "Practise 100 different sentences.", icon: "🧱", unit: "sentence" },
    Definition { code: "all_dialogues", title: "Ten dialogues", description: "Real conversations, worked through.", requirement: "Work through ten dialogue lessons.", icon: "💬", unit: "dialogue" },
    Definition { code: "checkin_first", title: "Passed!", description: "Your first lesson passed in conversation.", requirement: "Pass the check-in of any lesson.", icon: "✅", unit: "lesson" },
    Definition { code: "checkins_25", title: "Top of the class", description: "Polish you can use when it counts.", requirement: "Pass the check-ins of 25 lessons.", icon: "🎓", unit: "lesson" },
    Definition { code: "writing_first", title: "Got it across", description: "You wrote what you meant, and it was understood.", requirement: "Pass a writing lesson.", icon: "✍️", unit: "lesson" },
    Definition { code: "writing_10", title: "Pen pal", description: "Writing in Polish is becoming natural.", requirement: "Pass ten writing lessons.", icon: "📝", unit: "lesson" },
    Definition { code: "listening_first", title: "An ear for it", description: "Understanding Polish as it is spoken.", requirement: "Pass a listening lesson.", icon: "🎧", unit: "lesson" },
    Definition { code: "song_first", title: "First song", description: "Learning by singing along.", requirement: "Make a learning song in Music.", icon: "🎵", unit: "song" },
    Definition { code: "tutor_made", title: "Made to measure", description: "A lesson made just for you.", requirement: "Ask the tutor to make you a lesson.", icon: "🛠️", unit: "lesson" },
    Definition { code: "course_done", title: "Course complete", description: "A whole course behind you.", requirement: "Pass every core lesson of a course.", icon: "🏁", unit: "course" },
];

/// How far along an achievement is: what it counts now, and how many it
/// takes. The one place both come from, so the unlock check and the
/// progress shown always agree.
fn measure(code: &str, c: &Counts, longest_streak: i64) -> (i64, i64) {
    match code {
        "first_day" => (c.days_done, 1),
        "streak_3" => (longest_streak, 3),
        "streak_7" => (longest_streak, 7),
        "streak_30" => (longest_streak, 30),
        "streak_100" => (longest_streak, 100),
        "words_50" => (c.words_seen, 50),
        "words_250" => (c.words_seen, 250),
        // Measured against the words you have (but never fewer than 50).
        "words_all" => (c.words_seen, c.words_total.max(50)),
        "mastered_25" => (c.words_mastered, 25),
        "mastered_100" => (c.words_mastered, 100),
        "sentences_100" => (c.sentences_seen, 100),
        "all_dialogues" => (c.dialogues_seen, 10),
        "checkin_first" => (c.checkins_passed, 1),
        "checkins_25" => (c.checkins_passed, 25),
        "writing_first" => (c.writing_passed, 1),
        "writing_10" => (c.writing_passed, 10),
        "listening_first" => (c.listening_passed, 1),
        "song_first" => (c.songs_made, 1),
        "tutor_made" => (c.tutor_lessons, 1),
        "course_done" => (c.courses_finished, 1),
        // Unreachable for the codes above; an unknown one is never earned.
        _ => (0, 1),
    }
}

/// Every achievement with its progress; one newly earned is stored as
/// unlocked today.
fn evaluate_achievements(
    conn: &mut Conn,
    c: &Counts,
    longest_streak: i64,
) -> Result<Vec<Achievement>> {
    let stored: std::collections::HashMap<String, i64> = achievements::table
        .select((achievements::code, achievements::unlocked_day))
        .load::<(String, i64)>(conn)?
        .into_iter()
        .collect();

    let today = db::today(conn);
    let mut result = Vec::with_capacity(DEFINITIONS.len());

    for def in DEFINITIONS {
        let (value, target) = measure(def.code, c, longest_streak);
        let mut unlocked_day = stored.get(def.code).copied();
        if value >= target && unlocked_day.is_none() {
            diesel::insert_or_ignore_into(achievements::table)
                .values((
                    achievements::code.eq(def.code),
                    achievements::unlocked_day.eq(today),
                    achievements::unlocked_at.eq(db::now_secs()),
                ))
                .execute(conn)?;
            unlocked_day = Some(today);
        }
        let unlocked = unlocked_day.is_some();
        result.push(Achievement {
            code: def.code.to_string(),
            title: def.title.to_string(),
            description: def.description.to_string(),
            requirement: def.requirement.to_string(),
            icon: def.icon.to_string(),
            unlocked,
            // Once earned it stays earned (and full), whatever happens later.
            current: if unlocked { target } else { value.clamp(0, target) },
            target,
            unit: def.unit.to_string(),
            unlocked_day,
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
    fn every_achievement_is_measured() {
        let none = Counts {
            words_total: 0, words_seen: 0, words_mastered: 0, sentences_total: 0, sentences_seen: 0,
            dialogues_total: 0, dialogues_seen: 0, due_today: 0, days_done: 0, checkins_passed: 0,
            writing_passed: 0, listening_passed: 0, songs_made: 0, tutor_lessons: 0, courses_finished: 0,
        };
        let mut codes = std::collections::HashSet::new();
        for def in DEFINITIONS {
            assert!(codes.insert(def.code), "{} twice", def.code);
            let (value, target) = measure(def.code, &none, 4);
            assert!(target >= 1, "{} has a target", def.code);
            assert!(value < target || def.code == "streak_3", "{} starts unearned", def.code);
            assert!(!def.requirement.is_empty() && !def.description.is_empty());
        }
        assert_eq!(measure("streak_7", &none, 4), (4, 7), "streaks count the best run");
    }

    #[test]
    fn wrong_answers_earn_nothing() {
        assert_eq!(xp_for_answer("type", false, true), 0);
        assert!(xp_for_answer("type", true, false) > xp_for_answer("recognise", true, false));
    }
}
