//! The mastery ledger: every change to a lesson's mastery points, and every
//! graded go that left them as they were, with why — so the learner can see
//! the accounting on the lesson's page ("Mastery points") and the tutor can
//! read it (`get_mastery_log`).
//!
//! A row is written where mastery is decided (lessons.rs `credit_mastery`
//! and `stamp_mastered`), in the same transaction as the change, on the
//! device that made it, and never changed after: append-only and per device,
//! like the learning log (docs/SYNC.md: nothing to conflict). There is no backfill
//! (PRINCIPLES.md): a lesson's points from before the ledger began are shown
//! as one starting line.

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::{self, Conn, Db};
use crate::error::Result;
#[allow(unused_imports)]
use crate::ids::*;
use crate::schema::mastery_log;

/// The version of the details kept with each row.
pub const DETAILS_VERSION: u32 = 1;

/// Every reason a row can give, and what it means. No reason takes points
/// away (rows from before that rule keep the reason they were written with).
// Spec: MAST-6, MAST-10
pub const REASONS: &[(&str, &str)] = &[
    ("first_pass", "the first pass: 30 points (an ace a quarter more)"),
    ("pass_same_day", "a pass on the same day as the last go: 10"),
    ("pass_spaced", "a pass a day or more after the last go: 15, growing to 35 at two weeks apart"),
    ("partial", "partly there: a share of what a pass would earn in the same situation, by the score (a conversation: half)"),
    ("fail", "a fail: a share of what a pass would earn by the score, nothing at or below half right; never a loss"),
    ("fail_extra", "a fail on extra practice, from before fails stopped costing points: no change"),
    ("capped", "held at 90 until passes on two different days"),
    ("full", "already at 100: mastering it waits on something else (writing without help)"),
    ("mastered", "mastered: for good"),
    ("after_mastered", "a go at a mastered lesson: mastery is for good, no change"),
];

/// How a graded go was done: a drill (with its score) or a check-in, the go
/// it was (a drill's run key, a check-in's id), and whether it was extra
/// practice.
#[derive(Debug, Clone, Copy, Default)]
pub struct Go<'a> {
    /// "drill" | "checkin"
    pub via: &'a str,
    pub run: &'a str,
    /// A drill's score: right, almost, scored cards.
    pub score: Option<(i64, i64, i64)>,
    pub extra: bool,
}

/// The numbers behind a row, small and typed, versioned by `v`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Details {
    pub v: u32,
    /// What the rule gave before the 0–100 bounds and the cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain: Option<i64>,
    /// The cap that held the points, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap: Option<i64>,
    /// Set on the row of the go that first passed the lesson: that day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_pass_day: Option<i64>,
}

/// A row to write.
pub struct NewEntry<'a> {
    pub lesson_id: LessonId,
    pub before: i64,
    pub after: i64,
    /// fail | partial | pass | ace, or '' (the mastered stamp).
    pub grade: &'a str,
    pub days_since: Option<i64>,
    pub go: &'a Go<'a>,
    pub reason: &'a str,
    pub details: Details,
}

/// One row, as the lesson page and the tutor see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasteryEntry {
    pub id: MasteryEntryId,
    /// Milliseconds since 1970, and the learner's day.
    pub at: i64,
    pub day: i64,
    pub delta: i64,
    pub before: i64,
    pub after: i64,
    pub grade: String,
    pub correct: Option<i64>,
    pub almost: Option<i64>,
    pub total: Option<i64>,
    pub days_since: Option<i64>,
    pub extra: bool,
    /// "drill" | "checkin"
    pub via: String,
    pub reason: String,
    /// The row in plain words: "Pass, 3 days after the last go: +19 (30 → 49)".
    pub text: String,
}

/// A lesson's ledger: its points now, what the ledger doesn't explain
/// (points from before it began), and every row, newest first.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasteryLog {
    pub lesson_id: LessonId,
    pub mastery: i64,
    pub mastered: bool,
    /// The points the lesson had when its first row was written (or now,
    /// with no rows), when more than 0: earned before the log began.
    pub before_log: Option<i64>,
    pub entries: Vec<MasteryEntry>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Writes one row, and returns it as the screens see it.
pub fn write(conn: &mut Conn, e: &NewEntry) -> Result<MasteryEntry> {
    let id = MasteryEntryId::new();
    let (at, day) = (now_ms(), db::today(conn));
    let (correct, almost, total) = match e.go.score {
        Some((c, a, t)) => (Some(c), Some(a), Some(t)),
        None => (None, None, None),
    };
    diesel::insert_into(mastery_log::table)
        .values((
            mastery_log::id.eq(id),
            mastery_log::at.eq(at),
            mastery_log::day.eq(day),
            mastery_log::device.eq(crate::sync::device_id(conn)?),
            mastery_log::lesson_id.eq(e.lesson_id),
            mastery_log::delta.eq(e.after - e.before),
            mastery_log::points_before.eq(e.before),
            mastery_log::points_after.eq(e.after),
            mastery_log::grade.eq(e.grade),
            mastery_log::correct.eq(correct),
            mastery_log::almost.eq(almost),
            mastery_log::total.eq(total),
            mastery_log::days_since.eq(e.days_since),
            mastery_log::extra.eq(i64::from(e.go.extra)),
            mastery_log::via.eq(e.go.via),
            mastery_log::run.eq(e.go.run),
            mastery_log::reason.eq(e.reason),
            mastery_log::details.eq(serde_json::to_string(&e.details)?),
        ))
        .execute(conn)?;
    Ok(view(Row {
        id,
        at,
        day,
        delta: e.after - e.before,
        before: e.before,
        after: e.after,
        grade: e.grade.into(),
        correct,
        almost,
        total,
        days_since: e.days_since,
        extra: i64::from(e.go.extra),
        via: e.go.via.into(),
        reason: e.reason.into(),
        details: String::new(),
    }))
}

/// A row's columns, in [`Row`]'s order.
type Columns = (
    mastery_log::id,
    mastery_log::at,
    mastery_log::day,
    mastery_log::delta,
    mastery_log::points_before,
    mastery_log::points_after,
    mastery_log::grade,
    mastery_log::correct,
    mastery_log::almost,
    mastery_log::total,
    mastery_log::days_since,
    mastery_log::extra,
    mastery_log::via,
    mastery_log::reason,
    mastery_log::details,
);
const COLUMNS: Columns = (
    mastery_log::id,
    mastery_log::at,
    mastery_log::day,
    mastery_log::delta,
    mastery_log::points_before,
    mastery_log::points_after,
    mastery_log::grade,
    mastery_log::correct,
    mastery_log::almost,
    mastery_log::total,
    mastery_log::days_since,
    mastery_log::extra,
    mastery_log::via,
    mastery_log::reason,
    mastery_log::details,
);

#[derive(Queryable)]
struct Row {
    id: MasteryEntryId,
    at: i64,
    day: i64,
    delta: i64,
    before: i64,
    after: i64,
    grade: String,
    correct: Option<i64>,
    almost: Option<i64>,
    total: Option<i64>,
    days_since: Option<i64>,
    extra: i64,
    via: String,
    reason: String,
    #[allow(dead_code)]
    details: String,
}

fn view(r: Row) -> MasteryEntry {
    let text = describe(&r);
    MasteryEntry {
        id: r.id,
        at: r.at,
        day: r.day,
        delta: r.delta,
        before: r.before,
        after: r.after,
        grade: r.grade,
        correct: r.correct,
        almost: r.almost,
        total: r.total,
        days_since: r.days_since,
        extra: r.extra == 1,
        via: r.via,
        reason: r.reason,
        text,
    }
}

/// A score in words: "9 of 10", "7½ of 10 (1 almost)".
fn score_text(correct: i64, almost: i64, total: i64) -> String {
    let whole = correct + almost / 2;
    let half = if almost % 2 == 1 { "½" } else { "" };
    let almosts = if almost > 0 { format!(" ({almost} almost)") } else { String::new() };
    format!("{whole}{half} of {total}{almosts}")
}

/// A row in plain words.
fn describe(r: &Row) -> String {
    if r.reason == "mastered" {
        return format!("Mastered, for good ({} points)", r.after);
    }
    let grade = match r.grade.as_str() {
        "ace" => "Ace",
        "pass" => "Pass",
        "partial" => "Partial",
        _ => "Fail",
    };
    let grade = if r.via == "checkin" { format!("Conversation: {}", grade.to_lowercase()) } else { grade.to_string() };
    let score = match (r.correct, r.almost, r.total) {
        (Some(c), Some(a), Some(t)) if t > 0 => format!(" {}", score_text(c, a, t)),
        _ => String::new(),
    };
    let when = match r.days_since {
        None => "first go".to_string(),
        Some(0) => "same day".to_string(),
        Some(1) => "a day after the last go".to_string(),
        Some(n) => format!("{n} days after the last go"),
    };
    let extra = if r.extra == 1 { " on extra practice" } else { "" };
    let change = if r.delta == 0 {
        "no change".to_string()
    } else {
        let sign = if r.delta > 0 { "+" } else { "−" };
        format!("{sign}{} ({} → {})", r.delta.abs(), r.before, r.after)
    };
    let why = match r.reason.as_str() {
        "capped" => ", held at 90 until passes on two different days",
        "full" => ", already full",
        "after_mastered" => ", mastered for good",
        _ => "",
    };
    format!("{grade}{score}, {when}{extra}: {change}{why}")
}

/// The row of a lesson's first pass carries its day in its details.
const FIRST_PASS: &str = "%\"first_pass_day\"%";

/// The day the lesson was first passed, as the ledger knows it (None: never
/// passed since the ledger began — or passed before it began).
pub fn first_pass_day(conn: &mut Conn, lesson_id: LessonId) -> Result<Option<i64>> {
    Ok(mastery_log::table
        .filter(mastery_log::lesson_id.eq(lesson_id))
        .filter(mastery_log::details.like(FIRST_PASS))
        .select(mastery_log::day)
        .order(mastery_log::id)
        .first(conn)
        .optional()?)
}

/// Whether the lesson was passed on a day after `day` (the ledger's rows).
pub fn passed_after(conn: &mut Conn, lesson_id: LessonId, day: i64) -> Result<bool> {
    let n: i64 = mastery_log::table
        .filter(mastery_log::lesson_id.eq(lesson_id))
        .filter(mastery_log::grade.eq_any(["pass", "ace"]))
        .filter(mastery_log::day.gt(day))
        .count()
        .get_result(conn)?;
    Ok(n > 0)
}

/// Lessons first passed on `day` (for the estimates: they cannot be
/// mastered that day).
pub fn first_passed_on(conn: &mut Conn, day: i64) -> Result<std::collections::HashSet<LessonId>> {
    Ok(mastery_log::table
        .filter(mastery_log::details.like(FIRST_PASS))
        .filter(mastery_log::day.eq(day))
        .select(mastery_log::lesson_id)
        .load::<LessonId>(conn)?
        .into_iter()
        .collect())
}

/// A lesson's ledger, newest first.
pub fn for_lesson(conn: &mut Conn, lesson_id: LessonId) -> Result<MasteryLog> {
    let lesson = crate::lessons::load(conn, lesson_id)?;
    let rows: Vec<Row> = mastery_log::table
        .filter(mastery_log::lesson_id.eq(lesson_id))
        .order(mastery_log::id.desc())
        .select(COLUMNS)
        .load(conn)?;
    // What the first row started from (or the points now, with none): the
    // points earned before the log began.
    let start = rows.last().map_or(lesson.mastery, |r| r.before);
    Ok(MasteryLog {
        lesson_id,
        mastery: lesson.mastery,
        mastered: lesson.mastered,
        before_log: (start > 0).then_some(start),
        entries: rows.into_iter().map(view).collect(),
    })
}

/// A go's row in the ledger, found by its run (a check-in's id), and whether
/// the go also stamped the lesson mastered.
pub fn for_run(conn: &mut Conn, run: &str) -> Result<(Option<MasteryEntry>, bool)> {
    if run.is_empty() {
        return Ok((None, false));
    }
    let rows: Vec<Row> = mastery_log::table
        .filter(mastery_log::run.eq(run))
        .order(mastery_log::id)
        .select(COLUMNS)
        .load(conn)?;
    let mastered = rows.iter().any(|r| r.reason == "mastered");
    Ok((rows.into_iter().find(|r| r.reason != "mastered").map(view), mastered))
}

/// Every change to a lesson's mastery points, and why, newest first.
#[tauri::command]
pub async fn lesson_mastery_log(db: State<'_, Db>, lesson_id: LessonId) -> Result<MasteryLog> {
    for_lesson(&mut db.lock(), lesson_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(grade: &str, score: Option<(i64, i64, i64)>, days: Option<i64>, extra: bool, before: i64, after: i64, reason: &str) -> Row {
        let (correct, almost, total) = score.map_or((None, None, None), |(c, a, t)| (Some(c), Some(a), Some(t)));
        Row {
            id: MasteryEntryId::new(),
            at: 0,
            day: 0,
            delta: after - before,
            before,
            after,
            grade: grade.into(),
            correct,
            almost,
            total,
            days_since: days,
            extra: i64::from(extra),
            via: "drill".into(),
            reason: reason.into(),
            details: String::new(),
        }
    }

    #[test]
    fn rows_read_in_plain_words() {
        assert_eq!(describe(&row("pass", None, Some(3), false, 30, 49, "pass_spaced")), "Pass, 3 days after the last go: +19 (30 → 49)");
        assert_eq!(
            describe(&row("partial", Some((7, 1, 10)), Some(0), false, 30, 35, "partial")),
            "Partial 7½ of 10 (1 almost), same day: +5 (30 → 35)"
        );
        assert_eq!(describe(&row("fail", Some((2, 0, 10)), Some(0), true, 49, 49, "fail_extra")), "Fail 2 of 10, same day on extra practice: no change");
        assert_eq!(describe(&row("fail", None, Some(1), false, 10, 0, "fail")), "Fail, a day after the last go: −10 (10 → 0)");
        assert_eq!(describe(&row("pass", Some((10, 0, 10)), None, false, 0, 30, "first_pass")), "Pass 10 of 10, first go: +30 (0 → 30)");
        assert_eq!(describe(&row("pass", None, Some(0), false, 85, 90, "capped")), "Pass, same day: +5 (85 → 90), held at 90 until passes on two different days");
        assert_eq!(describe(&row("", None, None, false, 100, 100, "mastered")), "Mastered, for good (100 points)");
    }

    // Spec: keel.JSN-3
    #[test]
    fn details_are_strict_and_versioned() {
        let d = Details { v: DETAILS_VERSION, gain: Some(10), cap: Some(90), first_pass_day: Some(5) };
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Details>(&json).unwrap(), d);
        let unnamed = serde_json::from_str::<Details>(r#"{"v":1,"surprise":1}"#).unwrap();
        assert_eq!(unnamed, Details { v: 1, ..Details::default() }, "a field it doesn't name is dropped");
        assert!(serde_json::from_str::<Details>(r#"{"v":1,"gain":"ten"}"#).is_err());
    }
}
