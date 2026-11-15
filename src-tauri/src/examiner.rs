//! The examiner: checks a study block by interviewing the learner.
//!
//! It asks one question at a time — more, and harder ones, the more complex
//! the block — gives a line of feedback after each answer, and ends by calling
//! `finish_assessment` with a verdict. The verdict schedules the block's next
//! check-in exactly as a drill score does.
//!
//! A writing block is checked differently: the agent is not an examiner but
//! the other person in a situation — a neighbour, a waiter — writing only in
//! Polish and understanding only Polish. It runs on the partner model (small
//! and fast is fine) and gives its verdict as soon as the learner has done
//! what the block asks, or when they end the conversation.

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

use crate::agent::{tool, Agent, Tool, ToolOutput};
use crate::ai::Wire;
use crate::blocks;
use crate::db::{self, Conn, Db};
use crate::schema::{checkin_messages, checkins};
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

pub(crate) fn system_prompt(conn: &mut Conn, block: &blocks::BlockDetail) -> Result<String> {
    let previous = previous_feedback(conn, block.block.id)?;
    // A lesson built on Wikipedia: its sources, to keep questions to what
    // they say — and facts may be answered in the learner's own language.
    let reading = crate::wiki::reading_for(conn, block.block.id, 6000)?;
    let sources = if reading.is_empty() {
        String::new()
    } else {
        format!(
            "\nThe lesson is built on these sources. Ask only about what the lesson and they say. \
             Facts may be answered in English (or the learner's language); add one small Polish \
             step to each check-in — a sentence with the place or word in it.\n{reading}\n"
        )
    };
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
{listening}{sources}{previous}{memory}",
        plan = question_plan(&block.block.complexity),
        title = block.block.title,
        complexity = block.block.complexity,
        objectives = block.objectives.iter().map(|o| format!("- {o}")).collect::<Vec<_>>().join("\n"),
        lesson = block.lesson,
        listening = listening,
        sources = sources,
        previous = previous
            .map(|feedback| format!("\nLast check-in's verdict, to follow up on: {feedback}"))
            .unwrap_or_default(),
        memory = memory_section(conn)?,
    ))
}

/// How much the partner helps, by the block's support level (it moves with
/// the learner's results; see [`next_support_level`]).
fn support_rules(level: i64) -> &'static str {
    match level {
        0 => "- The learner is a beginner, writing with a word bank and hints beside the chat. Take \
  the goals one at a time and ask a simple, direct question that invites each (\"Jakie pokoje \
  masz?\"). A short answer — even two or three words — that gets a goal across counts. Be \
  warm and patient; if they are stuck, ask the question again more simply. Using the word bank \
  and hints is expected at this stage: it does not make a pass partial.",
        1 => "- The learner has hints if they need them. Ask questions that lead toward the goals, \
  but let them say more on their own.",
        _ => "- The learner writes on their own: let the conversation flow naturally; steer \
  only when a goal has not come up.",
    }
}

/// The support level after a writing check-in: a pass takes away a layer of
/// help next time, a fail adds one back, a partial stays.
pub fn next_support_level(level: i64, result: &str) -> i64 {
    match result {
        "pass" => (level + 1).min(2),
        "fail" => (level - 1).max(0),
        _ => level,
    }
}

/// How simply the partner writes, by complexity.
fn partner_level(complexity: &str) -> &'static str {
    match complexity {
        "low" => "A1: very short, simple sentences, the most common words, present tense",
        "medium" => "A2: short everyday sentences; past and future are fine",
        _ => "B1: natural everyday Polish, as you would write to a foreigner who speaks it reasonably",
    }
}

/// What the app tells the partner when the learner ends the conversation.
const LEARNER_ENDED: &str = "(The app: the learner has ended the conversation. Do not reply in \
     character — call finish_assessment now, judging only what they wrote.)";
const OPENING: &str = "(The app: the learner has opened the conversation. Begin it, in character, in Polish.)";

pub(crate) fn writing_prompt(conn: &mut Conn, block: &blocks::BlockDetail) -> Result<String> {
    Ok(format!(
        "You play a character in Polski, an app where an English speaker learns Polish by \
         writing. This is a role-play in writing, in Polish only.

The situation (the learner has read it):
{scenario}

What the learner has to get across — the goals:
{goals}

How to play it:
- Stay in character as the other person in the situation. Write only in Polish, at level \
  {level}. One or two sentences per message, like a chat. Never translate, never explain \
  grammar, never switch to English.
{support}
- You understand only Polish. If the learner writes in English or any other language — even a \
  single word of it in place of Polish — you don't understand that part: say so in Polish, \
  in character (\"Przepraszam, nie rozumiem…\", \"Co to znaczy?\") and wait for Polish. \
  Polish without diacritics (\"czesc\") is fine; so are small mistakes when the meaning is clear.
- When the meaning is unclear, ask in Polish what they mean, as a real person would. Don't \
  correct their mistakes in the conversation.
- Steer the conversation so the learner gets the chance to reach every goal — ask the \
  question that invites it — but never say it for them or put words in their mouth.
- As soon as the learner has got every goal across in understandable Polish, reply once in \
  character to close the exchange naturally, and in the same message call finish_assessment.
  pass: every goal reached in Polish. partial: reached, but only with a lot of help, or some \
  goals only half there. fail: the goals not reached when the learner ended it.
  In the verdict (in English), summary says how it went; to_review lists their actual \
  mistakes as \"what they wrote → correct Polish\" — the conversation itself corrected nothing.
- Messages starting with \"(The app:\" come from the app, not the learner: follow them.
{previous}",
        scenario = block.lesson,
        goals = block.objectives.iter().map(|o| format!("- {o}")).collect::<Vec<_>>().join("\n"),
        level = if block.block.support_level == 0 {
            "A1: very short, simple sentences, the most common words, present tense"
        } else {
            partner_level(&block.block.complexity)
        },
        support = support_rules(block.block.support_level),
        previous = previous_feedback(conn, block.block.id)?
            .map(|feedback| format!("\nLast time's verdict, for your judgement only: {feedback}"))
            .unwrap_or_default(),
    ))
}

fn previous_feedback(conn: &mut Conn, block_id: i64) -> Result<Option<String>> {
    Ok(checkins::table
        .filter(checkins::block_id.eq(block_id))
        .filter(checkins::finished_at.is_not_null())
        .order(checkins::id.desc())
        .select(checkins::feedback)
        .first::<Option<String>>(conn)
        .optional()?
        .flatten())
}

/// What the agents remember, plus what the examiner in particular should
/// save: what this check-in showed that will matter beyond this block.
fn memory_section(conn: &mut Conn) -> Result<String> {
    let section = crate::memories::prompt_section(conn)?;
    if section.is_empty() {
        return Ok(section);
    }
    Ok(format!(
        "{section}\n- As the examiner: use what you remember to aim questions at known weak spots \
         that touch this block. When you finish, save — in the same message as \
         finish_assessment — at most two memories about lasting patterns this check-in showed \
         (a recurring mistake, something now mastered), not this block's score. Update a \
         struggle that the learner has now clearly overcome."
    ))
}

fn tools() -> Vec<Tool> {
    vec![tool(
        "finish_assessment",
        "Weighing it up…",
        "End the check-in with your verdict.",
        json!({
            "type": "object",
            "properties": {
                "result": { "type": "string", "enum": ["pass", "partial", "fail"] },
                "summary": { "type": "string", "description": "Two or three sentences to the learner: how it went." },
                "strengths": { "type": "array", "items": { "type": "string" } },
                "to_review": { "type": "array", "items": { "type": "string" }, "description": "What to go over again, concretely." }
            },
            "required": ["result", "summary"]
        }),
    )]
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
    /// What the examiner saved to or changed in long-term memory this turn.
    pub remembered: Vec<String>,
}

/// Rounds per turn: memory calls, then the question or verdict.
const MAX_ROUNDS: usize = 3;

fn history(conn: &mut Conn, checkin_id: i64) -> Result<Vec<Wire>> {
    Ok(checkin_messages::table
        .filter(checkin_messages::checkin_id.eq(checkin_id))
        .order(checkin_messages::id)
        .select((checkin_messages::role, checkin_messages::content))
        .load::<(String, String)>(conn)?
        .into_iter()
        .map(|(role, content)| if role == "user" { Wire::user(content) } else { Wire::assistant(content) })
        .collect())
}

fn append(conn: &mut Conn, checkin_id: i64, role: &str, content: &str) -> Result<()> {
    diesel::insert_into(checkin_messages::table)
        .values((
            checkin_messages::checkin_id.eq(checkin_id),
            checkin_messages::role.eq(role),
            checkin_messages::content.eq(content),
            checkin_messages::created_at.eq(db::now_secs()),
        ))
        .execute(conn)?;
    Ok(())
}

/// Records the verdict: schedules the block, awards XP and counts the day.
pub fn record(conn: &mut Conn, checkin_id: i64, mut assessment: Assessment) -> Result<(Assessment, bool)> {
    let block_id: i64 = checkins::table.find(checkin_id).select(checkins::block_id).first(conn)?;
    if !["pass", "partial", "fail"].contains(&assessment.result.as_str()) {
        assessment.result = "partial".into();
    }
    let today = db::today(conn);
    // As with drills, only a due (or new) block moves its schedule; checking
    // in early is extra practice at half XP.
    let current = blocks::load(conn, block_id)?;
    let extra = blocks::is_extra_practice(&current, today);
    // A pass with nothing left to review is an ace: the gap grows faster.
    let ace = assessment.result == "pass" && assessment.to_review.is_empty();
    let block = if extra { current } else { blocks::record_graded(conn, block_id, &assessment.result, ace)? };
    let writing_goals = if block.format == "writing" { blocks::detail(conn, block_id)?.objectives.len() } else { 0 };
    if block.format == "writing" {
        // Help follows how it went, even on a replay the same day.
        diesel::update(crate::schema::blocks::table.find(block_id))
            .set(crate::schema::blocks::support_level.eq(next_support_level(block.support_level, &assessment.result)))
            .execute(conn)?;
    }
    assessment.xp = stats::xp_for_checkin(&block.complexity, &assessment.result, writing_goals);
    if extra {
        assessment.xp = (assessment.xp + 1) / 2;
    }
    assessment.next_due_day = block.due_day.unwrap_or(today);
    // A verdict is not an answer: it counts toward the day, not accuracy.
    stats::add_xp(conn, today, assessment.xp)?;
    let first = stats::mark_practised(conn, today)?;
    let mut feedback = assessment.summary.clone();
    if !assessment.to_review.is_empty() {
        feedback.push_str(&format!(" To review: {}.", assessment.to_review.join("; ")));
    }
    diesel::update(checkins::table.find(checkin_id))
        .set((
            checkins::finished_at.eq(db::now_secs()),
            checkins::result.eq(&assessment.result),
            checkins::feedback.eq(feedback),
        ))
        .execute(conn)?;
    Ok((assessment, first))
}

async fn turn(
    app: &tauri::AppHandle,
    db: &Db,
    checkin_id: i64,
    user: Option<String>,
    stream_id: &str,
) -> Result<CheckinTurn> {
    let (wire, writing, model) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let (block_id, finished): (i64, Option<i64>) =
            checkins::table.find(checkin_id).select((checkins::block_id, checkins::finished_at)).first(conn)?;
        if finished.is_some() {
            return Err(Error::msg("This check-in is already finished."));
        }
        let detail = blocks::detail(conn, block_id)?;
        if let Some(text) = &user {
            append(conn, checkin_id, "user", text)?;
        }
        let writing = detail.block.format == "writing";
        let (prompt, opening, model) = if writing {
            (writing_prompt(conn, &detail)?, OPENING, Some(crate::ai::partner_model(conn)?))
        } else {
            (system_prompt(conn, &detail)?, "I'm ready for the check-in.", None)
        };
        let mut wire = vec![Wire::system(crate::ai::with_say_player(prompt))];
        let past = history(conn, checkin_id)?;
        if past.is_empty() {
            wire.push(Wire::user(opening));
        }
        wire.extend(past);
        (wire, writing, model)
    };

    let agent = Agent {
        app,
        db,
        name: if writing { crate::costs::PARTNER } else { crate::costs::EXAMINER },
        stream_id,
        tools: Some(tools()),
        max_rounds: MAX_ROUNDS,
        // One question per turn. Memory calls alone are not an answer, so after
        // them the examiner gets another round to say something.
        stop_once_answered: true,
        show_status: false,
        model,
    };
    let outcome = agent
        .run(wire, |call| {
            Box::pin(async move {
                if call.function.name != "finish_assessment" {
                    return ToolOutput::reply(json!({ "error": "unknown tool" }));
                }
                match serde_json::from_str::<Assessment>(&call.function.arguments) {
                    Ok(verdict) => ToolOutput { result: json!({ "recorded": true }), extra: Some(verdict), ends_turn: true },
                    Err(e) => ToolOutput::reply(json!({ "error": format!("The verdict was unreadable: {e}. Call it again.") })),
                }
            })
        })
        .await;
    if let Some(error) = outcome.failed {
        return Err(error);
    }
    let reply = outcome.text;
    let remembered = outcome.remembered;
    let verdict = outcome.extras.into_iter().next();

    let mut guard = db.lock();

    let conn = &mut *guard;
    if !reply.is_empty() {
        append(conn, checkin_id, "assistant", &reply)?;
    }
    let (assessment, first) = match verdict {
        Some(parsed) => {
            let (assessment, first) = record(conn, checkin_id, parsed)?;
            (Some(assessment), first)
        }
        None => (None, false),
    };
    if reply.is_empty() && assessment.is_none() {
        return Err(Error::msg("The examiner did not answer. Try again."));
    }
    Ok(CheckinTurn { checkin_id, reply, assessment, first_completion_today: first, remembered })
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
        let mut guard = db.lock();
        let conn = &mut *guard;
        let block = blocks::load(conn, block_id)?;
        if block.kind != "study" {
            return Err(Error::msg("Only study blocks have an interview."));
        }
        diesel::insert_into(checkins::table)
            .values((checkins::block_id.eq(block_id), checkins::started_at.eq(db::now_secs())))
            .returning(checkins::id)
            .get_result::<i64>(conn)?
    };
    turn(&app, &db, checkin_id, None, &stream_id).await
}

/// Ends a writing conversation early: the partner gives its verdict on what
/// was written so far.
#[tauri::command]
pub async fn checkin_end(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    checkin_id: i64,
    stream_id: String,
) -> Result<CheckinTurn> {
    turn(&app, &db, checkin_id, Some(LEARNER_ENDED.to_string()), &stream_id).await
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

// ---------------------------------------------------------------------------
// Players in a lesson
// ---------------------------------------------------------------------------

/// Text compared for "nothing but the tags changed": players reduced to
/// their words, whitespace ignored.
fn bare(text: &str) -> String {
    crate::ai::unsay(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Puts audio players ([[say: …]]) around the Polish examples of a lesson
/// written before lessons had them. A small model does the tagging; the
/// result is kept only if, with the tags taken out, the lesson is exactly
/// what it was — so nothing else can change.
#[tauri::command]
pub async fn lesson_add_players(db: State<'_, Db>, block_id: i64) -> Result<blocks::BlockDetail> {
    let (lesson, key, model) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let detail = blocks::detail(conn, block_id)?;
        (detail.lesson, crate::ai::api_key(conn)?, crate::ai::partner_model(conn)?)
    };
    if lesson.contains("[[say:") {
        return Ok(blocks::detail(&mut db.lock(), block_id)?);
    }
    let prompt = format!(
        "Below is a Polish lesson in markdown. Wrap every Polish word, phrase or example sentence in \
         [[say: …]] — for example **Poproszę bułkę.** becomes **[[say: Poproszę bułkę.]]** — so the \
         learner can hear it. Change nothing else at all: not one character of the text, the \
         English, the markdown or the line breaks. Leave English, single letters and word endings \
         (like -ę) untagged, and never tag inside a table header. Reply with only the lesson.\n\n{lesson}"
    );
    let reply = crate::ai::complete_with(&key, &model, &[Wire::user(prompt)], 0.0, 8000).await?;
    let tagged = reply.text.trim().trim_start_matches("```markdown").trim_start_matches("```").trim_end_matches("```").trim().to_string();
    let _ = crate::costs::record_tokens(&mut db.lock(), crate::costs::PARTNER, &model, reply.cost, "lesson players", reply.tokens);
    if bare(&tagged) != bare(&lesson) {
        return Err(Error::msg("The lesson came back changed, so it was left as it was. Try again, or with another model."));
    }
    if !tagged.contains("[[say:") {
        return Err(Error::msg("No Polish was found to add players to."));
    }
    let mut guard = db.lock();
    blocks::update(&mut guard, block_id, blocks::BlockChanges { lesson: Some(tagged), ..Default::default() })?;
    blocks::detail(&mut guard, block_id)
}

// ---------------------------------------------------------------------------
// Help for writing: a word bank, and a hint when stuck
// ---------------------------------------------------------------------------

/// A phrase with its meaning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phrase {
    pub pl: String,
    pub en: String,
}

/// What helps with one goal: a way to start, and words to use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalHelp {
    pub goal: String,
    pub starter: Phrase,
    #[serde(default)]
    pub words: Vec<Phrase>,
}

/// The JSON object in a model's reply (it may wrap it in prose or fences).
fn json_in(text: &str) -> Option<&str> {
    let (start, end) = (text.find('{')?, text.rfind('}')?);
    (start < end).then(|| &text[start..=end])
}

/// A writing block's word bank: for each goal, a sentence starter and a few
/// words to use. Made once by the partner's model, then kept.
#[tauri::command]
pub async fn writing_scaffold(db: State<'_, Db>, block_id: i64) -> Result<Vec<GoalHelp>> {
    let (detail, stored, key, model) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let stored: Option<String> = crate::schema::blocks::table
            .find(block_id)
            .select(crate::schema::blocks::scaffold)
            .first(conn)?;
        (blocks::detail(conn, block_id)?, stored, crate::ai::api_key(conn)?, crate::ai::partner_model(conn)?)
    };
    if let Some(help) = stored.and_then(|json| serde_json::from_str::<Vec<GoalHelp>>(&json).ok()) {
        return Ok(help);
    }
    let prompt = format!(
        "A beginner learning Polish is about to write a short chat in Polish. The situation:\n{}\n\n\
         Their goals:\n{}\n\n\
         For each goal give a simple sentence starter they can finish, and 4–6 useful words or \
         short phrases (in the form they would use them in the sentence), each with its English \
         meaning. Simple, correct, everyday Polish (A1). Reply with only JSON: \
         {{\"goals\": [{{\"goal\": \"…\", \"starter\": {{\"pl\": \"Mam…\", \"en\": \"I have…\"}}, \
         \"words\": [{{\"pl\": \"…\", \"en\": \"…\"}}]}}]}}",
        detail.lesson,
        detail.objectives.iter().map(|o| format!("- {o}")).collect::<Vec<_>>().join("\n"),
    );
    let reply = crate::ai::complete(&key, &model, &[Wire::user(prompt)]).await?;
    #[derive(Deserialize)]
    struct Bank {
        goals: Vec<GoalHelp>,
    }
    let bank: Bank = json_in(&reply.text)
        .and_then(|json| serde_json::from_str(json).ok())
        .ok_or_else(|| Error::msg("The word bank came back unreadable. Try again."))?;
    let mut guard = db.lock();
    let conn = &mut *guard;
    let _ = crate::costs::record_tokens(conn, crate::costs::PARTNER, &model, reply.cost, "word bank", reply.tokens);
    diesel::update(crate::schema::blocks::table.find(block_id))
        .set(crate::schema::blocks::scaffold.eq(serde_json::to_string(&bank.goals)?))
        .execute(conn)?;
    Ok(bank.goals)
}

/// A hint for what to write next in a writing conversation: a sentence with
/// a blank or two for a goal not reached yet, with its meaning. Not sent to
/// the partner.
#[tauri::command]
pub async fn writing_hint(db: State<'_, Db>, checkin_id: i64) -> Result<String> {
    let (detail, past, key, model) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let block_id: i64 = checkins::table.find(checkin_id).select(checkins::block_id).first(conn)?;
        let past: Vec<(String, String)> = checkin_messages::table
            .filter(checkin_messages::checkin_id.eq(checkin_id))
            .order(checkin_messages::id)
            .select((checkin_messages::role, checkin_messages::content))
            .load(conn)?;
        (blocks::detail(conn, block_id)?, past, crate::ai::api_key(conn)?, crate::ai::partner_model(conn)?)
    };
    let chat = past
        .iter()
        .filter(|(_, content)| !content.starts_with("(The app:"))
        .map(|(role, content)| format!("{}: {content}", if role == "user" { "Learner" } else { "Partner" }))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "A beginner is writing a chat in Polish. Goals:\n{}\n\nThe chat so far:\n{}\n\n\
         Give one hint for their next message: a short, simple Polish sentence that answers the \
         partner and moves toward a goal not reached yet, with one or two key words left as \
         blanks (___) for them to fill in, then the English meaning in parentheses, then the \
         missing words' English in brackets. One line, nothing else. Example: \
         Mam ___ i ___. (I have a kitchen and a bedroom.) [kitchen, bedroom]",
        detail.objectives.iter().map(|o| format!("- {o}")).collect::<Vec<_>>().join("\n"),
        if chat.is_empty() { "(nothing yet)".into() } else { chat },
    );
    let reply = crate::ai::complete(&key, &model, &[Wire::user(prompt)]).await?;
    let _ = crate::costs::record_tokens(&mut db.lock(), crate::costs::PARTNER, &model, reply.cost, "hint", reply.tokens);
    Ok(reply.text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_help_steps_down_as_the_learner_passes() {
        assert_eq!(next_support_level(0, "pass"), 1);
        assert_eq!(next_support_level(1, "pass"), 2);
        assert_eq!(next_support_level(2, "pass"), 2);
        assert_eq!(next_support_level(1, "partial"), 1);
        assert_eq!(next_support_level(1, "fail"), 0);
        assert_eq!(next_support_level(0, "fail"), 0);
        assert_eq!(bare("**[[say: Poproszę bułkę.]]** (A roll)"), bare("**Poproszę bułkę.** (A roll)"));
        assert_ne!(bare("[[say: Poproszę bułki.]]"), bare("Poproszę bułkę."));
        assert_eq!(json_in("Here: ```json\n{\"a\": 1}\n```"), Some("{\"a\": 1}"));
    }

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
