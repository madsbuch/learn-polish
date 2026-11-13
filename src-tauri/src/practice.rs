//! Doing a block: building its exercises, checking answers, finishing a
//! drill, and the results screen after a drill or a check-in.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::blocks;
use crate::db::{self, Db};
use crate::error::Result;
use crate::models::*;
use crate::session;
use crate::srs;
use crate::stats;

// ---------------------------------------------------------------------------
// Answer checking
// ---------------------------------------------------------------------------

/// Folds away everything a learner should not be penalised for: case,
/// punctuation, doubled spaces and missing Polish diacritics. Typing "czesc"
/// counts as "cześć" — the correct spelling is shown in the feedback anyway.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_space = true;
    for ch in text.trim().chars() {
        let folded = match ch.to_lowercase().next().unwrap_or(ch) {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            other => other,
        };
        if folded.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else if folded.is_alphanumeric() {
            out.push(folded);
            last_was_space = false;
        }
        // Punctuation is dropped entirely.
    }
    out.trim_end().to_string()
}

fn answers_match(expected: &str, given: &str) -> bool {
    let given = normalize(given);
    if normalize(expected).is_empty() {
        return true;
    }
    if normalize(expected) == given {
        return true;
    }
    // Glosses often carry alternatives ("piece, bit"); any one of them will do.
    // The split happens before normalising, which strips the separator.
    expected
        .split(',')
        .map(normalize)
        .any(|alternative| !alternative.is_empty() && alternative == given)
}

// ---------------------------------------------------------------------------
// Answers and results
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitAnswer {
    pub kind: String,
    pub item_kind: String,
    pub item_id: i64,
    pub is_new: bool,
    pub expected: String,
    pub given: String,
    /// Optional override; normally derived from whether the answer was right.
    pub grade: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerResult {
    pub correct: bool,
    pub expected: String,
    pub xp_gained: i64,
    pub total_xp: i64,
    pub day_xp: i64,
    pub level: i64,
}

#[tauri::command]
pub async fn submit_answer(db: State<'_, Db>, answer: SubmitAnswer) -> Result<AnswerResult> {
    let conn = db.lock();
    let today = db::today(&conn);

    let correct = answers_match(&answer.expected, &answer.given);
    let grade = answer
        .grade
        .unwrap_or(if correct { srs::GOOD } else { srs::AGAIN })
        .clamp(srs::AGAIN, srs::EASY);

    // Conversation gaps exercise the dialogue rather than a scheduled item, so
    // they earn XP without creating review rows.
    if answer.item_kind == "word" || answer.item_kind == "sentence" {
        let state = srs::load(&conn, &answer.item_kind, answer.item_id)?
            .unwrap_or_else(|| srs::ReviewState::fresh(today));
        let next = srs::grade(state, grade, today);
        srs::save(&conn, &answer.item_kind, answer.item_id, &next)?;
    }

    let xp = stats::xp_for_answer(&answer.kind, correct, answer.is_new);
    stats::record_answer(&conn, today, xp, correct)?;

    let total_xp = stats::total_xp(&conn)?;
    let day_xp = conn
        .query_row("SELECT xp FROM day_log WHERE day = ?1", [today], |r| r.get(0))
        .unwrap_or(0);

    Ok(AnswerResult {
        correct,
        expected: answer.expected,
        xp_gained: xp,
        total_xp,
        day_xp,
        level: stats::level_from_xp(total_xp),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResult {
    pub first_completion_today: bool,
    pub newly_unlocked: Vec<Achievement>,
    pub stats: Stats,
}

/// Achievements unlocked by whatever just happened, with fresh stats.
fn finish_up(conn: &Connection, before: &std::collections::HashSet<String>, first: bool) -> Result<SessionResult> {
    // collect() writes newly earned achievements as a side effect.
    let stats = stats::collect(conn)?;
    let newly_unlocked = stats
        .achievements
        .iter()
        .filter(|a| a.unlocked && !before.contains(&a.code))
        .cloned()
        .collect();
    Ok(SessionResult { first_completion_today: first, newly_unlocked, stats })
}

fn unlocked_codes(conn: &Connection) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare("SELECT code FROM achievements")?;
    let codes = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(codes)
}

/// The exercises of a drill block.
#[tauri::command]
pub async fn block_session(db: State<'_, Db>, block_id: i64) -> Result<SessionPlan> {
    session::build_block(&db.lock(), block_id)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockFinished {
    pub block: blocks::Block,
    /// "pass" | "partial" | "fail"
    pub result: String,
    #[serde(flatten)]
    pub session: SessionResult,
}

/// A drill is done: its score becomes a result that schedules the next
/// check-in, and the day counts toward the streak.
#[tauri::command]
pub async fn block_finish(db: State<'_, Db>, block_id: i64, correct: i64, total: i64) -> Result<BlockFinished> {
    let conn = db.lock();
    let before = unlocked_codes(&conn)?;
    let result = blocks::result_from_score(correct, total);
    let block = blocks::record(&conn, block_id, result)?;
    let first = stats::mark_practised(&conn, db::today(&conn))?;
    Ok(BlockFinished { block, result: result.into(), session: finish_up(&conn, &before, first)? })
}

/// Stats after a study check-in, for the summary screen.
#[tauri::command]
pub async fn checkin_summary(db: State<'_, Db>, first_completion_today: bool) -> Result<SessionResult> {
    let conn = db.lock();
    finish_up(&conn, &Default::default(), first_completion_today)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_is_forgiving_about_diacritics_and_case() {
        assert!(answers_match("cześć", "Czesc"));
        assert!(answers_match("dzień dobry", "  dzien   dobry "));
        assert!(answers_match("Do widzenia!", "do widzenia"));
        assert!(!answers_match("kot", "pies"));
    }

    #[test]
    fn any_listed_gloss_counts() {
        assert!(answers_match("piece, bit", "bit"));
        assert!(answers_match("piece, bit", "piece"));
        assert!(answers_match("piece, bit", "piece, bit"));
        assert!(!answers_match("piece, bit", "chunk"));
    }
}
