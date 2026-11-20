//! The day's lessons: a handful of the most important ones — by default
//! five — chosen once a day and kept. Finishing them is the day done, with
//! a celebration and nothing more: there is no pile of everything due. More
//! can always be done from a course's page, where its lessons are listed in
//! the same order of importance (see [`order_for_course`]).
//!
//! What matters most: a lesson **slipping** (failed or partly there last
//! time) that is due again, then the other lessons **due**, most overdue
//! first — then, while there is room, at most [`MAX_NEW`] **new** lessons, the
//! next of the newest courses. A **mastered** lesson is never due again,
//! nor is one only a closed course lists: they are out of play — except that,
//! so a mastered lesson's words don't fade, one whose words are due for
//! review (`reviews.due_day`) can take a spare place as a light **review**:
//! at most [`MAX_REVIEWS`] a day, only when the due and new lessons leave
//! room, never in place of one. It stays mastered whatever happens: a miss
//! there only brings its words back sooner (srs.rs).
//!
//! New check-ins are **spread**: a lesson lands on the lightest day near its
//! target, so they don't bunch up.

use std::collections::HashSet;

use diesel::prelude::*;
use diesel::upsert::excluded;
use serde::Serialize;

use crate::lesson_types::minutes as minutes_for;
use crate::db::{self, Conn};
use crate::error::Result;
use crate::schema::{course_lessons, courses, lessons, day_log};
#[allow(unused_imports)]
use crate::ids::*;

/// Lessons a day, unless the learner sets another number (Settings).
pub const DEFAULT_DAILY: i64 = 5;
/// At most this many new lessons a day: what is due comes first.
pub const MAX_NEW: usize = 2;
/// At most this many mastered lessons a day come back as a review.
pub const MAX_REVIEWS: usize = 1;

/// How many lessons a day asks for (Settings).
pub fn daily_count(conn: &mut Conn) -> i64 {
    db::get_setting_i64(conn, "daily_lessons", DEFAULT_DAILY).clamp(1, 20)
}

/// The lightest day near `today + gap` for a lesson's next check-in:
/// within a tenth of the gap either side (short gaps stay exact).
pub fn spread(conn: &mut Conn, today: i64, gap: i64, lesson_id: LessonId) -> Result<i64> {
    let target = today + gap;
    if gap < 4 {
        return Ok(target);
    }
    let fuzz = crate::convert::round(crate::convert::real(gap) * 0.1).clamp(1, 7);
    let (from, to) = ((target - fuzz).max(today + 1), target + fuzz);
    let booked: Vec<(i64, String, String)> = lessons::table
        .filter(lessons::due_day.between(from, to))
        .filter(lessons::id.ne(lesson_id))
        .select((lessons::due_day.assume_not_null(), lessons::lesson_type, lessons::complexity))
        .load(conn)?;
    let load = |day: i64| -> i64 {
        booked.iter().filter(|(d, _, _)| *d == day).map(|(_, kind, complexity)| minutes_for(kind, complexity)).sum()
    };
    Ok((from..=to).min_by_key(|day| (load(*day), (day - target).abs())).unwrap_or(target))
}

/// Counts a scheduled check-in's minutes toward the day (and toward
/// "ahead" when it was done before its day).
pub fn log_minutes(conn: &mut Conn, day: i64, minutes: i64, early: bool) -> Result<()> {
    let early = if early { minutes } else { 0 };
    let device = crate::sync::device_id(conn)?;
    diesel::insert_into(day_log::table)
        .values((
            day_log::day.eq(day),
            day_log::device.eq(device),
            day_log::minutes.eq(minutes),
            day_log::early_minutes.eq(early),
        ))
        .on_conflict((day_log::day, day_log::device))
        .do_update()
        .set((
            day_log::minutes.eq(day_log::minutes + excluded(day_log::minutes)),
            day_log::early_minutes.eq(day_log::early_minutes + excluded(day_log::early_minutes)),
        ))
        .execute(conn)?;
    Ok(())
}

#[derive(Queryable)]
struct Scheduled {
    id: LessonId,
    due_day: Option<i64>,
    last_day: Option<i64>,
    interval_days: i64,
    ease: f64,
    last_result: Option<String>,
    mastered_at: Option<i64>,
    challenge: i64,
}

impl Scheduled {
    fn mastered(&self) -> bool {
        self.mastered_at.is_some()
    }
}

/// Lessons out of play: mastered ones, and ones only closed courses list.
#[cfg(test)]
pub fn out_of_play(conn: &mut Conn) -> Result<HashSet<LessonId>> {
    let mut out = crate::courses::retired_lessons(conn)?;
    out.extend(in_play_or_not(conn)?.into_iter().filter(|l| l.mastered()).map(|l| l.id));
    Ok(out)
}

fn in_play_or_not(conn: &mut Conn) -> Result<Vec<Scheduled>> {
    Ok(lessons::table
        .select((
            lessons::id,
            lessons::due_day,
            lessons::last_day,
            lessons::interval_days,
            lessons::ease,
            lessons::last_result,
            lessons::mastered_at,
            lessons::challenge,
        ))
        .load::<Scheduled>(conn)?)
}

/// Every lesson still in play.
fn in_play(conn: &mut Conn) -> Result<Vec<Scheduled>> {
    let retired = crate::courses::retired_lessons(conn)?;
    Ok(in_play_or_not(conn)?.into_iter().filter(|l| !l.mastered() && !retired.contains(&l.id)).collect())
}

/// How much a due lesson is at risk of being forgotten: how far past its gap
/// it is, more for a hard lesson (low ease).
fn risk(lesson: &Scheduled, today: i64) -> f64 {
    let elapsed = crate::convert::real(lesson.last_day.map_or(0, |last| today - last));
    let overdue = elapsed / crate::convert::real(lesson.interval_days.max(1));
    let hard = (2.5 - lesson.ease).max(0.0) * 0.5;
    overdue + hard
}

/// Where a lesson stands in importance, most important first: 0 slipping
/// and due, 1 due, 2 new, 3 started but not due yet, 4 an unstarted
/// challenge. Within a group, the riskier (or sooner due) first.
fn importance(lesson: &Scheduled, today: i64) -> (u8, f64) {
    let due = lesson.due_day.is_some_and(|d| d <= today);
    let slipping = matches!(lesson.last_result.as_deref(), Some("fail") | Some("partial"));
    match (lesson.due_day, due) {
        (Some(_), true) if slipping => (0, -risk(lesson, today)),
        (Some(_), true) => (1, -risk(lesson, today)),
        (None, _) if lesson.challenge == 0 => (2, 0.0),
        (Some(day), false) => (3, crate::convert::real(day)),
        (None, _) => (4, 0.0),
    }
}

/// The next new lessons: from the newest courses first, each in its own
/// order — what the tutor just made is what the learner asked for.
fn new_lessons(conn: &mut Conn, playing: &HashSet<LessonId>, limit: usize) -> Result<Vec<LessonId>> {
    let mut found = Vec::new();
    for course in courses::table
        .filter(courses::closed_at.is_null())
        .order(crate::lessons::course_order())
        .select(courses::id)
        .load::<CourseId>(conn)?
    {
        let listed: Vec<(LessonId, i64, i64, Option<i64>)> = course_lessons::table
            .inner_join(lessons::table)
            .filter(course_lessons::course_id.eq(course))
            .order((course_lessons::ord, lessons::id))
            .select((lessons::id, lessons::challenge, lessons::passed, lessons::due_day))
            .load(conn)?;
        // Challenges come once the course's other lessons are passed: they
        // are needed to master it, so the daily list offers them then.
        let core_passed = listed.iter().filter(|l| l.1 == 0).all(|l| l.2 == 1);
        let ids = listed.into_iter().filter(|l| l.3.is_none() && (l.1 == 0 || core_passed)).map(|l| l.0);
        for id in ids {
            if found.len() == limit {
                return Ok(found);
            }
            if playing.contains(&id) && !found.contains(&id) {
                found.push(id);
            }
        }
    }
    Ok(found)
}

/// Today's lessons, chosen the first time they are asked for each day and
/// kept: any already done today, then what is due, most important first,
/// then new ones while there is room (at most [`MAX_NEW`]).
/// Whether this device joined a vault and hasn't yet had a whole sync: its
/// lessons are still arriving, so a list chosen now would be chosen from
/// half the data — and, being a synced setting and the newest, would replace
/// the other devices' list too.
fn catching_up(conn: &mut Conn) -> Result<bool> {
    #[derive(diesel::QueryableByName)]
    struct Found {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    let found = diesel::sql_query(
        "SELECT count(*) AS n FROM sync_state s
         WHERE s.id = 1 AND s.vault_id IS NOT NULL
           AND EXISTS (SELECT 1 FROM sync_peers)
           AND NOT EXISTS (SELECT 1 FROM sync_peers WHERE last_complete_at IS NOT NULL)",
    )
    .get_result::<Found>(conn)?;
    Ok(found.n > 0)
}

fn todays_list(conn: &mut Conn, today: i64) -> Result<Vec<LessonId>> {
    if db::get_setting_i64(conn, "daily_day", -1) == today {
        if let Some(ids) =
            db::get_setting(conn, "daily_ids")?.and_then(|json| serde_json::from_str::<Vec<LessonId>>(&json).ok())
        {
            // A kept list stands — unless it names nothing this device has:
            // empty (chosen before there was anything to choose, e.g. while
            // a joining device's data was still arriving), or only lessons
            // that aren't here. Then it's chosen again.
            let known: i64 = if ids.is_empty() {
                0
            } else {
                lessons::table.filter(lessons::id.eq_any(&ids)).count().get_result(conn)?
            };
            if known > 0 {
                return Ok(ids);
            }
        }
    }
    let size = crate::convert::index(daily_count(conn));
    // Lessons already done today count toward the day: chosen after work
    // has been done (the first day after an update, or a new size set
    // mid-day), the list starts with that work, not from nothing.
    let mut done_today: Vec<LessonId> = lessons::table
        .filter(lessons::last_day.eq(today))
        .order(lessons::id)
        .select(lessons::id)
        .load(conn)?;
    done_today.truncate(size);
    let mut due: Vec<Scheduled> = in_play(conn)?
        .into_iter()
        .filter(|l| l.last_day != Some(today) && l.due_day.is_some_and(|d| d <= today))
        .collect();
    due.sort_by(|a, b| {
        let (ga, ka) = importance(a, today);
        let (gb, kb) = importance(b, today);
        ga.cmp(&gb).then(ka.total_cmp(&kb)).then(a.id.cmp(&b.id))
    });
    let mut ids = done_today;
    ids.extend(due.iter().take(size - ids.len()).map(|l| l.id));
    if ids.len() < size {
        let playing: HashSet<LessonId> =
            in_play(conn)?.into_iter().map(|l| l.id).filter(|id| !ids.contains(id)).collect();
        ids.extend(new_lessons(conn, &playing, (size - ids.len()).min(MAX_NEW))?);
    }
    if ids.len() < size && MAX_REVIEWS > 0 {
        ids.extend(review_due(conn, today, &ids)?);
    }
    // Kept for the day — but not an empty list (nothing to keep: it is
    // chosen again next time), and not while this device is still catching
    // up with its vault (see `catching_up`).
    if !ids.is_empty() && !catching_up(conn)? {
        db::set_setting(conn, "daily_ids", &serde_json::to_string(&ids)?)?;
        db::set_setting(conn, "daily_day", &today.to_string())?;
    }
    Ok(ids)
}

/// A mastered lesson whose words (or sentences) are due for review today,
/// the one most overdue first (then the one with the most due): a light
/// review for a spare place in the day's list. Not one done today, not one
/// only closed courses list, and only lessons with words or sentences to
/// review (a writing or study lesson has none).
fn review_due(conn: &mut Conn, today: i64, taken: &[LessonId]) -> Result<Option<LessonId>> {
    #[derive(diesel::QueryableByName)]
    struct Due {
        #[diesel(sql_type = diesel::sql_types::Text)]
        lesson_id: String,
    }
    let due: Vec<Due> = diesel::sql_query(
        "SELECT l.id AS lesson_id, min(r.due_day) AS first, count(*) AS n
         FROM lessons l
         JOIN (SELECT w.lesson_id AS lesson_id, w.id AS item_id, 'word' AS kind FROM words w
               UNION ALL SELECT s.lesson_id, s.id, 'sentence' FROM sentences s) i ON i.lesson_id = l.id
         JOIN reviews r ON r.kind = i.kind AND r.item_id = i.item_id
         WHERE l.mastered_at IS NOT NULL AND (l.last_day IS NULL OR l.last_day <> ?1) AND r.due_day <= ?1
         GROUP BY l.id
         ORDER BY first, n DESC, l.id",
    )
    .bind::<diesel::sql_types::BigInt, _>(today)
    .load(conn)?;
    if due.is_empty() {
        return Ok(None);
    }
    let retired = crate::courses::retired_lessons(conn)?;
    Ok(due
        .into_iter()
        .filter_map(|d| LessonId::parse(&d.lesson_id).ok())
        .find(|id| !retired.contains(id) && !taken.contains(id)))
}

/// Where today stands.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// Today's lessons still to do, most important first.
    pub left: Vec<LessonId>,
    /// How many today asked for, and how many of those are done.
    pub total: usize,
    pub done: usize,
}

pub fn plan(conn: &mut Conn, today: i64) -> Result<Plan> {
    let list = todays_list(conn, today)?;
    let retired = crate::courses::retired_lessons(conn)?;
    let state: Vec<(LessonId, Option<i64>)> = lessons::table
        .filter(lessons::id.eq_any(&list))
        .select((lessons::id, lessons::last_day))
        .load(conn)?;
    let (mut left, mut done) = (Vec::new(), 0usize);
    for id in &list {
        // A lesson deleted since just drops out; one retired since counts
        // as done. A mastered one in the list is today's review (or was
        // mastered today: done).
        match state.iter().find(|s| s.0 == *id) {
            Some((_, last)) if *last == Some(today) => done += 1,
            Some(_) if retired.contains(id) => done += 1,
            Some(_) => left.push(*id),
            None => {}
        }
    }
    Ok(Plan { total: done + left.len(), left, done })
}

/// How many of a day's lessons are still to do: today's list, or — for a
/// day that has no list kept — what is due by then and still in play.
pub fn remaining(conn: &mut Conn, day: i64) -> Result<i64> {
    if day == db::today(conn) {
        return Ok(crate::convert::count(plan(conn, day)?.left.len()));
    }
    Ok(crate::convert::count(in_play(conn)?.into_iter().filter(|l| l.due_day.is_some_and(|d| d <= day)).count()))
}

/// A course's lessons, most important first (the order today's lessons are
/// chosen in), then the mastered ones. Ties keep the course's own order.
pub fn order_for_course(conn: &mut Conn, lessons: Vec<crate::lessons::Lesson>) -> Result<Vec<crate::lessons::Lesson>> {
    let today = db::today(conn);
    let facts: std::collections::HashMap<LessonId, Scheduled> = in_play_or_not(conn)?.into_iter().map(|l| (l.id, l)).collect();
    let mut ranked: Vec<(usize, (u8, f64), crate::lessons::Lesson)> = lessons
        .into_iter()
        .enumerate()
        .map(|(n, lesson)| {
            let key = match facts.get(&lesson.id) {
                Some(f) if f.mastered() => (9, 0.0),
                Some(f) => importance(f, today),
                None => (8, 0.0),
            };
            (n, key, lesson)
        })
        .collect();
    ranked.sort_by(|a, b| a.1 .0.cmp(&b.1 .0).then(a.1 .1.total_cmp(&b.1 .1)).then(a.0.cmp(&b.0)));
    Ok(ranked.into_iter().map(|(_, _, lesson)| lesson).collect())
}
