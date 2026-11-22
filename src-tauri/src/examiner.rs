//! The examiner: checks a study lesson by interviewing the learner.
//!
//! It asks one question at a time — more, and harder ones, the more complex
//! the lesson — gives a line of feedback after each answer, and ends by calling
//! `finish_assessment` with a verdict. The verdict schedules the lesson's next
//! check-in exactly as a drill score does.
//!
//! A writing lesson is checked differently: the agent is not an examiner but
//! the other person in a situation — a neighbour, a waiter — writing only in
//! Polish and understanding only Polish. It runs on the partner model (small
//! and fast is fine) and gives its verdict as soon as the learner has done
//! what the lesson asks, or when they end the conversation.

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

use crate::agent::{tool, Agent, Tool, ToolOutput};
use crate::ai::Wire;
use crate::lessons;
use crate::db::{self, Conn, Db};
use crate::schema::{checkin_messages, checkins};
use crate::error::{Error, Result};
use crate::stats;
#[allow(unused_imports)]
use crate::ids::*;

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

// Spec: TERM-2
pub(crate) fn system_prompt(conn: &mut Conn, lesson: &lessons::LessonDetail) -> Result<String> {
    let previous = previous_feedback(conn, lesson.lesson.id)?;
    // A lesson built on Wikipedia: its sources, to keep questions to what
    // they say — and facts may be answered in the learner's own language.
    let reading = crate::wiki::reading_for(conn, lesson.lesson.id, 6000)?;
    let sources = if reading.is_empty() {
        String::new()
    } else {
        format!(
            "\nThe lesson is built on these sources. Ask only about what the lesson and they say. \
             Facts may be answered in English (or the learner's language); add one small Polish \
             step to each conversation — a sentence with the place or word in it.\n{reading}\n"
        )
    };
    let song = match lesson.lesson.song_id {
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
                "\nThis is a LISTENING lesson. The learner has just listened to the song \"{}\" and can \
                 replay it during the conversation. Ask about what they heard, not about the lesson \
                 note: what the chorus means, which word fills a gap in a line you quote with one \
                 word blanked, what a line means, what happens in the song, and — for harder \
                 lessons — why a word has the form it has. Quote at most one short line per \
                 question; never paste the lyrics.\nThe lyrics (Polish — English):\n{}\n",
                song.title,
                lyrics.join("\n\n")
            )
        })
        .unwrap_or_default();
    Ok(format!(
        "You are a friendly examiner in Polski, an app where an English speaker learns Polish. \
         You check whether the learner has understood one study lesson, by interviewing them.

How to run the conversation:
- Ask {plan}. One question per message — never several at once.
- Write in English; ask for answers in Polish where the objective is about Polish.
- After each answer, give one short line of feedback (right, or the correction and why), \
  then ask the next question in the same message.
- The last answer gets its feedback too: write that line in your message, then call \
  finish_assessment in the same message (and put the same line in its last_feedback).
- Accept Polish written without diacritics (\"czesc\" for \"cześć\"); mention the correct \
  spelling but don't count it as wrong.
- Cover every objective. Vary the questions: fill in a form, translate a short sentence, \
  pick the right option, explain a rule, use it in a new sentence.
- Don't reveal answers to questions you have not asked yet. Keep messages short.
- When you have enough evidence — or the learner asks to stop — call finish_assessment. \
  pass: every objective shown, at most small slips. partial: some objectives shaky. \
  fail: most not shown. aced: a pass with nothing worth going over again — it brings the \
  lesson back later and earns more mastery, so set it only when that is true. Be honest; a \
  pass that isn't earned helps nobody.

The lesson: {title} (complexity: {complexity})
Objectives:
{objectives}

The lesson the learner studied:
{content}
{listening}{sources}{previous}{memory}",
        plan = question_plan(&lesson.lesson.complexity),
        title = lesson.lesson.title,
        complexity = lesson.lesson.complexity,
        objectives = lesson.objectives.iter().map(|o| format!("- {o}")).collect::<Vec<_>>().join("\n"),
        content = lesson.content,
        listening = listening,
        sources = sources,
        previous = previous
            .map(|feedback| format!("\nLast conversation's verdict, to follow up on: {feedback}"))
            .unwrap_or_default(),
        memory = memory_section(conn)?,
    ))
}

/// How much the partner helps, by the lesson's support level (it moves with
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
/// help next time, a fail adds one back, a partial stays. Level 3 is level
/// 2 (writing on your own) passed: what mastering writing asks.
pub fn next_support_level(level: i64, result: &str) -> i64 {
    match result {
        "pass" => (level + 1).min(3),
        "fail" => (level.min(2) - 1).max(0),
        _ => level,
    }
}

/// How the partner writes: at the lesson's CEFR level (its own, else its
/// course's) — never below what the learner asked for. While the learner is
/// guided (help level 0) it keeps to the simplest sentences of that level.
/// Without a level, a beginner's.
fn partner_level(conn: &mut Conn, lesson: &lessons::LessonDetail) -> Result<String> {
    let level = match &lesson.lesson.level {
        Some(level) => Some(level.clone()),
        None => match lesson.lesson.course_id {
            Some(id) => crate::schema::courses::table
                .find(id)
                .select(crate::schema::courses::level)
                .first::<Option<String>>(conn)
                .optional()?
                .flatten(),
            None => None,
        },
    };
    let style = match level.as_deref().unwrap_or("A1") {
        "A1" => "A1: very short, simple sentences, the most common words, present tense",
        "A2" => "A2: short everyday sentences; past and future are fine",
        "B1" => "B1: natural everyday Polish, as you would write to a foreigner who speaks it reasonably",
        "B2" => "B2: natural Polish at normal pace, idiom included, as to someone fluent in everyday life",
        _ => "C1–C2: fully natural Polish, as to a native speaker",
    };
    Ok(if lesson.lesson.support_level == 0 {
        format!("{style} — while they are guided, the simplest sentences of that level")
    } else {
        style.to_string()
    })
}

/// What the app tells the partner when the learner ends the conversation.
const LEARNER_ENDED: &str = "(The app: the learner has ended the conversation. Do not reply in \
     character — call finish_assessment now, judging only what they wrote.)";
const OPENING: &str = "(The app: the learner has opened the conversation. Begin it, in character, in Polish.)";

pub(crate) fn writing_prompt(conn: &mut Conn, lesson: &lessons::LessonDetail) -> Result<String> {
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
  pass: every goal reached in understandable Polish. partial: some goals only half there. \
  fail: the goals not reached when the learner ended it. The help the app gives (word bank, \
  hints) never makes a pass partial — only what the learner got across counts.
  In the verdict (in English), summary says how it went; to_review lists their actual \
  mistakes as \"what they wrote → correct Polish\" — the conversation itself corrected nothing. \
  aced: a pass with nothing worth going over again (it brings the lesson back later and earns \
  more mastery) — small slips that never got in the way don't stop it.
- Messages starting with \"(The app:\" come from the app, not the learner: follow them.
{previous}{memory}",
        scenario = lesson.content,
        goals = lesson.objectives.iter().map(|o| format!("- {o}")).collect::<Vec<_>>().join("\n"),
        level = partner_level(conn, lesson)?,
        support = support_rules(lesson.lesson.support_level),
        previous = previous_feedback(conn, lesson.lesson.id)?
            .map(|feedback| format!("\nLast time's verdict, for your judgement only: {feedback}"))
            .unwrap_or_default(),
        memory = memory_section(conn)?,
    ))
}

fn previous_feedback(conn: &mut Conn, lesson_id: LessonId) -> Result<Option<String>> {
    Ok(checkins::table
        .filter(checkins::lesson_id.eq(lesson_id))
        .filter(checkins::finished_at.is_not_null())
        .order(checkins::id.desc())
        .select(checkins::feedback)
        .first::<Option<String>>(conn)
        .optional()?
        .flatten())
}

/// What the agents remember, plus what the examiner in particular should
/// save: what this check-in showed that will matter beyond this lesson.
fn memory_section(conn: &mut Conn) -> Result<String> {
    let section = crate::memories::prompt_section(conn)?;
    if section.is_empty() {
        return Ok(section);
    }
    Ok(format!(
        "{section}\n- As the examiner: use what you remember to aim questions at known weak spots \
         that touch this lesson. When you finish, save — in the same message as \
         finish_assessment — at most two memories about lasting patterns this conversation showed \
         (a recurring mistake, something now mastered), not this lesson's score. Update a \
         struggle that the learner has now clearly overcome."
    ))
}

fn tools() -> Vec<Tool> {
    vec![tool(
        "finish_assessment",
        "Weighing it up…",
        "End the conversation with your verdict.",
        json!({
            "type": "object",
            "properties": {
                "result": { "type": "string", "enum": ["pass", "partial", "fail"] },
                "last_feedback": { "type": "string", "description": "Your one line of feedback on the learner's last answer (right, or the correction and why), as after every other answer." },
                "summary": { "type": "string", "description": "Two or three sentences to the learner: how it went." },
                "strengths": { "type": "array", "items": { "type": "string" } },
                "aced": { "type": "boolean", "description": "A pass with nothing worth going over again: the lesson comes back later and earns more mastery. False when to_review has anything that matters." },
                "to_review": { "type": "array", "items": { "type": "string" }, "description": "What to go over again, concretely (shown to the learner)." },
                "mistakes": {
                    "type": "array",
                    "description": "Each slip in the learner's Polish during this conversation, as said and corrected, for their learning log (not shown as a list). Leave out when there were none.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "said": { "type": "string", "description": "What the learner wrote, the words around the slip" },
                            "correct": { "type": "string", "description": "The same words, corrected" },
                            "grammar": { "type": "array", "items": { "type": "string", "enum": crate::grammar::keys() }, "description": "The structures the slip is about" },
                            "note": { "type": "string", "description": "A few words on what went wrong" }
                        },
                        "required": ["said", "correct"]
                    }
                }
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
    /// A pass with nothing worth going over again (the examiner says so).
    #[serde(default)]
    pub aced: bool,
    /// The feedback on the learner's last answer, shown as the examiner's
    /// message when it gave the verdict without one — the last answer is
    /// never left without a reply. Not part of the verdict the screens show.
    #[serde(default, alias = "last_feedback", skip_serializing)]
    pub last_feedback: Option<String>,
    /// The slips the examiner noted, for the learning log (not shown).
    #[serde(default, skip_serializing)]
    pub mistakes: Vec<Slip>,
    /// Filled in when recorded.
    #[serde(default)]
    pub next_due_day: i64,
    #[serde(default)]
    pub xp: i64,
}

/// A slip in the learner's Polish, as the examiner noted it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Slip {
    pub said: String,
    pub correct: String,
    #[serde(default)]
    pub grammar: Vec<String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinTurn {
    pub checkin_id: CheckinId,
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

fn history(conn: &mut Conn, checkin_id: CheckinId) -> Result<Vec<Wire>> {
    Ok(checkin_messages::table
        .filter(checkin_messages::checkin_id.eq(checkin_id))
        .order(checkin_messages::id)
        .select((checkin_messages::role, checkin_messages::content))
        .load::<(String, String)>(conn)?
        .into_iter()
        .map(|(role, content)| if role == "user" { Wire::user(content) } else { Wire::assistant(content) })
        .collect())
}

fn append(conn: &mut Conn, checkin_id: CheckinId, role: &str, content: &str) -> Result<()> {
    diesel::insert_into(checkin_messages::table)
        .values((
            checkin_messages::id.eq(CheckinMessageId::new()),
            checkin_messages::checkin_id.eq(checkin_id),
            checkin_messages::role.eq(role),
            checkin_messages::content.eq(content),
            checkin_messages::created_at.eq(db::now_secs()),
        ))
        .execute(conn)?;
    Ok(())
}

/// The verdict in the learning log, in the verdict's transaction: one
/// answer for the check-in, and one for each slip the examiner noted, with
/// its mistake class and the structures it named (unknown ones dropped:
/// the verdict matters more than a tag).
fn log_verdict(conn: &mut Conn, checkin_id: CheckinId, lesson_id: LessonId, assessment: &Assessment) -> Result<()> {
    let run = checkin_id.to_string();
    let lesson = lesson_id.to_string();
    let none = crate::mistakes::Mistake::default();
    let signals = crate::learnlog::Signals::default();
    let context = started_from(conn, checkin_id, lesson_id)?;
    let verdict = crate::learnlog::Verdict {
        result: assessment.result.clone(),
        xp: assessment.xp,
        aced: assessment.aced,
        summary: assessment.summary.clone(),
    };
    let title = crate::schema::lessons::table.find(lesson_id).select(crate::schema::lessons::title).first::<String>(conn).optional()?.unwrap_or_default();
    let prompt = format!("Conversation: {title}");
    crate::learnlog::write_attempt(
        conn,
        &crate::learnlog::NewAttempt {
            item_kind: "lesson",
            item_id: &lesson,
            lesson_id: Some(lesson_id),
            exercise: "checkin",
            run: &run,
            correct: assessment.result == "pass",
            given: &assessment.result,
            expected: "pass",
            mistake: &none,
            signals: &signals,
            options: &[],
            grammar: vec![],
            note: assessment.to_review.join("; "),
            context: &context,
            prompt: &prompt,
            card: None,
            pl: "",
            checkin_id: Some(checkin_id),
            verdict: Some(verdict.clone()),
        },
    )?;
    for slip in assessment.mistakes.iter().take(20) {
        if slip.said.trim().is_empty() || slip.correct.trim().is_empty() {
            continue;
        }
        let mistake = crate::mistakes::classify(&crate::mistakes::Graded {
            exercise: "checkinMistake",
            expected: &slip.correct,
            given: &slip.said,
            ..Default::default()
        });
        let grammar: Vec<String> = slip.grammar.iter().filter_map(|k| crate::grammar::by_key(k)).map(|t| t.key.to_string()).collect();
        crate::learnlog::write_attempt(
            conn,
            &crate::learnlog::NewAttempt {
                item_kind: "lesson",
                item_id: &lesson,
                lesson_id: Some(lesson_id),
                exercise: "checkinMistake",
                run: &run,
                correct: false,
                given: &slip.said,
                expected: &slip.correct,
                mistake: &mistake,
                signals: &signals,
                options: &[],
                grammar,
                note: slip.note.clone(),
                context: &context,
                prompt: &prompt,
                card: None,
                pl: "",
                checkin_id: Some(checkin_id),
                verdict: Some(verdict.clone()),
            },
        )?;
    }
    Ok(())
}

/// Where a check-in was started from: what its `checkin_started` event
/// kept (the course page, the daily five…), or the lesson's own course.
fn started_from(conn: &mut Conn, checkin_id: CheckinId, lesson_id: LessonId) -> Result<crate::learnlog::PracticeContext> {
    use crate::schema::learning_events as ev;
    let found: Option<(Option<CourseId>, String)> = ev::table
        .filter(ev::kind.eq("checkin_started"))
        .filter(ev::run.eq(checkin_id.to_string()))
        .select((ev::course_id, ev::source))
        .first(conn)
        .optional()?;
    let given = found.map(|(course_id, source)| crate::learnlog::PracticeContext { source, course_id });
    crate::learnlog::resolve(conn, given.as_ref(), Some(lesson_id), "")
}

/// Records the verdict: schedules the lesson, awards XP and counts the day.
/// Records a verdict, all or nothing (one transaction).
pub fn record(conn: &mut Conn, checkin_id: CheckinId, assessment: Assessment) -> Result<(Assessment, bool)> {
    conn.transaction(|tx| record_steps(tx, checkin_id, assessment))
}

fn record_steps(conn: &mut Conn, checkin_id: CheckinId, mut assessment: Assessment) -> Result<(Assessment, bool)> {
    let lesson_id: LessonId = checkins::table.find(checkin_id).select(checkins::lesson_id).first(conn)?;
    if !["pass", "partial", "fail"].contains(&assessment.result.as_str()) {
        assessment.result = "partial".into();
    }
    let today = db::today(conn);
    // As with drills, a lesson already passed today is extra practice: half
    // XP, and its schedule stays. (Checking in early, before its day, is not:
    // it moves the schedule — see lessons::next_gap.)
    let current = lessons::load(conn, lesson_id)?;
    let extra = lessons::is_extra_practice(&current, today);
    // A pass with nothing left to review is an ace: the gap grows faster.
    // An ace is the examiner's call, made knowing what it does.
    let ace = assessment.result == "pass" && assessment.aced;
    let run = checkin_id.to_string();
    let go = lessons::Go { via: "checkin", run: &run, score: None, extra };
    let lesson = if extra {
        // Extra practice leaves the schedule alone, but still earns mastery.
        lessons::credit_mastery(conn, &current, &assessment.result, ace, today, &go)?;
        lessons::load(conn, lesson_id)?
    } else {
        lessons::record_go(conn, lesson_id, &assessment.result, ace, &go)?.0
    };
    // What the lesson's type adds: a bonus per goal (writing), and what
    // follows the verdict (writing: its help steps down or up).
    let conversation = match &crate::lesson_types::get(&lesson.lesson_type)?.run {
        crate::lesson_types::Run::Conversation(c) => Some(c),
        _ => None,
    };
    let goals = if conversation.is_some_and(|c| c.goals_pay) { lessons::detail(conn, lesson_id)?.objectives.len() } else { 0 };
    if let Some(after) = conversation.and_then(|c| c.after_verdict) {
        after(conn, &lesson, &assessment.result)?;
    }
    let writing_goals = goals;
    assessment.xp = stats::xp_for_checkin(&lesson.complexity, &assessment.result, writing_goals);
    if extra {
        assessment.xp = (assessment.xp + 1) / 2;
    }
    assessment.next_due_day = lesson.due_day.unwrap_or(today);
    // A verdict is not an answer: it counts toward the day, not accuracy.
    stats::add_xp(conn, today, assessment.xp)?;
    let verdict = match assessment.result.as_str() {
        "pass" => "passed",
        "partial" => "partly there",
        _ => "not yet",
    };
    let mut note = Vec::new();
    if writing_goals > 0 && assessment.result != "fail" {
        note.push(format!("{writing_goals} goal{} counted", if writing_goals == 1 { "" } else { "s" }));
    }
    if extra {
        note.push("extra practice: half XP".to_string());
    }
    crate::xplog::write(
        conn,
        today,
        &format!("checkin-{checkin_id}"),
        Some(lesson_id),
        &format!("Conversation: {verdict}"),
        assessment.xp,
        &note.join(", "),
    )?;
    let first = stats::mark_practised(conn, today)?;
    log_verdict(conn, checkin_id, lesson_id, &assessment)?;
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
    checkin_id: CheckinId,
    user: Option<String>,
    stream_id: &str,
) -> Result<CheckinTurn> {
    let (wire, writing, model) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let (lesson_id, finished): (LessonId, Option<i64>) =
            checkins::table.find(checkin_id).select((checkins::lesson_id, checkins::finished_at)).first(conn)?;
        if finished.is_some() {
            return Err(Error::msg("This conversation is already finished."));
        }
        let detail = lessons::detail(conn, lesson_id)?;
        if let Some(text) = &user {
            append(conn, checkin_id, "user", text)?;
        }
        // A partner in a situation (writing) rather than an examiner.
        let writing = matches!(
            &crate::lesson_types::get(&detail.lesson.lesson_type)?.run,
            crate::lesson_types::Run::Conversation(c) if c.partner
        );
        let (prompt, opening, model) = if writing {
            (writing_prompt(conn, &detail)?, OPENING, Some(crate::ai::partner_model(conn)?))
        } else {
            (system_prompt(conn, &detail)?, "I'm ready for your questions.", None)
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
    let remembered = outcome.remembered;
    let verdict = outcome.extras.into_iter().next();
    // A verdict given without a word: its feedback on the last answer is the
    // message, so the learner hears how that answer went too.
    let reply = match verdict.as_ref().and_then(|v| v.last_feedback.as_deref()).map(str::trim) {
        Some(feedback) if outcome.text.trim().is_empty() && !feedback.is_empty() => feedback.to_string(),
        _ => outcome.text,
    };

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

/// Starts a check-in on a study lesson; the examiner asks the first question.
#[tauri::command]
pub async fn checkin_start(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    lesson_id: LessonId,
    stream_id: String,
    context: Option<crate::learnlog::PracticeContext>,
) -> Result<CheckinTurn> {
    let checkin_id = start(&mut db.lock(), lesson_id, context.as_ref())?;
    turn(&app, &db, checkin_id, None, &stream_id).await
}

/// Opens a check-in on a study lesson, noting where it was started from
/// (its verdict is logged with that: [`started_from`]).
pub fn start(conn: &mut Conn, lesson_id: LessonId, context: Option<&crate::learnlog::PracticeContext>) -> Result<CheckinId> {
    let context = crate::learnlog::resolve(conn, context, Some(lesson_id), "")?;
    let lesson = lessons::load(conn, lesson_id)?;
    if crate::lesson_types::get(&lesson.lesson_type)?.is_drill() {
        return Err(Error::msg("Only study lessons have an interview."));
    }
    conn.transaction(|conn| {
        let id = diesel::insert_into(checkins::table)
            .values((checkins::id.eq(CheckinId::new()), checkins::lesson_id.eq(lesson_id), checkins::started_at.eq(db::now_secs())))
            .returning(checkins::id)
            .get_result::<CheckinId>(conn)?;
        crate::learnlog::note(conn, "checkin_started", "lesson", &lesson_id.to_string(), Some(lesson_id), &id.to_string(), Default::default(), &context)?;
        Ok(id)
    })
}

/// Ends a writing conversation early: the partner gives its verdict on what
/// was written so far.
#[tauri::command]
pub async fn checkin_end(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    checkin_id: CheckinId,
    stream_id: String,
) -> Result<CheckinTurn> {
    turn(&app, &db, checkin_id, Some(LEARNER_ENDED.to_string()), &stream_id).await
}

/// Answers the examiner.
#[tauri::command]
pub async fn checkin_send(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    checkin_id: CheckinId,
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
pub async fn lesson_add_players(db: State<'_, Db>, lesson_id: LessonId) -> Result<lessons::LessonDetail> {
    let (content, key, model) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let detail = lessons::detail(conn, lesson_id)?;
        (detail.content, crate::ai::api_key(conn)?, crate::ai::partner_model(conn)?)
    };
    if content.contains("[[say:") {
        return Ok(lessons::detail(&mut db.lock(), lesson_id)?);
    }
    let prompt = format!(
        "Below is a Polish lesson in markdown. Wrap every Polish word, phrase or example sentence in \
         [[say: …]] — for example **Poproszę bułkę.** becomes **[[say: Poproszę bułkę.]]** — so the \
         learner can hear it. Change nothing else at all: not one character of the text, the \
         English, the markdown or the line breaks. Leave English, single letters and word endings \
         (like -ę) untagged, and never tag inside a table header. Reply with only the lesson.\n\n{content}"
    );
    let reply = crate::ai::complete_with(&key, &model, &[Wire::user(prompt)], 0.0, 8000).await?;
    let tagged = reply.text.trim().trim_start_matches("```markdown").trim_start_matches("```").trim_end_matches("```").trim().to_string();
    let _ = crate::costs::record_tokens(&mut db.lock(), crate::costs::PARTNER, &model, reply.cost, "lesson players", reply.tokens);
    if bare(&tagged) != bare(&content) {
        return Err(Error::msg("The lesson came back changed, so it was left as it was. Try again, or with another model."));
    }
    if !tagged.contains("[[say:") {
        return Err(Error::msg("No Polish was found to add players to."));
    }
    let mut guard = db.lock();
    lessons::update(&mut guard, lesson_id, lessons::LessonChanges { content: Some(tagged), ..Default::default() })?;
    lessons::detail(&mut guard, lesson_id)
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

/// A phrase as models write it: `{"pl", "en"}` (or the other names they
/// reach for), or a "Polish — English" string.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawPhrase {
    Text(String),
    Pair {
        #[serde(alias = "polish", alias = "phrase", alias = "text", alias = "word")]
        pl: String,
        #[serde(default, alias = "english", alias = "meaning", alias = "translation")]
        en: String,
    },
}

impl RawPhrase {
    fn phrase(self) -> Option<Phrase> {
        let (pl, en) = match self {
            RawPhrase::Text(text) => {
                let (pl, en) =
                    [" — ", " – ", " - ", ": "].iter().find_map(|sep| text.split_once(sep)).unwrap_or((&text, ""));
                (pl.trim().to_string(), en.trim().to_string())
            }
            RawPhrase::Pair { pl, en } => (pl.trim().to_string(), en.trim().to_string()),
        };
        (!pl.is_empty()).then_some(Phrase { pl, en })
    }
}

/// One goal's help as a model writes it.
#[derive(Deserialize)]
struct RawGoal {
    #[serde(default, alias = "name", alias = "title")]
    goal: String,
    #[serde(default, alias = "phrases", alias = "vocabulary")]
    words: Vec<crate::wire::Lenient<RawPhrase>>,
    #[serde(default, deserialize_with = "crate::wire::lenient")]
    starter: Option<RawPhrase>,
}

/// The bank: `{"goals": [...]}` (or under another name models use), or a
/// bare list.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawBank {
    List(Vec<crate::wire::Lenient<RawGoal>>),
    Wrapped {
        #[serde(alias = "help", alias = "bank", alias = "items", alias = "word_bank")]
        goals: Vec<crate::wire::Lenient<RawGoal>>,
    },
}

/// A word bank from a model's reply, read leniently: `{"goals": [...]}` or
/// a bare list, phrases as objects or strings. None when nothing usable is in it.
fn parse_bank(text: &str) -> Option<Vec<GoalHelp>> {
    let goals = match crate::ai::json_in::<RawBank>(text).ok()? {
        RawBank::List(goals) | RawBank::Wrapped { goals } => goals,
    };
    let help: Vec<GoalHelp> = crate::wire::kept(goals)
        .filter_map(|g| {
            let words: Vec<Phrase> = crate::wire::kept(g.words).filter_map(RawPhrase::phrase).collect();
            let starter = match g.starter.and_then(RawPhrase::phrase) {
                Some(starter) => starter,
                None => words.first()?.clone(),
            };
            Some(GoalHelp { goal: g.goal.trim().to_string(), starter, words })
        })
        .collect();
    (!help.is_empty()).then_some(help)
}

/// A writing lesson's word bank: for each goal, a sentence starter and a few
/// words to use. Made once by the partner's model, then kept.
#[tauri::command]
pub async fn writing_scaffold(db: State<'_, Db>, lesson_id: LessonId) -> Result<Vec<GoalHelp>> {
    let (detail, stored, key, model) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let stored: Option<String> = crate::schema::lessons::table
            .find(lesson_id)
            .select(crate::schema::lessons::scaffold)
            .first(conn)?;
        (lessons::detail(conn, lesson_id)?, stored, crate::ai::api_key(conn)?, crate::ai::partner_model(conn)?)
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
        detail.content,
        detail.objectives.iter().map(|o| format!("- {o}")).collect::<Vec<_>>().join("\n"),
    );
    let reply = crate::ai::complete_json(&key, &model, &[Wire::user(prompt)], 6000).await?;
    let mut guard = db.lock();
    let conn = &mut *guard;
    let _ = crate::costs::record_tokens(conn, crate::costs::PARTNER, &model, reply.cost, "word bank", reply.tokens);
    let Some(goals) = parse_bank(&reply.text) else {
        crate::diagnostics::unreadable_reply(&format!("A word bank (from {model})"), &reply.text);
        return Err(Error::msg(if reply.truncated {
            "The word bank was cut off before it was finished. Try again, or pick another Writing partner model in Settings."
        } else {
            "The word bank came back unreadable. Try again, or pick another Writing partner model in Settings."
        }));
    };
    diesel::update(crate::schema::lessons::table.find(lesson_id))
        .set(crate::schema::lessons::scaffold.eq(serde_json::to_string(&goals)?))
        .execute(conn)?;
    Ok(goals)
}

/// A hint for what to write next in a writing conversation: a sentence with
/// a blank or two for a goal not reached yet, with its meaning. Not sent to
/// the partner.
#[tauri::command]
pub async fn writing_hint(db: State<'_, Db>, checkin_id: CheckinId) -> Result<String> {
    let (detail, past, key, model) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let lesson_id: LessonId = checkins::table.find(checkin_id).select(checkins::lesson_id).first(conn)?;
        let past: Vec<(String, String)> = checkin_messages::table
            .filter(checkin_messages::checkin_id.eq(checkin_id))
            .order(checkin_messages::id)
            .select((checkin_messages::role, checkin_messages::content))
            .load(conn)?;
        (lessons::detail(conn, lesson_id)?, past, crate::ai::api_key(conn)?, crate::ai::partner_model(conn)?)
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
        assert_eq!(next_support_level(2, "pass"), 3);
        assert_eq!(next_support_level(3, "pass"), 3);
        assert_eq!(next_support_level(3, "fail"), 1);
        assert_eq!(next_support_level(1, "partial"), 1);
        assert_eq!(next_support_level(1, "fail"), 0);
        assert_eq!(next_support_level(0, "fail"), 0);
        assert_eq!(bare("**[[say: Poproszę bułkę.]]** (A roll)"), bare("**Poproszę bułkę.** (A roll)"));
        assert_ne!(bare("[[say: Poproszę bułki.]]"), bare("Poproszę bułkę."));
    }

    #[test]
    fn word_banks_are_read_however_the_model_shapes_them() {
        let asked = r#"{"goals": [{"goal": "Order", "starter": {"pl": "Poproszę…", "en": "I'd like…"},
            "words": [{"pl": "chleb", "en": "bread"}, {"pl": "bułki", "en": "rolls"}]}]}"#;
        let bank = parse_bank(asked).unwrap();
        assert_eq!(bank[0].starter.pl, "Poproszę…");
        assert_eq!(bank[0].words.len(), 2);

        let bare = r#"```json
[{"goal": "Pay", "starter": "Płacę kartą — I'll pay by card", "words": ["gotówka — cash", {"polish": "karta", "english": "card"}]}]
```"#;
        let bank = parse_bank(bare).unwrap();
        assert_eq!((bank[0].starter.pl.as_str(), bank[0].starter.en.as_str()), ("Płacę kartą", "I'll pay by card"));
        assert_eq!(bank[0].words[0].en, "cash");
        assert_eq!(bank[0].words[1].pl, "karta");

        assert!(parse_bank(r#"{"goals": [{"goal": "Order", "starter": {"pl": "Popro"#).is_none());
        assert!(parse_bank(r#"{"goals": []}"#).is_none());
    }

    #[test]
    fn harder_lessons_get_longer_interviews() {
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
        // The last answer's feedback comes in, and stays out of what the
        // screens are sent.
        let c: Assessment =
            serde_json::from_str(r#"{"result":"pass","summary":"x","last_feedback":"Right: mam."}"#).unwrap();
        assert_eq!(c.last_feedback.as_deref(), Some("Right: mam."));
        assert!(!serde_json::to_string(&c).unwrap().contains("Right: mam."));
    }
}
