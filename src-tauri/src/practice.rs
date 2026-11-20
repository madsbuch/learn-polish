//! Doing a lesson: building its exercises, checking answers, finishing a
//! drill, and the results screen after a drill or a check-in.

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::lessons;
use crate::db::{self, Conn, Db};
use crate::schema::achievements;
use crate::error::Result;
use crate::models::*;
use crate::session;
use crate::srs;
use crate::stats;
#[allow(unused_imports)]
use crate::ids::*;

// ---------------------------------------------------------------------------
// Answer checking
// ---------------------------------------------------------------------------

/// Folds away everything a learner should not be penalised for: case,
/// punctuation, doubled spaces and missing Polish diacritics. Typing "czesc"
/// counts as "cześć" — the correct spelling is shown in the feedback anyway.
pub(crate) fn normalize(text: &str) -> String {
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

/// A typed answer one letter off a word of four letters or more ("wolno"
/// for "wolny"): wrong, but nearly there. It is still a mistake in the log,
/// but earns half XP and is not started over (see [`submit_steps`]). Short
/// words are left out: one letter off "kot" is another word ("kod"), not a
/// slip. Checked like [`answers_match`]: the whole answer, or any one of its
/// listed alternatives.
fn almost_matches(expected: &str, given: &str) -> bool {
    let given = normalize(given);
    if given.is_empty() {
        return false;
    }
    // Letters are counted as written, before folding (which can spell one
    // letter as two).
    std::iter::once(expected)
        .chain(expected.split(','))
        .filter(|candidate| candidate.chars().filter(|c| c.is_alphabetic()).count() >= 4)
        .any(|candidate| crate::mistakes::distance(&normalize(candidate), &given) <= 1)
}

// ---------------------------------------------------------------------------
// Answers and results
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitAnswer {
    pub kind: String,
    pub item_kind: String,
    pub item_id: String,
    pub is_new: bool,
    pub expected: String,
    pub given: String,
    /// Optional override; normally derived from whether the answer was right.
    pub grade: Option<i64>,
    /// Extra practice on a lesson that is not due: half XP.
    #[serde(default)]
    pub extra: bool,
    /// The lesson, and this go at it (the session's key): for the XP log.
    #[serde(default)]
    pub lesson_id: Option<LessonId>,
    #[serde(default)]
    pub run: String,
    /// The options shown (a choice, a sound pair): for the learning log.
    #[serde(default)]
    pub options: Vec<String>,
    /// What the card showed and the Polish it is about (a scene's
    /// true-or-false card: the statement and the true sentence), so the
    /// mistake can be told (mistakes.rs).
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub pl: String,
    /// How it was answered (time, tries, audio, translations shown).
    #[serde(default)]
    pub signals: Option<crate::learnlog::Signals>,
    /// Where it is practised from (the plan's context, handed back).
    #[serde(default)]
    pub context: Option<crate::learnlog::PracticeContext>,
    /// What else the card showed: its key, place in the run, hint, tiles.
    #[serde(default)]
    pub card: Option<crate::learnlog::CardShown>,
}

/// Cards that are only read (a new word, a new sentence, a dialogue to read
/// through): nothing is answered, so nothing is scored.
pub const READING_KINDS: &[&str] = &["intro", "sentenceIntro", "dialogueRead", "pictureIntro", "sceneExplore", "storyPanel", "storyEnd"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerResult {
    pub correct: bool,
    /// Wrong, but nearly: one letter off a typed answer (see
    /// [`almost_matches`]), or one word off a dictation or a spoken
    /// sentence of three words or more (`dictation::nearly`): half XP, half
    /// a point, and the review comes back sooner rather than starting over.
    pub almost: bool,
    pub expected: String,
    pub xp_gained: i64,
    pub total_xp: i64,
    pub day_xp: i64,
    pub level: i64,
    /// A dictation: what was written, word by word against the sentence
    /// (dictation.rs); a speaking card: what was heard. Empty for every
    /// other kind.
    pub diff: Vec<crate::dictation::DiffWord>,
    /// A speaking card: words heard as others by a known contrast, and what
    /// to try (speaking.rs). Empty for every other kind.
    pub tips: Vec<crate::sounds::SoundTip>,
}

#[tauri::command]
pub async fn submit_answer(db: State<'_, Db>, answer: SubmitAnswer) -> Result<AnswerResult> {
    submit_answer_in(&mut db.lock(), answer)
}

/// The work behind [`submit_answer`], callable without Tauri (tests).
/// The grade, the schedule, the XP and the learning log's row are one
/// transaction: all or nothing.
pub fn submit_answer_in(conn: &mut Conn, answer: SubmitAnswer) -> Result<AnswerResult> {
    conn.transaction(|tx| submit_steps(tx, answer))
}

fn submit_steps(conn: &mut Conn, answer: SubmitAnswer) -> Result<AnswerResult> {
    let today = db::today(conn);
    let context = crate::learnlog::resolve(conn, answer.context.as_ref(), answer.lesson_id, "")?;

    // Reading a card is not answering it: no XP, no review, not counted in
    // accuracy. (Seeing a new word must not move it toward "mastered".)
    let label = crate::xplog::answer_label(&answer.kind, &answer.expected);
    if READING_KINDS.contains(&answer.kind.as_str()) {
        crate::xplog::write(conn, today, &answer.run, answer.lesson_id, &label, 0, "read: no XP")?;
        // Read, not answered: an event in the log, not an attempt.
        let item_id = if crate::ids::kind_of(&answer.item_id).is_some() { answer.item_id.as_str() } else { "" };
        crate::learnlog::note(
            conn,
            "item_viewed",
            &answer.item_kind,
            item_id,
            answer.lesson_id,
            &answer.run,
            crate::learnlog::EventDetails { exercise: Some(answer.kind.clone()), source: Some("session".into()), ..Default::default() },
            &context,
        )?;
        let total_xp = stats::total_xp(conn)?;
        let day_xp = stats::day_xp(conn, today)?;
        return Ok(AnswerResult {
            correct: true,
            almost: false,
            expected: answer.expected,
            xp_gained: 0,
            total_xp,
            day_xp,
            level: stats::level_from_xp(total_xp),
            diff: vec![],
            tips: vec![],
        });
    }

    // Tiles are given, not typed: their Polish letters are the point (the
    // ending picked on a tile: "kawę", not "kawą"), so nothing is forgiven
    // there but case and punctuation.
    // A dictation is compared word by word, missing Polish letters
    // forgiven but shown. A sound pair is the one word or the other: they
    // differ in just the letters that folding would forgive ("tę", "te").
    // A speaking card is what the learner's best attempt was heard as,
    // compared like a dictation (speaking.rs).
    let mut diff = vec![];
    let mut tips = vec![];
    let mut almost = false;
    let correct = match answer.kind.as_str() {
        "wordOrder" => crate::tiles::fold(&answer.expected) == crate::tiles::fold(&answer.given),
        "dictation" => {
            let checked = crate::dictation::check(&answer.expected, &answer.given);
            almost = !checked.correct && crate::dictation::nearly(&checked.words);
            diff = checked.words;
            checked.correct
        }
        "soundPair" => answer.expected.trim().to_lowercase() == answer.given.trim().to_lowercase(),
        "sayIt" | "dialogueSay" => {
            let spoken = crate::speaking::check(&answer.expected, &answer.given);
            // Mostly heard right: all but one word of a longer sentence.
            almost = !spoken.correct && crate::dictation::nearly(&spoken.words);
            diff = spoken.words;
            tips = spoken.tips;
            spoken.correct
        }
        _ => {
            let right = answers_match(&answer.expected, &answer.given);
            // Only what was typed can be a slip: a pick one letter off the
            // answer is a look-alike, a real confusion (mistakes.rs).
            almost = !right && !crate::mistakes::PICKED.contains(&answer.kind.as_str()) && almost_matches(&answer.expected, &answer.given);
            right
        }
    };
    // A sentence not quite heard is not a sentence forgotten: the learner
    // knew what to say, and the ear (a model's, in a room) is fallible. It
    // comes back sooner, but is not started over.
    // A slip of one letter, or one word off a sentence written or said,
    // is the same: known, nearly — sooner, not over.
    let missed = if almost || crate::speaking::KINDS.contains(&answer.kind.as_str()) { srs::HARD } else { srs::AGAIN };
    let grade = answer
        .grade
        .unwrap_or(if correct { srs::GOOD } else { missed })
        .clamp(srs::AGAIN, srs::EASY);

    // Conversation gaps exercise the dialogue rather than a scheduled item, so
    // they earn XP without creating review rows.
    if answer.item_kind == "word" || answer.item_kind == "sentence" {
        let state = srs::load(conn, &answer.item_kind, &answer.item_id)?
            .unwrap_or_else(|| srs::ReviewState::fresh(today));
        let next = srs::grade(state, grade, today);
        srs::save(conn, &answer.item_kind, &answer.item_id, &next)?;
    }

    let mut xp = stats::xp_for_answer(&answer.kind, correct || almost, answer.is_new);
    if almost {
        // Nearly right is worth something: half, rounded up.
        xp = (xp + 1) / 2;
    }
    if answer.extra {
        xp = (xp + 1) / 2;
    }
    stats::record_answer(conn, today, xp, correct)?;
    let note = match (correct, answer.extra, answer.is_new) {
        (false, true, _) if almost => "almost, on extra practice: a quarter XP",
        (false, _, _) if almost => "almost: half XP",
        (false, _, _) => "a mistake: no XP",
        (true, true, _) => "extra practice: half XP",
        (true, false, true) => "new: +2",
        _ => "",
    };
    crate::xplog::write(conn, today, &answer.run, answer.lesson_id, &label, xp, note)?;

    // The learning log: what kind of mistake it was, and how it was answered.
    let endings = if answer.kind == "wordOrder" && answer.item_kind == "sentence" {
        SentenceId::parse(&answer.item_id)
            .ok()
            .and_then(|id| crate::schema::sentences::table.find(id).select(crate::schema::sentences::endings).first::<String>(conn).ok())
            .map(|json| crate::tiles::parse(&json))
            .unwrap_or_default()
    } else {
        vec![]
    };
    let mistake = crate::mistakes::classify(&crate::mistakes::Graded {
        exercise: &answer.kind,
        expected: &answer.expected,
        given: &answer.given,
        correct,
        diff: &diff,
        options: &answer.options,
        prompt: &answer.prompt,
        pl: &answer.pl,
        endings: &endings,
    });
    let signals = answer.signals.clone().unwrap_or_default();
    crate::learnlog::write_attempt(
        conn,
        &crate::learnlog::NewAttempt {
            item_kind: &answer.item_kind,
            item_id: &answer.item_id,
            lesson_id: answer.lesson_id,
            exercise: &answer.kind,
            run: &answer.run,
            correct,
            given: &answer.given,
            expected: &answer.expected,
            mistake: &mistake,
            signals: &signals,
            options: &answer.options,
            grammar: vec![],
            note: String::new(),
            context: &context,
            prompt: &answer.prompt,
            card: answer.card.as_ref(),
            pl: &answer.pl,
            checkin_id: None,
            verdict: None,
        },
    )?;

    let total_xp = stats::total_xp(conn)?;
    let day_xp = stats::day_xp(conn, today)?;

    Ok(AnswerResult {
        correct,
        almost,
        expected: answer.expected,
        xp_gained: xp,
        total_xp,
        day_xp,
        level: stats::level_from_xp(total_xp),
        diff,
        tips,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResult {
    pub first_completion_today: bool,
    pub newly_unlocked: Vec<Achievement>,
    pub stats: Stats,
    /// Courses this completed or mastered.
    pub milestones: Vec<crate::courses::Milestone>,
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
    let milestones = crate::courses::milestones(conn)?;
    Ok(SessionResult { first_completion_today: first, newly_unlocked, stats, milestones })
}

fn unlocked_codes(conn: &mut Conn) -> Result<std::collections::HashSet<String>> {
    Ok(achievements::table.select(achievements::code).load::<String>(conn)?.into_iter().collect())
}

/// The exercises of a drill lesson.
#[tauri::command]
pub async fn lesson_session(db: State<'_, Db>, lesson_id: LessonId, context: Option<crate::learnlog::PracticeContext>) -> Result<SessionPlan> {
    session_with(&mut db.lock(), lesson_id, context.as_ref())
}

/// A drill's plan, with where it is practised from (checked, its course
/// filled in from the lesson when the screen gave none).
pub fn session_with(conn: &mut Conn, lesson_id: LessonId, context: Option<&crate::learnlog::PracticeContext>) -> Result<SessionPlan> {
    let mut plan = session::build_lesson(conn, lesson_id)?;
    plan.context = crate::learnlog::resolve(conn, context, Some(lesson_id), "")?;
    Ok(plan)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LessonFinished {
    pub lesson: lessons::Lesson,
    /// "pass" | "partial" | "fail"
    pub result: String,
    /// False when nothing was scored: not graded at all (no mastery, no
    /// schedule change), and `result` means nothing.
    pub graded: bool,
    /// Extra practice on a lesson that was not due: its schedule is unchanged.
    pub extra: bool,
    /// This drill's score: right answers out of the scored cards, and the
    /// answers one letter off (each counts as half right).
    pub correct: i64,
    pub almost: i64,
    pub total: i64,
    /// What it did to the lesson's mastery points: its row in the ledger.
    pub mastery: Option<crate::masterylog::MasteryEntry>,
    #[serde(flatten)]
    pub session: SessionResult,
}

/// A drill is done: its score becomes a result that schedules the next
/// check-in, and the day counts toward the streak.
#[tauri::command]
pub async fn lesson_finish(
    db: State<'_, Db>,
    lesson_id: LessonId,
    correct: i64,
    almost: Option<i64>,
    total: i64,
    context: Option<crate::learnlog::PracticeContext>,
    run: Option<String>,
) -> Result<LessonFinished> {
    let conn = &mut db.lock();
    let context = crate::learnlog::resolve(conn, context.as_ref(), Some(lesson_id), "")?;
    let run = run.unwrap_or_default();
    conn.transaction(|tx| lesson_finish_steps(tx, lesson_id, Score { correct, almost: almost.unwrap_or(0), total }, &context, &run))
}

/// The work behind [`lesson_finish`], callable without Tauri (tests).
/// Finishes a drill: its grade, schedule, mastery, XP and the day, all or
/// nothing (one transaction).
#[cfg(test)]
pub fn lesson_finish_in(conn: &mut Conn, lesson_id: LessonId, correct: i64, total: i64) -> Result<LessonFinished> {
    lesson_finish_with(conn, lesson_id, Score { correct, almost: 0, total }, None)
}

/// A drill's score: right answers, answers one letter off, and the cards
/// scored.
#[derive(Debug, Clone, Copy, Default)]
pub struct Score {
    pub correct: i64,
    pub almost: i64,
    pub total: i64,
}

/// A drill finished, where it was practised from (for its event).
#[cfg(test)]
pub fn lesson_finish_with(
    conn: &mut Conn,
    lesson_id: LessonId,
    score: Score,
    context: Option<&crate::learnlog::PracticeContext>,
) -> Result<LessonFinished> {
    let context = crate::learnlog::resolve(conn, context, Some(lesson_id), "")?;
    conn.transaction(|tx| lesson_finish_steps(tx, lesson_id, score, &context, ""))
}

fn lesson_finish_steps(
    conn: &mut Conn,
    lesson_id: LessonId,
    score: Score,
    context: &crate::learnlog::PracticeContext,
    run: &str,
) -> Result<LessonFinished> {
    // The screen's counts are trusted only so far: never more right than
    // scored, never more almosts than misses.
    let total = score.total.max(0);
    let correct = score.correct.clamp(0, total);
    let almost = score.almost.clamp(0, total - correct);
    let before = unlocked_codes(conn)?;
    crate::learnlog::note(
        conn,
        "lesson_finished",
        "lesson",
        &lesson_id.to_string(),
        Some(lesson_id),
        "",
        crate::learnlog::EventDetails {
            answered: Some(total),
            correct: Some(correct),
            almost: (almost > 0).then_some(almost),
            ..Default::default()
        },
        context,
    )?;
    let today = db::today(conn);
    let current = lessons::load(conn, lesson_id)?;
    // Nothing scored (every card read, or the speaking ones skipped): there
    // is nothing to grade, so no mastery and no schedule change. The day
    // still counts it.
    if total == 0 {
        let first = stats::mark_practised(conn, today)?;
        return Ok(LessonFinished {
            lesson: current,
            result: "fail".into(),
            graded: false,
            extra: false,
            correct,
            almost,
            total,
            mastery: None,
            session: finish_up(conn, &before, first)?,
        });
    }
    let result = lessons::result_from_score(correct, almost, total);
    // Only a lesson that is due (or new) moves its schedule. Practising one
    // early is extra practice: it counts for the day, but a pass must not
    // push the next check-in further out — three quick replays would
    // otherwise send it months away.
    let extra = lessons::is_extra_practice(&current, today);
    // Nearly all right is an ace: the gap grows faster. Only answers fully
    // right count toward it: an almost is a slip, however small.
    let ace = almost == 0 && correct * 100 >= total * 95;
    let go = lessons::Go { via: "drill", run, score: Some((correct, almost, total)), extra };
    let (lesson, entry) = if extra {
        // Extra practice leaves the schedule alone, but still earns mastery.
        let entry = lessons::credit_mastery(conn, &current, result, ace, today, &go)?;
        (lessons::load(conn, lesson_id)?, entry)
    } else {
        lessons::record_go(conn, lesson_id, result, ace, &go)?
    };
    let first = stats::mark_practised(conn, today)?;
    Ok(LessonFinished {
        lesson,
        result: result.into(),
        graded: true,
        extra,
        correct,
        almost,
        total,
        mastery: Some(entry),
        session: finish_up(conn, &before, first)?,
    })
}

/// Stats after a check-in, for the summary screen. What counts as newly
/// unlocked is what was unlocked since the check-in began.
#[tauri::command]
pub async fn checkin_summary(db: State<'_, Db>, checkin_id: CheckinId, first_completion_today: bool) -> Result<SessionResult> {
    let mut guard = db.lock();
    checkin_summary_in(&mut guard, checkin_id, first_completion_today)
}

pub fn checkin_summary_in(conn: &mut Conn, checkin_id: CheckinId, first_completion_today: bool) -> Result<SessionResult> {
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
