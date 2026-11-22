//! The XP log: what earned each point, so any day can be opened and read —
//! each lesson run with its answers, each check-in with its verdict. It is
//! written where XP is awarded (practice.rs for answers, examiner.rs for
//! check-ins), next to the day's total in day_log (summed over devices), so
//! the two always agree.
//! Days from before the log have their total only.

use std::collections::HashMap;

use diesel::prelude::*;
use serde::Serialize;
use tauri::State;

use crate::db::{self, Conn, Db};
use crate::error::Result;
use crate::schema::{courses, course_lessons, day_totals, lessons, xp_log};
#[allow(unused_imports)]
use crate::ids::*;

/// Notes something that earned (or, with 0, did not earn) XP.
#[allow(clippy::too_many_arguments)]
pub fn write(
    conn: &mut Conn,
    day: i64,
    run: &str,
    lesson_id: Option<LessonId>,
    label: &str,
    xp: i64,
    note: &str,
) -> Result<()> {
    diesel::insert_into(xp_log::table)
        .values((
            xp_log::id.eq(XpEntryId::new()),
            xp_log::day.eq(day),
            xp_log::at.eq(db::now_secs()),
            xp_log::run.eq(run),
            xp_log::lesson_id.eq(lesson_id),
            xp_log::label.eq(label),
            xp_log::xp.eq(xp),
            xp_log::note.eq(note),
        ))
        .execute(conn)?;
    Ok(())
}

/// An answer in words: what was done, and with what.
pub fn answer_label(kind: &str, expected: &str) -> String {
    let what: String = expected.chars().take(60).collect();
    let what = if expected.chars().count() > 60 { format!("{what}…") } else { what };
    match kind {
        "intro" | "pictureIntro" => format!("Met “{what}”"),
        "sentenceIntro" => format!("Read “{what}”"),
        "dialogueRead" => "Read the conversation".into(),
        "recognise" => format!("Recognised “{what}”"),
        "produce" => format!("Picked “{what}”"),
        "type" => format!("Typed “{what}”"),
        "wordOrder" | "wordOrderTwo" | "wordOrderSet" => format!("Built “{what}”"),
        "bankDictation" | "wordDictation" => format!("Wrote down “{what}”"),
        "gapType" => format!("Filled in “{what}”"),
        "dialogueGap" => format!("Said the next line: “{what}”"),
        "gapFill" => "Filled the gaps".into(),
        "memory" => "Memory round".into(),
        "wordPick" => format!("Picked the picture of “{what}”"),
        "picturePick" => format!("Named the picture: “{what}”"),
        "dictation" => format!("Wrote down “{what}”"),
        "soundPair" => format!("Heard “{what}”"),
        "sayIt" => format!("Said “{what}”"),
        "dialogueSay" => format!("Said the next line: “{what}”"),
        "storyPanel" => "Read a page of a story".into(),
        "storyEnd" => "Finished a story".into(),
        "storyQuestion" => "Answered a question about the story".into(),
        other => format!("{other}: “{what}”"),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub at: i64,
    pub label: String,
    pub xp: i64,
    /// Why it earned what it did, when that needs saying ("a mistake: no XP").
    pub note: String,
}

/// One lesson run (or check-in) of a day.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub lesson_id: Option<LessonId>,
    pub lesson_title: String,
    pub course_title: Option<String>,
    pub started_at: i64,
    pub xp: i64,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct XpDay {
    pub day: i64,
    /// The day's total (day_log): kept for every day.
    pub total: i64,
    /// Whether the day has its details (days from before the log do not).
    pub detailed: bool,
    pub runs: Vec<Run>,
}

/// A day's XP: its total, and what earned it, run by run in order.
pub fn day(conn: &mut Conn, day: i64) -> Result<XpDay> {
    let total = crate::stats::day_xp(conn, day)?;
    let rows: Vec<(i64, String, Option<LessonId>, String, i64, String)> = xp_log::table
        .filter(xp_log::day.eq(day))
        .order(xp_log::id)
        .select((xp_log::at, xp_log::run, xp_log::lesson_id, xp_log::label, xp_log::xp, xp_log::note))
        .load(conn)?;
    let ids: Vec<LessonId> = rows.iter().filter_map(|r| r.2).collect();
    let titles: HashMap<LessonId, String> =
        lessons::table.filter(lessons::id.eq_any(&ids)).select((lessons::id, lessons::title)).load(conn)?.into_iter().collect();
    let course_of: HashMap<LessonId, String> = course_lessons::table
        .inner_join(courses::table)
        .filter(course_lessons::lesson_id.eq_any(&ids))
        .select((course_lessons::lesson_id, courses::title_en))
        .load::<(LessonId, String)>(conn)?
        .into_iter()
        .collect();
    let mut order: Vec<String> = Vec::new();
    let mut runs: HashMap<String, Run> = HashMap::new();
    for (at, run, lesson_id, label, xp, note) in rows {
        let entry = runs.entry(run.clone()).or_insert_with(|| {
            order.push(run.clone());
            Run {
                lesson_id,
                lesson_title: lesson_id.and_then(|id| titles.get(&id).cloned()).unwrap_or_else(|| {
                    if lesson_id.is_some() {
                        "A lesson since deleted".into()
                    } else if run.starts_with(crate::games::RUN_PREFIX) {
                        // Played from Home, outside the lessons (games.rs).
                        "Quick game".into()
                    } else {
                        "Practice".into()
                    }
                }),
                course_title: lesson_id.and_then(|id| course_of.get(&id).cloned()),
                started_at: at,
                xp: 0,
                entries: Vec::new(),
            }
        });
        entry.xp += xp;
        entry.entries.push(Entry { at, label, xp, note });
    }
    let runs: Vec<Run> = order.into_iter().filter_map(|key| runs.remove(&key)).collect();
    Ok(XpDay { day, total, detailed: !runs.is_empty(), runs })
}

/// A day's total, for the history chart.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDay {
    pub day: i64,
    pub xp: i64,
    pub completed: bool,
    pub frozen: bool,
}

/// Every day with XP, or done or frozen, in the last `days` days.
pub fn history(conn: &mut Conn, days: i64) -> Result<Vec<HistoryDay>> {
    let today = db::today(conn);
    Ok(day_totals::table
        .filter(day_totals::day.gt(today - days.clamp(1, 3660)))
        .order(day_totals::day)
        .select((day_totals::day, day_totals::xp, day_totals::completed, day_totals::frozen))
        .load::<(i64, i64, i64, i64)>(conn)?
        .into_iter()
        .map(|(day, xp, completed, frozen)| HistoryDay { day, xp, completed: completed == 1, frozen: frozen == 1 })
        .collect())
}

#[tauri::command]
pub async fn xp_day(db: State<'_, Db>, day: i64) -> Result<XpDay> {
    self::day(&mut db.lock(), day)
}

#[tauri::command]
pub async fn xp_history(db: State<'_, Db>, days: i64) -> Result<Vec<HistoryDay>> {
    history(&mut db.lock(), days)
}
