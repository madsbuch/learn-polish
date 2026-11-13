//! The examiner: checks a study block by interviewing the learner.
//!
//! It asks one question at a time — more, and harder ones, the more complex
//! the block — gives a line of feedback after each answer, and ends by calling
//! `finish_assessment` with a verdict. The verdict schedules the block's next
//! check-in exactly as a drill score does.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

use crate::ai::{self, Wire};
use crate::blocks;
use crate::db::{self, Db};
use crate::error::{Error, Result};
use crate::stats;

/// How many questions to ask, by complexity.
fn question_plan(complexity: &str) -> &'static str {
    match complexity {
        "low" => "2–3 short questions",
        "medium" => "4–6 questions, at least two asking the learner to produce Polish",
        _ => {
            "7–10 questions that build up: recognise, then produce, then apply the rule to a \
             new case, and at least one asking the learner to explain *why*"
        }
    }
}

fn system_prompt(conn: &Connection, block: &blocks::BlockDetail) -> Result<String> {
    let previous: Option<String> = conn
        .query_row(
            "SELECT feedback FROM checkins WHERE block_id = ?1 AND finished_at IS NOT NULL
             ORDER BY id DESC LIMIT 1",
            [block.block.id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    let song = match block.block.song_id {
        Some(id) => Some(crate::songs::load_song(conn, id)?),
        None => None,
    };
    let listening = song
        .map(|song| {
            let lyrics: Vec<String> = song
                .sections
                .iter()
                .map(|section| {
                    let lines: Vec<String> = section
                        .lines
                        .iter()
                        .map(|l| if l.en.is_empty() { l.pl.clone() } else { format!("{} — {}", l.pl, l.en) })
                        .collect();
                    format!("[{}]\n{}", section.kind, lines.join("\n"))
                })
                .collect();
            format!(
                "\nThis is a LISTENING block. The learner has just listened to the song \"{}\" and can \
                 replay it during the check-in. Ask about what they heard, not about the lesson \
                 note: what the chorus means, which word fills a gap in a line you quote with one \
                 word blanked, what a line means, what happens in the song, and — for harder \
                 blocks — why a word has the form it has. Quote at most one short line per \
                 question; never paste the lyrics.\nThe lyrics (Polish — English):\n{}\n",
                song.title,
                lyrics.join("\n\n")
            )
        })
        .unwrap_or_default();
    Ok(format!(
        "You are a friendly examiner in Polski, an app where an English speaker learns Polish. \
         You check whether the learner has understood one study block, by interviewing them.

How to run the check-in:
- Ask {plan}. One question per message — never several at once.
- Write in English; ask for answers in Polish where the objective is about Polish.
- After each answer, give one short line of feedback (right, or the correction and why), \
  then ask the next question in the same message.
- Accept Polish written without diacritics (\"czesc\" for \"cześć\"); mention the correct \
  spelling but don't count it as wrong.
- Cover every objective. Vary the questions: fill in a form, translate a short sentence, \
  pick the right option, explain a rule, use it in a new sentence.
- Don't reveal answers to questions you have not asked yet. Keep messages short.
- When you have enough evidence — or the learner asks to stop — call finish_assessment. \
  pass: every objective shown, at most small slips. partial: some objectives shaky. \
  fail: most not shown. Be honest; a pass that isn't earned helps nobody.

The block: {title} (complexity: {complexity})
Objectives:
{objectives}

The lesson the learner studied:
{lesson}
{listening}{previous}",
        plan = question_plan(&block.block.complexity),
        title = block.block.title,
        complexity = block.block.complexity,
        objectives = block.objectives.iter().map(|o| format!("- {o}")).collect::<Vec<_>>().join("\n"),
        lesson = block.lesson,
        listening = listening,
        previous = previous
            .map(|feedback| format!("\nLast check-in's verdict, to follow up on: {feedback}"))
            .unwrap_or_default(),
    ))
}

fn tools() -> serde_json::Value {
    json!([{
        "type": "function",
        "function": {
            "name": "finish_assessment",
            "description": "End the check-in with your verdict.",
            "parameters": {
                "type": "object",
                "properties": {
                    "result": { "type": "string", "enum": ["pass", "partial", "fail"] },
                    "summary": { "type": "string", "description": "Two or three sentences to the learner: how it went." },
                    "strengths": { "type": "array", "items": { "type": "string" } },
                    "to_review": { "type": "array", "items": { "type": "string" }, "description": "What to go over again, concretely." }
                },
                "required": ["result", "summary"]
            }
        }
    }])
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    pub result: String,
    pub summary: String,
    #[serde(default)]
    pub strengths: Vec<String>,
    #[serde(default, alias = "to_review")]
    pub to_review: Vec<String>,
    /// Filled in when recorded.
    #[serde(default)]
    pub next_due_day: i64,
    #[serde(default)]
    pub xp: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinTurn {
    pub checkin_id: i64,
    /// What the examiner just said (may be empty when it only finished).
    pub reply: String,
    pub assessment: Option<Assessment>,
    /// True when the day's streak was earned by this check-in.
    pub first_completion_today: bool,
}

fn history(conn: &Connection, checkin_id: i64) -> Result<Vec<Wire>> {
    let mut stmt =
        conn.prepare("SELECT role, content FROM checkin_messages WHERE checkin_id = ?1 ORDER BY id")?;
    let rows = stmt
        .query_map([checkin_id], |r| {
            let role: String = r.get(0)?;
            let content: String = r.get(1)?;
            Ok(if role == "user" { Wire::user(content) } else { Wire::assistant(content) })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn append(conn: &Connection, checkin_id: i64, role: &str, content: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO checkin_messages (checkin_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![checkin_id, role, content, db::now_secs()],
    )?;
    Ok(())
}

/// Records the verdict: schedules the block, awards XP and counts the day.
pub fn record(conn: &Connection, checkin_id: i64, mut assessment: Assessment) -> Result<(Assessment, bool)> {
    let block_id: i64 =
        conn.query_row("SELECT block_id FROM checkins WHERE id = ?1", [checkin_id], |r| r.get(0))?;
    if !["pass", "partial", "fail"].contains(&assessment.result.as_str()) {
        assessment.result = "partial".into();
    }
    let block = blocks::record(conn, block_id, &assessment.result)?;
    let today = db::today(conn);
    assessment.xp = stats::xp_for_checkin(&block.complexity, &assessment.result);
    assessment.next_due_day = block.due_day.unwrap_or(today);
    stats::record_answer(conn, today, assessment.xp, assessment.result != "fail")?;
    let first = stats::mark_practised(conn, today)?;
    let mut feedback = assessment.summary.clone();
    if !assessment.to_review.is_empty() {
        feedback.push_str(&format!(" To review: {}.", assessment.to_review.join("; ")));
    }
    conn.execute(
        "UPDATE checkins SET finished_at = ?2, result = ?3, feedback = ?4 WHERE id = ?1",
        rusqlite::params![checkin_id, db::now_secs(), assessment.result, feedback],
    )?;
    Ok((assessment, first))
}

async fn turn(
    app: &tauri::AppHandle,
    db: &Db,
    checkin_id: i64,
    user: Option<String>,
    stream_id: &str,
) -> Result<CheckinTurn> {
    let (key, model, wire) = {
        let conn = db.lock();
        let (block_id, finished): (i64, Option<i64>) = conn.query_row(
            "SELECT block_id, finished_at FROM checkins WHERE id = ?1",
            [checkin_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if finished.is_some() {
            return Err(Error::msg("This check-in is already finished."));
        }
        let detail = blocks::detail(&conn, block_id)?;
        if let Some(text) = &user {
            append(&conn, checkin_id, "user", text)?;
        }
        let mut wire = vec![Wire::system(system_prompt(&conn, &detail)?)];
        let past = history(&conn, checkin_id)?;
        if past.is_empty() {
            wire.push(Wire::user("I'm ready for the check-in."));
        }
        wire.extend(past);
        (ai::api_key(&conn)?, ai::require_model(&conn)?, wire)
    };

    let emitter = app.clone();
    let id = stream_id.to_string();
    let tools = tools();
    let result = ai::stream_turn(&key, &model, &wire, Some(&tools), |delta| {
        crate::commands::stream_to_frontend(&emitter, &id, Some(delta), false, None);
    })
    .await;
    let finished_stream = |error: Option<String>| {
        crate::commands::stream_to_frontend(app, stream_id, None, true, error);
    };
    let turn = match result {
        Ok(turn) => turn,
        Err(error) => {
            finished_stream(Some(error.to_string()));
            return Err(error);
        }
    };
    finished_stream(None);

    let conn = db.lock();
    let _ = crate::costs::record(&conn, crate::costs::EXAMINER, "openrouter", &model, turn.cost, "");
    let reply = turn.text.trim().to_string();
    if !reply.is_empty() {
        append(&conn, checkin_id, "assistant", &reply)?;
    }
    let call = turn.tool_calls.iter().find(|c| c.function.name == "finish_assessment");
    let (assessment, first) = match call {
        Some(call) => {
            let parsed: Assessment = serde_json::from_str(&call.function.arguments)
                .map_err(|e| Error::msg(format!("The examiner's verdict was unreadable: {e}")))?;
            let (assessment, first) = record(&conn, checkin_id, parsed)?;
            (Some(assessment), first)
        }
        None => (None, false),
    };
    if reply.is_empty() && assessment.is_none() {
        return Err(Error::msg("The examiner did not answer. Try again."));
    }
    Ok(CheckinTurn { checkin_id, reply, assessment, first_completion_today: first })
}

/// Starts a check-in on a study block; the examiner asks the first question.
#[tauri::command]
pub async fn checkin_start(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    block_id: i64,
    stream_id: String,
) -> Result<CheckinTurn> {
    let checkin_id = {
        let conn = db.lock();
        let block = blocks::load(&conn, block_id)?;
        if block.kind != "study" {
            return Err(Error::msg("Only study blocks have an interview."));
        }
        conn.execute(
            "INSERT INTO checkins (block_id, started_at) VALUES (?1, ?2)",
            rusqlite::params![block_id, db::now_secs()],
        )?;
        conn.last_insert_rowid()
    };
    turn(&app, &db, checkin_id, None, &stream_id).await
}

/// Answers the examiner.
#[tauri::command]
pub async fn checkin_send(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    checkin_id: i64,
    message: String,
    stream_id: String,
) -> Result<CheckinTurn> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err(Error::msg("Write an answer first."));
    }
    turn(&app, &db, checkin_id, Some(message), &stream_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harder_blocks_get_longer_interviews() {
        assert!(question_plan("low").starts_with("2–3"));
        assert!(question_plan("medium").starts_with("4–6"));
        assert!(question_plan("high").contains("why"));
    }

    #[test]
    fn verdicts_parse_in_either_spelling() {
        let a: Assessment =
            serde_json::from_str(r#"{"result":"pass","summary":"Good.","to_review":["sa vs jest"]}"#).unwrap();
        assert_eq!(a.to_review, vec!["sa vs jest"]);
        let b: Assessment = serde_json::from_str(r#"{"result":"fail","summary":"x","toReview":[]}"#).unwrap();
        assert_eq!(b.result, "fail");
    }
}
