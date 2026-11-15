//! The daily plan: which check-ins today asks for, out of everything due.
//!
//! With hundreds of blocks, "everything due" can be far more than a day can
//! hold — and some days hold a lot more than others. So today gets a list:
//! the due blocks most at risk of being forgotten, until a **budget** of
//! minutes is filled. The budget follows what the learner actually does (the
//! average of recent days they studied), never less than a floor they set,
//! so a quiet spell shrinks the list instead of piling up a wall.
//!
//! The list is made once a day and kept, so finishing it means something:
//! clearing it counts as clearing the day for the streak. What is due but
//! not on it waits, ranked again tomorrow, without counting as missed. On a
//! big day, **keep going** offers more — the rest of what is due, then what
//! comes due soonest; doing one early is a real check-in (see
//! [`crate::blocks::next_gap`]) and takes it off the day it was due.
//!
//! New check-ins are also **spread**: a block lands on the lightest day near
//! its target, so they don't bunch up.

use diesel::prelude::*;
use diesel::upsert::excluded;
use serde::Serialize;

use crate::blocks::minutes_for;
use crate::db::{self, Conn};
use crate::error::Result;
use crate::schema::{blocks, day_log};

/// The smallest daily list, in minutes, unless the learner sets another.
pub const DEFAULT_FLOOR: i64 = 10;
/// How many recent days the budget learns from.
const BUDGET_DAYS: i64 = 14;
/// How far ahead "keep going" looks for check-ins to pull forward.
const AHEAD_DAYS: i64 = 7;
/// How many extra check-ins "keep going" offers at a time.
const MORE: usize = 12;

/// The lightest day near `today + gap` for a block's next check-in:
/// within a tenth of the gap either side (short gaps stay exact).
pub fn spread(conn: &mut Conn, today: i64, gap: i64, block_id: i64) -> Result<i64> {
    let target = today + gap;
    if gap < 4 {
        return Ok(target);
    }
    let fuzz = ((gap as f64) * 0.1).round().clamp(1.0, 7.0) as i64;
    let (from, to) = ((target - fuzz).max(today + 1), target + fuzz);
    let booked: Vec<(i64, String, String)> = blocks::table
        .filter(blocks::due_day.between(from, to))
        .filter(blocks::id.ne(block_id))
        .select((blocks::due_day.assume_not_null(), blocks::kind, blocks::complexity))
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
    diesel::insert_into(day_log::table)
        .values((day_log::day.eq(day), day_log::minutes.eq(minutes), day_log::early_minutes.eq(early)))
        .on_conflict(day_log::day)
        .do_update()
        .set((
            day_log::minutes.eq(day_log::minutes + excluded(day_log::minutes)),
            day_log::early_minutes.eq(day_log::early_minutes + excluded(day_log::early_minutes)),
        ))
        .execute(conn)?;
    Ok(())
}

/// The quietest a day's list gets, in minutes (Settings).
pub fn floor(conn: &mut Conn) -> i64 {
    db::get_setting_i64(conn, "review_floor_minutes", DEFAULT_FLOOR).clamp(1, 240)
}

/// Today's budget: what the learner usually does on a day they study, never
/// less than the floor.
pub fn budget(conn: &mut Conn, today: i64) -> Result<i64> {
    let recent: Vec<i64> = day_log::table
        .filter(day_log::day.between(today - BUDGET_DAYS, today - 1))
        .filter(day_log::minutes.gt(0))
        .select(day_log::minutes)
        .load(conn)?;
    let usual = if recent.is_empty() { 0 } else { recent.iter().sum::<i64>() / recent.len() as i64 };
    Ok(usual.max(floor(conn)))
}

#[derive(Queryable)]
struct Scheduled {
    id: i64,
    kind: String,
    complexity: String,
    due_day: Option<i64>,
    last_day: Option<i64>,
    interval_days: i64,
    ease: f64,
    last_result: Option<String>,
}

fn scheduled(conn: &mut Conn, until: i64) -> Result<Vec<Scheduled>> {
    Ok(blocks::table
        .filter(blocks::due_day.le(until))
        .select((
            blocks::id,
            blocks::kind,
            blocks::complexity,
            blocks::due_day,
            blocks::last_day,
            blocks::interval_days,
            blocks::ease,
            blocks::last_result,
        ))
        .load(conn)?)
}

/// How much a due block is at risk of being forgotten: how far past its gap
/// it is, more for a hard block (low ease) or one that went badly last time.
fn risk(block: &Scheduled, today: i64) -> f64 {
    let elapsed = block.last_day.map_or(0, |last| today - last) as f64;
    let overdue = elapsed / block.interval_days.max(1) as f64;
    let hard = (2.5 - block.ease).max(0.0) * 0.5;
    let last = match block.last_result.as_deref() {
        Some("fail") => 1.0,
        Some("partial") => 0.3,
        _ => 0.0,
    };
    overdue + hard + last
}

fn by_risk(mut list: Vec<Scheduled>, today: i64) -> Vec<Scheduled> {
    list.sort_by(|a, b| risk(b, today).total_cmp(&risk(a, today)).then(a.id.cmp(&b.id)));
    list
}

/// Today's list, made the first time it is asked for each day and kept.
fn todays_list(conn: &mut Conn, today: i64) -> Result<Vec<i64>> {
    if db::get_setting_i64(conn, "plan_day", -1) == today {
        if let Some(ids) = db::get_setting(conn, "plan_ids")?.and_then(|json| serde_json::from_str(&json).ok()) {
            return Ok(ids);
        }
    }
    let budget = budget(conn, today)?;
    let due: Vec<Scheduled> =
        scheduled(conn, today)?.into_iter().filter(|b| b.last_day != Some(today)).collect();
    let mut ids = Vec::new();
    let mut minutes = 0;
    for block in by_risk(due, today) {
        if minutes >= budget {
            break;
        }
        minutes += minutes_for(&block.kind, &block.complexity);
        ids.push(block.id);
    }
    db::set_setting(conn, "plan_ids", &serde_json::to_string(&ids)?)?;
    db::set_setting(conn, "plan_day", &today.to_string())?;
    Ok(ids)
}

/// Where today stands.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// Today's list, still to do, most at risk first.
    pub left: Vec<i64>,
    /// How many were on today's list, and how many of those are done.
    pub total: usize,
    pub done: usize,
    /// Today's budget, in minutes.
    pub budget_minutes: i64,
    /// What "keep going" offers: the rest of what is due (most at risk
    /// first), then what comes due soonest.
    pub more: Vec<i64>,
    /// Minutes due but not on today's list: waiting for another day.
    pub behind_minutes: i64,
    /// Minutes of check-ins done before their day, this past week.
    pub ahead_minutes: i64,
}

pub fn plan(conn: &mut Conn, today: i64) -> Result<Plan> {
    let list = todays_list(conn, today)?;
    let state: Vec<(i64, Option<i64>, Option<i64>)> = blocks::table
        .filter(blocks::id.eq_any(&list))
        .select((blocks::id, blocks::due_day, blocks::last_day))
        .load(conn)?;
    let (mut left, mut done) = (Vec::new(), 0usize);
    for id in &list {
        // A block deleted since, or moved off today, just drops out.
        match state.iter().find(|s| s.0 == *id) {
            Some((_, _, last)) if *last == Some(today) => done += 1,
            Some((_, Some(due), _)) if *due <= today => left.push(*id),
            _ => {}
        }
    }

    let soon = scheduled(conn, today + AHEAD_DAYS)?;
    let (due, upcoming): (Vec<Scheduled>, Vec<Scheduled>) = soon
        .into_iter()
        .filter(|b| b.last_day != Some(today) && !list.contains(&b.id))
        .partition(|b| b.due_day.is_some_and(|d| d <= today));
    let behind_minutes = due.iter().map(|b| minutes_for(&b.kind, &b.complexity)).sum();
    let mut upcoming = upcoming;
    upcoming.sort_by(|a, b| a.due_day.cmp(&b.due_day).then(risk(b, today).total_cmp(&risk(a, today))));
    let more = by_risk(due, today).into_iter().chain(upcoming).take(MORE).map(|b| b.id).collect();

    let ahead_minutes = day_log::table
        .filter(day_log::day.between(today - 6, today))
        .select(day_log::early_minutes)
        .load::<i64>(conn)?
        .into_iter()
        .sum();
    Ok(Plan {
        total: done + left.len(),
        left,
        done,
        budget_minutes: budget(conn, today)?,
        more,
        behind_minutes,
        ahead_minutes,
    })
}

/// How many of a day's check-ins are still to do: today's list, or — for a
/// day that has no list kept — everything due by then.
pub fn remaining(conn: &mut Conn, day: i64) -> Result<i64> {
    if day == db::today(conn) {
        return Ok(plan(conn, day)?.left.len() as i64);
    }
    Ok(blocks::table.filter(blocks::due_day.le(day)).count().get_result(conn)?)
}
