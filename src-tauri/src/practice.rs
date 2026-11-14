//! Doing a block: building its exercises, checking answers, finishing a
//! drill, and the results screen after a drill or a check-in.

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::blocks;
use crate::db::{self, Conn, Db};
use crate::schema::{achievements, day_log};
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
    /// Extra practice on a block that is not due: half XP.
    #[serde(default)]
    pub extra: bool,
}

/// Cards that are only read (a new word, a new sentence, a dialogue to read
/// through): nothing is answered, so nothing is scored.
pub const READING_KINDS: &[&str] = &["intro", "sentenceIntro", "dialogueRead"];

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
    submit_answer_in(&mut db.lock(), answer)
}

/// The work behind [`submit_answer`], callable without Tauri (tests).
pub fn submit_answer_in(conn: &mut Conn, answer: SubmitAnswer) -> Result<AnswerResult> {
    let today = db::today(conn);

    // Reading a card is not answering it: no XP, no review, not counted in
    // accuracy. (Seeing a new word must not move it toward "mastered".)
    if READING_KINDS.contains(&answer.kind.as_str()) {
        let total_xp = stats::total_xp(conn)?;
        let day_xp = day_log::table.find(today).select(day_log::xp).first(conn).optional()?.unwrap_or(0);
        return Ok(AnswerResult {
            correct: true,
            expected: answer.expected,
            xp_gained: 0,
            total_xp,
            day_xp,
            level: stats::level_from_xp(total_xp),
        });
    }

    let correct = answers_match(&answer.expected, &answer.given);
    let grade = answer
        .grade
        .unwrap_or(if correct { srs::GOOD } else { srs::AGAIN })
        .clamp(srs::AGAIN, srs::EASY);

    // Conversation gaps exercise the dialogue rather than a scheduled item, so
    // they earn XP without creating review rows.
    if answer.item_kind == "word" || answer.item_kind == "sentence" {
        let state = srs::load(conn, &answer.item_kind, answer.item_id)?
            .unwrap_or_else(|| srs::ReviewState::fresh(today));
        let next = srs::grade(state, grade, today);
        srs::save(conn, &answer.item_kind, answer.item_id, &next)?;
    }

    let mut xp = stats::xp_for_answer(&answer.kind, correct, answer.is_new);
    if answer.extra {
        xp = (xp + 1) / 2;
    }
    stats::record_answer(conn, today, xp, correct)?;

    let total_xp = stats::total_xp(conn)?;
    let day_xp = day_log::table.find(today).select(day_log::xp).first(conn).optional()?.unwrap_or(0);

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
fn finish_up(conn: &mut Conn, before: &std::collections::HashSet<String>, first: bool) -> Result<SessionResult> {
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

fn unlocked_codes(conn: &mut Conn) -> Result<std::collections::HashSet<String>> {
    Ok(achievements::table.select(achievements::code).load::<String>(conn)?.into_iter().collect())
}

/// The exercises of a drill block.
#[tauri::command]
pub async fn block_session(db: State<'_, Db>, block_id: i64) -> Result<SessionPlan> {
    session::build_block(&mut db.lock(), block_id)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockFinished {
    pub block: blocks::Block,
    /// "pass" | "partial" | "fail"
    pub result: String,
    /// Extra practice on a block that was not due: its schedule is unchanged.
    pub extra: bool,
    #[serde(flatten)]
    pub session: SessionResult,
}

/// A drill is done: its score becomes a result that schedules the next
/// check-in, and the day counts toward the streak.
#[tauri::command]
pub async fn block_finish(db: State<'_, Db>, block_id: i64, correct: i64, total: i64) -> Result<BlockFinished> {
    block_finish_in(&mut db.lock(), block_id, correct, total)
}

/// The work behind [`block_finish`], callable without Tauri (tests).
pub fn block_finish_in(conn: &mut Conn, block_id: i64, correct: i64, total: i64) -> Result<BlockFinished> {
    let before = unlocked_codes(conn)?;
    let result = blocks::result_from_score(correct, total);
    let today = db::today(conn);
    // Only a block that is due (or new) moves its schedule. Practising one
    // early is extra practice: it counts for the day, but a pass must not
    // push the next check-in further out — three quick replays would
    // otherwise send it months away.
    let current = blocks::load(conn, block_id)?;
    let extra = blocks::is_extra_practice(&current, today);
    let block = if extra { current } else { blocks::record(conn, block_id, result)? };
    let first = stats::mark_practised(conn, today)?;
    Ok(BlockFinished { block, result: result.into(), extra, session: finish_up(conn, &before, first)? })
}

/// Stats after a check-in, for the summary screen. What counts as newly
/// unlocked is what was unlocked since the check-in began.
#[tauri::command]
pub async fn checkin_summary(db: State<'_, Db>, checkin_id: i64, first_completion_today: bool) -> Result<SessionResult> {
    let mut guard = db.lock();
    checkin_summary_in(&mut guard, checkin_id, first_completion_today)
}

pub fn checkin_summary_in(conn: &mut Conn, checkin_id: i64, first_completion_today: bool) -> Result<SessionResult> {
    use crate::schema::checkins;
    let started: i64 = checkins::table.find(checkin_id).select(checkins::started_at).first(conn)?;
    let before: std::collections::HashSet<String> = achievements::table
        .filter(achievements::unlocked_at.lt(started))
        .select(achievements::code)
        .load::<String>(conn)?
        .into_iter()
        .collect();
    finish_up(conn, &before, first_completion_today)
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
