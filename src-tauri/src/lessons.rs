//! Lessons: the unit of learning.
//!
//! A lesson is small and has one job, and its **type** says which (see
//! lesson_types): a drill of exercises on a handful of words or sentences
//! (words, pictures, typing, memory cards, sentences, gaps, a conversation),
//! or a check-in with the examiner — a study text, a song to listen to, or a
//! situation played out in writing with a partner.
//!
//! Every lesson has a **complexity** (low ≈ 1–2 minutes, medium ≈ 5–10, high
//! ≈ 20+), set by how hard its subject is, and its own **check-in period**.
//! Passing a check-in pushes the next one further out; struggling brings it
//! back sooner.

use std::collections::HashMap;

use diesel::dsl::count_star;
use diesel::helper_types::{Asc, Desc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::db::{self, Conn};
use crate::schema::{lessons, checkins, course_lessons, sentences, songs, courses, words};
use crate::text::clean;
use crate::error::{Error, Result};

/// Words per word drill when a course's words are cut into drills: enough to
/// be worth it, small enough to finish in a minute or two.
pub const WORDS_PER_LESSON: usize = 8;
pub const SENTENCES_PER_LESSON: usize = 5;
/// Longest gap between check-ins, however often a lesson was passed.
const MAX_INTERVAL: i64 = 120;


/// What a listening lesson checks, by complexity: understanding first, then
/// picking out words, then the grammar of what was sung.
fn listening_objectives(complexity: &str) -> Vec<String> {
    let mut objectives = vec![
        "Say what the chorus means".to_string(),
        "Pick out familiar words heard in the song".to_string(),
    ];
    if complexity != "low" {
        objectives.push("Translate a line heard in a verse".into());
        objectives.push("Fill in a missing word in a line of the song".into());
    }
    if complexity == "high" {
        objectives.push("Explain the grammar of a line (case, verb form, agreement)".into());
        objectives.push("Retell what the song is about in simple Polish".into());
    }
    objectives
}
pub const COMPLEXITIES: &[&str] = &["low", "medium", "high"];


#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Lesson {
    pub id: i64,
    /// Its type: one of lesson_types::REGISTRY.
    #[serde(rename = "type")]
    pub lesson_type: String,
    /// How it is practised, from its type: "exercises" | "conversation".
    pub engine: &'static str,
    pub title: String,
    pub summary: String,
    /// "low" | "medium" | "high"
    pub complexity: String,
    /// The first course it was listed in, for display. None only for a lesson
    /// that is about to go.
    pub course_id: Option<i64>,
    /// That course's title.
    pub course_title: Option<String>,
    /// Every course that lists it.
    pub course_ids: Vec<i64>,
    /// Who made it: "starter" (came with the app) | "tutor" | "you".
    /// Informational only — every lesson can be edited and deleted.
    pub origin: String,
    pub period_days: i64,
    pub interval_days: i64,
    /// None until the lesson has been done once.
    pub due_day: Option<i64>,
    pub last_day: Option<i64>,
    /// "pass" | "partial" | "fail"
    pub last_result: Option<String>,
    pub reps: i64,
    /// How fast its gaps grow (see plan.rs).
    pub ease: f64,
    /// Writing lessons: 0 guided, 1 supported, 2 free, 3 passed free.
    pub support_level: i64,
    /// Beyond the course's level: needed to master the course, not to
    /// complete it.
    pub challenge: bool,
    /// Passed at least once.
    pub passed: bool,
    /// Mastery points, 0 to [`MASTERY_FULL`] (see [`mastery_gain`]).
    pub mastery: i64,
    /// Mastered: for good — mastery is never lost (see [`stamp_mastered`]).
    pub mastered: bool,
    /// Its own CEFR level, when it differs from its course's (levels.rs).
    pub level: Option<String>,
    pub items: i64,
    /// A listening lesson's song.
    pub song_id: Option<i64>,
}

/// A word of a lesson, with its picture if it has one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordPicture {
    pub word_id: i64,
    pub word: String,
    pub meaning: String,
    pub image: Option<i64>,
}

/// A lesson with everything its page shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LessonDetail {
    #[serde(flatten)]
    pub lesson: Lesson,
    /// Study lessons: the lesson's text, in markdown.
    pub content: String,
    pub objectives: Vec<String>,
    /// Drills: what is practised, Polish and English.
    pub words: Vec<(String, String)>,
    /// The same words with their ids and pictures (picture lessons, and
    /// word drills that were given pictures).
    pub word_pictures: Vec<WordPicture>,
    pub sentences: Vec<(String, String)>,
    /// The last check-in's feedback, if any.
    pub last_feedback: Option<String>,
    /// The courses that list it: (id, English title, Polish title).
    pub courses: Vec<(i64, String, String)>,
    /// A conversation drill's dialogue.
    pub lines: Vec<crate::models::DialogueLine>,
    /// What the last check against the lesson's sources changed, if anything.
    pub fact_note: Option<String>,
    /// When the lesson was last checked against its sources.
    pub fact_checked_at: Option<i64>,
}

/// What the home screen shows: what is due, and what to start next.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub today: i64,
    /// Today's lessons still to do, most important first (see plan.rs).
    pub due: Vec<Lesson>,
    /// Lessons done today.
    pub done_today: i64,
    /// Today's lessons: how many, and how many done.
    pub plan_total: usize,
    pub plan_done: usize,
}

/// What a `lessons` row stores about a lesson itself.
#[derive(Queryable, Selectable)]
#[diesel(table_name = lessons, check_for_backend(diesel::sqlite::Sqlite))]
struct LessonRow {
    id: i64,
    lesson_type: String,
    title: String,
    summary: String,
    complexity: String,
    origin: String,
    period_days: i64,
    interval_days: i64,
    due_day: Option<i64>,
    last_day: Option<i64>,
    last_result: Option<String>,
    reps: i64,
    ease: f64,
    support_level: i64,
    challenge: i64,
    passed: i64,
    mastery: i64,
    level: Option<String>,
    song_id: Option<i64>,
    mastered_at: Option<i64>,
}

/// A new `lessons` row.
#[derive(Insertable, Default)]
#[diesel(table_name = lessons)]
struct NewLesson<'a> {
    slug: Option<&'a str>,
    lesson_type: &'a str,
    ord: i64,
    title: &'a str,
    summary: &'a str,
    complexity: &'a str,
    content: &'a str,
    objectives: &'a str,
    period_days: i64,
    origin: &'a str,
    created_at: i64,
    song_id: Option<i64>,
}

fn insert(conn: &mut Conn, lesson: NewLesson) -> Result<i64> {
    Ok(diesel::insert_into(lessons::table).values(&lesson).returning(lessons::id).get_result(conn)?)
}

/// Completes lesson rows with what lives elsewhere: the courses that list
/// them (the first one also by title) and how many items they own.
fn assemble(conn: &mut Conn, rows: Vec<LessonRow>) -> Result<Vec<Lesson>> {
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let mut listed: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
    for (lesson, course, title) in course_lessons::table
        .inner_join(courses::table)
        .filter(course_lessons::lesson_id.eq_any(&ids))
        .order(course_lessons::rowid)
        .select((course_lessons::lesson_id, courses::id, courses::title_en))
        .load::<(i64, i64, String)>(conn)?
    {
        listed.entry(lesson).or_default().push((course, title));
    }
    let mut items: HashMap<i64, i64> = HashMap::new();
    for (lesson, n) in words::table
        .filter(words::lesson_id.eq_any(&ids))
        .group_by(words::lesson_id)
        .select((words::lesson_id, count_star()))
        .load::<(i64, i64)>(conn)?
        .into_iter()
        .chain(
            sentences::table
                .filter(sentences::lesson_id.eq_any(&ids))
                .group_by(sentences::lesson_id)
                .select((sentences::lesson_id, count_star()))
                .load::<(i64, i64)>(conn)?,
        )
    {
        *items.entry(lesson).or_default() += n;
    }
    Ok(rows
        .into_iter()
        .map(|row| {
            let courses = listed.remove(&row.id).unwrap_or_default();
            Lesson {
                course_id: courses.first().map(|c| c.0),
                course_title: courses.first().map(|c| c.1.clone()),
                course_ids: courses.iter().map(|c| c.0).collect(),
                items: items.get(&row.id).copied().unwrap_or(0),
                id: row.id,
                engine: crate::lesson_types::get(&row.lesson_type).map_or("exercises", |t| t.engine()),
                lesson_type: row.lesson_type,
                title: row.title,
                summary: row.summary,
                complexity: row.complexity,
                origin: row.origin,
                period_days: row.period_days,
                interval_days: row.interval_days,
                due_day: row.due_day,
                last_day: row.last_day,
                last_result: row.last_result,
                reps: row.reps,
                ease: row.ease,
                support_level: row.support_level,
                challenge: row.challenge == 1,
                passed: row.passed == 1,
                mastery: row.mastery,
                level: row.level,
                song_id: row.song_id,
                mastered: row.mastered_at.is_some(),
            }
        })
        .collect())
}

pub fn load(conn: &mut Conn, id: i64) -> Result<Lesson> {
    let row = lessons::table
        .find(id)
        .select(LessonRow::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg(format!("There is no lesson with id {id}.")))?;
    Ok(assemble(conn, vec![row])?.remove(0))
}

pub fn detail(conn: &mut Conn, id: i64) -> Result<LessonDetail> {
    let lesson = load(conn, id)?;
    let (content, objectives, fact_note, fact_checked_at): (String, String, Option<String>, Option<i64>) = lessons::table
        .find(id)
        .select((lessons::content, lessons::objectives, lessons::fact_note, lessons::fact_checked_at))
        .first(conn)?;
    let words = words::table
        .filter(words::lesson_id.eq(id))
        .order((words::ord, words::id))
        .select((words::pl, words::en))
        .load(conn)?;
    let sentences = sentences::table
        .filter(sentences::lesson_id.eq(id))
        .order((sentences::ord, sentences::id))
        .select((sentences::pl, sentences::en))
        .load(conn)?;
    let courses = course_lessons::table
        .inner_join(courses::table)
        .filter(course_lessons::lesson_id.eq(id))
        .order(course_lessons::rowid)
        .select((courses::id, courses::title_en, courses::title_pl))
        .load(conn)?;
    let last_feedback = checkins::table
        .filter(checkins::lesson_id.eq(id))
        .filter(checkins::finished_at.is_not_null())
        .order(checkins::id.desc())
        .select(checkins::feedback)
        .first::<Option<String>>(conn)
        .optional()?
        .flatten();
    let word_pictures = words::table
        .filter(words::lesson_id.eq(id))
        .order((words::ord, words::id))
        .select((words::id, words::pl, words::en, words::image_file_id))
        .load::<(i64, String, String, Option<i64>)>(conn)?
        .into_iter()
        .map(|(word_id, word, meaning, image)| WordPicture { word_id, word, meaning, image })
        .collect();
    Ok(LessonDetail {
        word_pictures,
        words,
        sentences,
        lines: crate::session::lesson_lines(conn, id)?,
        courses,
        content,
        objectives: serde_json::from_str(&objectives).unwrap_or_default(),
        last_feedback,
        fact_note,
        fact_checked_at,
        lesson,
    })
}

/// How courses are listed everywhere: the newest first (the starter courses
/// are dated 0), then in their course order.
pub fn course_order() -> (Desc<courses::created_at>, Asc<courses::ord>, Asc<courses::id>) {
    (courses::created_at.desc(), courses::ord.asc(), courses::id.asc())
}

/// A course's lessons, in the course's order.
pub fn for_course(conn: &mut Conn, course_id: i64) -> Result<Vec<Lesson>> {
    let rows = course_lessons::table
        .inner_join(lessons::table)
        .filter(course_lessons::course_id.eq(course_id))
        .order((course_lessons::ord, lessons::id))
        .select(LessonRow::as_select())
        .load(conn)?;
    assemble(conn, rows)
}

/// The home screen: lessons due now, then a few to start.
pub fn overview(conn: &mut Conn) -> Result<Overview> {
    let today = db::today(conn);
    let plan = crate::plan::plan(conn, today)?;
    let due = load_in_order(conn, &plan.left)?;
    let done_today = lessons::table.filter(lessons::last_day.eq(today)).count().get_result(conn)?;
    Ok(Overview { today, due, done_today, plan_total: plan.total, plan_done: plan.done })
}

/// Lessons by id, in the order given.
fn load_in_order(conn: &mut Conn, ids: &[i64]) -> Result<Vec<Lesson>> {
    let rows = lessons::table.filter(lessons::id.eq_any(ids)).select(LessonRow::as_select()).load(conn)?;
    let mut list = assemble(conn, rows)?;
    list.sort_by_key(|b| ids.iter().position(|id| *id == b.id));
    Ok(list)
}

// ---------------------------------------------------------------------------
// Scheduling
// ---------------------------------------------------------------------------

/// Turns a drill score into a result: most right is a pass.
pub fn result_from_score(correct: i64, total: i64) -> &'static str {
    if total <= 0 {
        return "pass";
    }
    let share = correct as f64 / total as f64;
    if share >= 0.85 {
        "pass"
    } else if share >= 0.6 {
        "partial"
    } else {
        "fail"
    }
}

/// How a check-in went, finer than its result: an ace is a pass with
/// nothing to fix (a drill nearly all right, a check-in with nothing to
/// review), and grows the gap faster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    Ace,
    Pass,
    Partial,
    Fail,
}

impl Grade {
    pub fn of(result: &str, ace: bool) -> Grade {
        match result {
            "pass" if ace => Grade::Ace,
            "pass" => Grade::Pass,
            "partial" => Grade::Partial,
            _ => Grade::Fail,
        }
    }
}

const MIN_EASE: f64 = 1.3;
const MAX_EASE: f64 = 3.0;

/// The next gap and ease after a check-in.
///
/// The gap grows from the time that has actually passed since the last
/// check-in (`elapsed`), times the lesson's ease: on time that is the old
/// gap, early it is less (an early check-in is honest, not a shortcut), late
/// it is more — remembering it after longer is worth more. An ace grows it
/// faster and raises the ease; a partial repeats the lesson's period and
/// lowers it; a fail means tomorrow and lowers it more. The first check-in
/// starts from the period (an ace: three times it).
pub fn next_gap(grade: Grade, reps_before: i64, interval: i64, period: i64, ease: f64, elapsed: i64) -> (i64, f64) {
    let period = period.max(1);
    let (ease, gap) = if reps_before == 0 {
        match grade {
            Grade::Ace => (ease + 0.15, period * 3),
            Grade::Pass => (ease, period),
            Grade::Partial => (ease - 0.15, period),
            Grade::Fail => (ease - 0.2, 1),
        }
    } else {
        // Late counts, but at most twice the gap: a lesson left for a year
        // and still known is not proof of a decade.
        let base = elapsed.clamp(1, (interval * 2).max(1)) as f64;
        match grade {
            Grade::Ace => {
                let ease = ease + 0.15;
                (ease, ((base * ease * 1.3).round() as i64).max(period).max(elapsed + 1))
            }
            Grade::Pass => (ease, ((base * ease).round() as i64).max(period).max(elapsed + 1)),
            Grade::Partial => (ease - 0.15, period),
            Grade::Fail => (ease - 0.2, 1),
        }
    };
    (gap.min(MAX_INTERVAL), ease.clamp(MIN_EASE, MAX_EASE))
}

/// The mastery points at which a lesson is mastered.
pub const MASTERY_FULL: i64 = 100;

/// The gap, in days, at which a pass earns its most mastery points.
const MASTERY_SPACED_DAYS: i64 = 14;

/// The mastery points a check-in earns (or, for a fail, costs).
///
/// Mastery is earned by passing, not by waiting: practising now gets there,
/// it only takes more passes than letting the check-ins space them out.
/// The first pass earns 30. A later pass on the same day as the last
/// attempt earns 10; one a day or more later earns 15, plus up to 20 more
/// as the gap grows to two weeks — remembering it after a while is better
/// evidence than getting it right twice in a row. An ace earns a quarter
/// more; a partial earns nothing; a fail costs 20.
///
/// Spaced out, that is three passes (30 + 35 + 35); a day apart, six; all
/// in one sitting, eight.
pub fn mastery_gain(grade: Grade, passed_before: bool, elapsed: i64) -> i64 {
    let base = match grade {
        Grade::Partial => return 0,
        Grade::Fail => return -20,
        Grade::Ace | Grade::Pass if !passed_before => 30,
        Grade::Ace | Grade::Pass if elapsed <= 0 => 10,
        Grade::Ace | Grade::Pass => 15 + 20 * elapsed.min(MASTERY_SPACED_DAYS) / MASTERY_SPACED_DAYS,
    };
    if grade == Grade::Ace {
        (base * 5 + 2) / 4
    } else {
        base
    }
}

/// Whether a lesson is mastered: passed, with full mastery points, and —
/// for a type that asks it (writing) — done without help (see examiner's
/// support levels).
pub fn is_mastered(passed: bool, mastery: i64, lesson_type: &str, support_level: i64) -> bool {
    let unaided = crate::lesson_types::get(lesson_type).is_ok_and(|t| t.unaided_to_master);
    passed && mastery >= MASTERY_FULL && (!unaided || support_level >= 3)
}

/// Marks a lesson mastered once it has earned it. Mastery is for good
/// (PRINCIPLES.md): once stamped, nothing — a fail included — takes it
/// away, and nothing changes its points or help level again.
pub fn stamp_mastered(conn: &mut Conn, lesson_id: i64) -> Result<bool> {
    let (passed, mastery, lesson_type, support, at): (i64, i64, String, i64, Option<i64>) = lessons::table
        .find(lesson_id)
        .select((lessons::passed, lessons::mastery, lessons::lesson_type, lessons::support_level, lessons::mastered_at))
        .first(conn)?;
    if at.is_some() || !is_mastered(passed == 1, mastery, &lesson_type, support) {
        return Ok(false);
    }
    diesel::update(lessons::table.find(lesson_id)).set(lessons::mastered_at.eq(crate::db::now_secs())).execute(conn)?;
    Ok(true)
}

/// Adds a check-in's mastery points to a lesson, as it stood before the
/// check-in. Every check-in counts, extra practice too.
pub fn credit_mastery(conn: &mut Conn, before: &Lesson, result: &str, ace: bool, today: i64) -> Result<()> {
    if before.mastered {
        return Ok(());
    }
    let elapsed = before.last_day.map_or(0, |last| today - last);
    let gain = mastery_gain(Grade::of(result, ace), before.passed, elapsed);
    let mastery = (before.mastery + gain).clamp(0, MASTERY_FULL);
    diesel::update(lessons::table.find(before.id)).set(lessons::mastery.eq(mastery)).execute(conn)?;
    stamp_mastered(conn, before.id)?;
    Ok(())
}

/// Doing a lesson again on a day it was already passed is extra practice:
/// it counts for the day, but does not move the schedule (three quick
/// replays must not send it months away). After a fail or a partial, doing
/// it again the same day is a real retry: it is recorded, and a pass counts
/// as passing it. Doing one before its day is an early check-in, and does
/// move it — see [`next_gap`].
pub fn is_extra_practice(lesson: &Lesson, today: i64) -> bool {
    lesson.reps > 0 && lesson.last_day == Some(today) && lesson.last_result.as_deref() == Some("pass")
}


/// Records a check-in and schedules the next one (`ace`: a pass with
/// nothing to fix). An early pass never brings the next check-in closer
/// than it already was.
#[cfg(test)]
pub fn record(conn: &mut Conn, lesson_id: i64, result: &str) -> Result<Lesson> {
    record_graded(conn, lesson_id, result, false)
}

pub fn record_graded(conn: &mut Conn, lesson_id: i64, result: &str, ace: bool) -> Result<Lesson> {
    if !["pass", "partial", "fail"].contains(&result) {
        return Err(Error::msg(format!("Unknown result '{result}'.")));
    }
    let lesson = load(conn, lesson_id)?;
    let today = db::today(conn);
    credit_mastery(conn, &lesson, result, ace, today)?;
    let grade = Grade::of(result, ace);
    let elapsed = lesson.last_day.map_or(0, |last| today - last);
    let (gap, ease) = next_gap(grade, lesson.reps, lesson.interval_days, lesson.period_days, lesson.ease, elapsed);
    let mut due = crate::plan::spread(conn, today, gap, lesson_id)?;
    let early = lesson.due_day.is_some_and(|d| d > today);
    if early && matches!(grade, Grade::Ace | Grade::Pass) {
        due = due.max(lesson.due_day.unwrap_or(due));
    }
    diesel::update(lessons::table.find(lesson_id))
        .set((
            lessons::interval_days.eq(due - today),
            lessons::due_day.eq(due),
            lessons::last_day.eq(today),
            lessons::last_result.eq(result),
            lessons::reps.eq(lessons::reps + 1),
            lessons::ease.eq(ease),
        ))
        .execute(conn)?;
    if result == "pass" {
        diesel::update(lessons::table.find(lesson_id)).set(lessons::passed.eq(1)).execute(conn)?;
    }
    crate::plan::log_minutes(conn, today, crate::lesson_types::minutes(&lesson.lesson_type, &lesson.complexity), early)?;
    stamp_mastered(conn, lesson_id)?;
    load(conn, lesson_id)
}

/// Makes a lesson due today (the learner or the tutor wants it now).
pub fn due_now(conn: &mut Conn, lesson_id: i64) -> Result<Lesson> {
    let today = db::today(conn);
    diesel::update(lessons::table.find(lesson_id)).set(lessons::due_day.eq(today)).execute(conn)?;
    load(conn, lesson_id)
}

// ---------------------------------------------------------------------------
// Making and changing lessons
// ---------------------------------------------------------------------------

fn check_complexity(complexity: &str) -> Result<()> {
    if COMPLEXITIES.contains(&complexity) {
        Ok(())
    } else {
        Err(Error::msg("Complexity must be low, medium or high."))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewStudy {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub complexity: String,
    /// The lesson's text, in markdown.
    #[serde(alias = "lesson")]
    pub content: String,
    #[serde(default)]
    pub objectives: Vec<String>,
    #[serde(default)]
    pub period_days: Option<i64>,
    /// The course to file it under; None files it in "Other lessons".
    #[serde(default, alias = "course_id")]
    pub course_id: Option<i64>,
}

pub fn create_study(conn: &mut Conn, input: NewStudy, origin: &str) -> Result<Lesson> {
    let title = clean(&input.title);
    if title.is_empty() || input.content.trim().is_empty() {
        return Err(Error::msg("A study lesson needs a title and content."));
    }
    check_complexity(&input.complexity)?;
    let objectives: Vec<String> =
        input.objectives.iter().map(|o| clean(o)).filter(|o| !o.is_empty()).collect();
    if objectives.is_empty() {
        return Err(Error::msg("A study lesson needs objectives: what the learner should be able to do."));
    }
    let period = input
        .period_days
        .unwrap_or_else(|| crate::lesson_types::get("study").map_or(7, |t| t.default_period(&input.complexity)))
        .clamp(1, 60);
    let course_id = match input.course_id {
        Some(id) => {
            crate::courses::require_open_course(conn, id)?;
            id
        }
        None => crate::courses::other_course(conn)?,
    };
    let id = insert(
        conn,
        NewLesson {
            lesson_type: "study",
            title: &title,
            summary: &clean(&input.summary),
            complexity: &input.complexity,
            content: input.content.trim(),
            objectives: &serde_json::to_string(&objectives)?,
            period_days: period,
            origin,
            created_at: db::now_secs(),
            ..Default::default()
        },
    )?;
    crate::courses::add_lesson(conn, course_id, id)?;
    load(conn, id)
}

/// A writing lesson: a situation the learner writes their way through, with
/// an AI partner who only understands Polish. The goals are what they must
/// get across; the conversation ends when they have.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewWriting {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub complexity: String,
    /// The situation, in English: where they are and who they are talking to.
    #[serde(alias = "scenario", alias = "lesson")]
    pub content: String,
    /// What the learner has to get across, e.g. "Say where you live".
    #[serde(default, alias = "goals")]
    pub objectives: Vec<String>,
    #[serde(default)]
    pub period_days: Option<i64>,
    /// The course to file it under; None files it in "Other lessons".
    #[serde(default, alias = "course_id")]
    pub course_id: Option<i64>,
}

pub fn create_writing(conn: &mut Conn, input: NewWriting, origin: &str) -> Result<Lesson> {
    let title = clean(&input.title);
    if title.is_empty() || input.content.trim().is_empty() {
        return Err(Error::msg("A writing lesson needs a title and a situation."));
    }
    check_complexity(&input.complexity)?;
    let goals: Vec<String> = input.objectives.iter().map(|o| clean(o)).filter(|o| !o.is_empty()).collect();
    if goals.is_empty() {
        return Err(Error::msg("A writing lesson needs goals: what the learner has to get across."));
    }
    let course_id = match input.course_id {
        Some(id) => {
            crate::courses::require_open_course(conn, id)?;
            id
        }
        None => crate::courses::other_course(conn)?,
    };
    let id = insert(
        conn,
        NewLesson {
            lesson_type: "writing",
            title: &title,
            summary: &clean(&input.summary),
            complexity: &input.complexity,
            content: input.content.trim(),
            objectives: &serde_json::to_string(&goals)?,
            period_days: input.period_days.unwrap_or_else(|| crate::lesson_types::get("writing").map_or(7, |t| t.default_period(&input.complexity))).clamp(1, 60),
            origin,
            created_at: db::now_secs(),
            ..Default::default()
        },
    )?;
    crate::courses::add_lesson(conn, course_id, id)?;
    load(conn, id)
}

/// A listening lesson: listen to a song, then the examiner asks about it.
/// One per song — asking again returns the existing lesson.
/// It is filed in `course` if given, else in the course the song was written
/// for, else in "Other lessons". Like any lesson, it can be moved or shared later.
pub fn create_listening(
    conn: &mut Conn,
    song_id: i64,
    complexity: Option<&str>,
    focus: Option<&str>,
    course: Option<i64>,
    origin: &str,
) -> Result<Lesson> {
    if let Some(id) =
        lessons::table.filter(lessons::song_id.eq(song_id)).select(lessons::id).first::<i64>(conn).optional()?
    {
        return load(conn, id);
    }
    let (title, song_course): (String, Option<i64>) = songs::table
        .find(song_id)
        .select((songs::title, songs::course_id))
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg("That song no longer exists."))?;
    let complexity = complexity.unwrap_or("medium");
    check_complexity(complexity)?;
    // The "lesson" is only a note: the lyrics stay hidden until asked for,
    // since the point is to understand them by ear.
    let content = match focus.map(clean).filter(|f| !f.is_empty()) {
        Some(focus) => format!("Listen to the song a couple of times. Pay attention to: {focus}"),
        None => "Listen to the song a couple of times, then check in. Try it without the lyrics first.".into(),
    };
    let course_id = match course.or(song_course) {
        Some(id) => {
            crate::courses::require_open_course(conn, id)?;
            id
        }
        None => crate::courses::other_course(conn)?,
    };
    let id = insert(
        conn,
        NewLesson {
            lesson_type: "listening",
            title: &format!("Listen: {title}"),
            summary: "Listen to the song, then answer questions about what you heard.",
            complexity,
            content: &content,
            objectives: &serde_json::to_string(&listening_objectives(complexity))?,
            period_days: crate::lesson_types::get("listening").map_or(7, |t| t.default_period(complexity)),
            origin,
            created_at: db::now_secs(),
            song_id: Some(song_id),
            ..Default::default()
        },
    )?;
    crate::courses::add_lesson(conn, course_id, id)?;
    load(conn, id)
}

/// A drill that owns the words, sentences or dialogue lines it practises,
/// listed in a course. Items it already holds (by their Polish) are skipped;
/// the count of skipped items is returned with the lesson.
#[allow(clippy::too_many_arguments)]
pub fn create_drill(
    conn: &mut Conn,
    course_id: i64,
    title: &str,
    summary: &str,
    lesson_type: &str,
    complexity: &str,
    items: &crate::courses::Items,
    period_days: Option<i64>,
    origin: &str,
) -> Result<(Lesson, i64)> {
    let kind = crate::lesson_types::get(lesson_type)?;
    if !kind.is_drill() {
        return Err(Error::msg(format!("'{lesson_type}' is not a drill: it is made with its own tool.")));
    }
    check_complexity(complexity)?;
    crate::courses::require_open_course(conn, course_id)?;
    let period = period_days.unwrap_or_else(|| kind.default_period(complexity)).clamp(1, 60);
    let id = insert(
        conn,
        NewLesson {
            lesson_type,
            title: &clean(title),
            summary: &clean(summary),
            complexity,
            objectives: "[]",
            period_days: period,
            origin,
            created_at: db::now_secs(),
            ..Default::default()
        },
    )?;
    let skipped = crate::courses::insert_items(conn, id, items)?;
    crate::courses::add_lesson(conn, course_id, id)?;
    Ok((load(conn, id)?, skipped))
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LessonChanges {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub complexity: Option<String>,
    #[serde(default, alias = "lesson")]
    pub content: Option<String>,
    #[serde(default)]
    pub objectives: Option<Vec<String>>,
    #[serde(default)]
    pub period_days: Option<i64>,
    /// Beyond the course's level: needed to master it, not to complete it.
    #[serde(default)]
    pub challenge: Option<bool>,
}

/// The columns an edit may change; `None` leaves one as it is.
#[derive(AsChangeset, Default, PartialEq)]
#[diesel(table_name = lessons)]
struct LessonEdit {
    period_days: Option<i64>,
    title: Option<String>,
    summary: Option<String>,
    complexity: Option<String>,
    content: Option<String>,
    objectives: Option<String>,
    challenge: Option<i64>,
    /// A writing lesson's word bank goes stale when its situation or goals
    /// change: Some(None) clears it.
    scaffold: Option<Option<String>>,
}

pub fn update(conn: &mut Conn, id: i64, changes: LessonChanges) -> Result<Lesson> {
    load(conn, id)?;
    if let Some(complexity) = &changes.complexity {
        check_complexity(complexity)?;
    }
    let edit = LessonEdit {
        period_days: changes.period_days.map(|p| p.clamp(1, 60)),
        title: changes.title.map(|t| clean(&t)).filter(|t| !t.is_empty()),
        summary: changes.summary.map(|s| clean(&s)),
        complexity: changes.complexity,
        content: changes.content.map(|l| l.trim().to_string()).filter(|l| !l.is_empty()),
        objectives: changes
            .objectives
            .map(|list| list.iter().map(|o| clean(o)).filter(|o| !o.is_empty()).collect::<Vec<_>>())
            .map(|list| serde_json::to_string(&list))
            .transpose()?,
        challenge: changes.challenge.map(i64::from),
        scaffold: None,
    };
    let mut edit = edit;
    if edit.content.is_some() || edit.objectives.is_some() {
        edit.scaffold = Some(None);
    }
    if edit != LessonEdit::default() {
        diesel::update(lessons::table.find(id)).set(&edit).execute(conn)?;
    }
    load(conn, id)
}

/// Deletes a lesson everywhere, with its words, sentences, dialogue and
/// check-in history.
pub fn delete(conn: &mut Conn, id: i64) -> Result<String> {
    let lesson = load(conn, id)?;
    diesel::delete(lessons::table.find(id)).execute(conn)?;
    Ok(lesson.title)
}

// ---------------------------------------------------------------------------
// Starter lessons
// ---------------------------------------------------------------------------

#[cfg(test)]
pub struct LessonFile {
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub complexity: String,
    pub period: i64,
    pub objectives: Vec<String>,
    pub body: String,
}

/// Reads a lesson: a `---` front matter lesson of `key: value` lines (and a
/// `- ` list under `objectives:`), then the markdown lesson.
#[cfg(test)]
pub fn parse_lesson(text: &str) -> Result<LessonFile> {
    let rest = text
        .strip_prefix("---\n")
        .ok_or_else(|| Error::msg("lesson without front matter"))?;
    let (head, body) = rest
        .split_once("\n---\n")
        .ok_or_else(|| Error::msg("lesson front matter is not closed"))?;
    let mut file = LessonFile {
        slug: String::new(),
        title: String::new(),
        summary: String::new(),
        complexity: "medium".into(),
        period: 7,
        objectives: vec![],
        body: body.trim().to_string(),
    };
    for line in head.lines() {
        if let Some(item) = line.strip_prefix("- ") {
            file.objectives.push(item.trim().to_string());
            continue;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim().to_string();
        match key.trim() {
            "slug" => file.slug = value,
            "title" => file.title = value,
            "summary" => file.summary = value,
            "complexity" => file.complexity = value,
            "period" => file.period = value.parse().unwrap_or(7),
            _ => {}
        }
    }
    if file.slug.is_empty() || file.title.is_empty() || file.objectives.is_empty() {
        return Err(Error::msg(format!("lesson '{}' is missing slug, title or objectives", file.slug)));
    }
    if !COMPLEXITIES.contains(&file.complexity.as_str()) {
        return Err(Error::msg(format!("lesson '{}' has complexity '{}'", file.slug, file.complexity)));
    }
    Ok(file)
}

/// Writes the starter lessons, on a fresh install (filed in a course by
/// `db::unify_courses`).
#[cfg(test)]
pub fn seed_lessons(conn: &mut Conn) -> Result<()> {
    for (ord, text) in crate::content::LESSONS.iter().enumerate() {
        let file = parse_lesson(text)?;
        let slug = format!("lesson/{}", file.slug);
        // Written once: a lesson already there (by its slug) is left alone.
        let added: Option<i64> = diesel::insert_or_ignore_into(lessons::table)
            .values(NewLesson {
                slug: Some(&slug),
                lesson_type: "study",
                ord: ord as i64,
                title: &file.title,
                summary: &file.summary,
                complexity: &file.complexity,
                content: &file.body,
                objectives: &serde_json::to_string(&file.objectives)?,
                period_days: file.period,
                origin: "starter",
                created_at: db::now_secs(),
                ..Default::default()
            })
            .returning(lessons::id)
            .get_result(conn)
            .optional()?;
        if let Some(lesson) = added {
            let grammar = crate::courses::grammar_course(conn)?;
            crate::courses::add_lesson(conn, grammar, lesson)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

pub mod commands {
    use tauri::State;

    use crate::db::Db;
    use crate::error::Result;

    #[tauri::command]
    pub async fn lessons_overview(db: State<'_, Db>) -> Result<super::Overview> {
        super::overview(&mut db.lock())
    }

    #[tauri::command]
    pub async fn lessons_for_course(db: State<'_, Db>, course_id: i64) -> Result<Vec<super::Lesson>> {
        super::for_course(&mut db.lock(), course_id)
    }

    /// A course's lessons, most important first, the mastered ones last —
    /// its page's list (see plan::order_for_course).
    #[tauri::command]
    pub async fn lessons_by_importance(db: State<'_, Db>, course_id: i64) -> Result<Vec<super::Lesson>> {
        let mut guard = db.lock();
        let lessons = super::for_course(&mut guard, course_id)?;
        crate::plan::order_for_course(&mut guard, lessons)
    }

    #[tauri::command]
    pub async fn lesson_detail(db: State<'_, Db>, lesson_id: i64) -> Result<super::LessonDetail> {
        super::detail(&mut db.lock(), lesson_id)
    }

    /// The learner changes how often a lesson checks in.
    #[tauri::command]
    pub async fn lesson_set_period(db: State<'_, Db>, lesson_id: i64, period_days: i64) -> Result<super::Lesson> {
        super::update(
            &mut db.lock(),
            lesson_id,
            super::LessonChanges { period_days: Some(period_days), ..Default::default() },
        )
    }

    /// The learner marks a lesson as a challenge (or back into the course).
    #[tauri::command]
    pub async fn lesson_set_challenge(db: State<'_, Db>, lesson_id: i64, challenge: bool) -> Result<super::Lesson> {
        super::update(&mut db.lock(), lesson_id, super::LessonChanges { challenge: Some(challenge), ..Default::default() })
    }

    /// The listening lesson for a song, made on first ask.
    #[tauri::command]
    pub async fn lesson_for_song(db: State<'_, Db>, song_id: i64) -> Result<super::Lesson> {
        super::create_listening(&mut db.lock(), song_id, None, None, None, "you")
    }

    #[tauri::command]
    pub async fn lesson_delete(db: State<'_, Db>, lesson_id: i64) -> Result<()> {
        super::delete(&mut db.lock(), lesson_id).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mastery_is_earned_faster_when_spaced_but_never_needs_waiting() {
        use Grade::*;
        assert_eq!(mastery_gain(Pass, false, 0), 30);
        assert_eq!(mastery_gain(Ace, false, 0), 38);
        assert_eq!(mastery_gain(Pass, true, 0), 10);
        assert_eq!(mastery_gain(Pass, true, 1), 16);
        assert_eq!(mastery_gain(Pass, true, 7), 25);
        assert_eq!(mastery_gain(Pass, true, 14), 35);
        assert_eq!(mastery_gain(Pass, true, 60), 35);
        assert_eq!(mastery_gain(Ace, true, 14), 44);
        assert_eq!(mastery_gain(Partial, true, 14), 0);
        assert_eq!(mastery_gain(Fail, true, 14), -20);
        let passes = |gap: i64| {
            let (mut points, mut n) = (mastery_gain(Pass, false, 0), 1);
            while points < MASTERY_FULL {
                points += mastery_gain(Pass, true, gap);
                n += 1;
            }
            n
        };
        assert_eq!((passes(14), passes(1), passes(0)), (3, 6, 8));
        assert!(is_mastered(true, 100, "words", 0));
        assert!(!is_mastered(true, 100, "writing", 1), "writing also needs doing without help");
        assert!(!is_mastered(false, 100, "words", 0));
    }

    #[test]
    fn check_ins_spread_out_when_passed_and_come_back_when_failed() {
        use Grade::*;
        assert_eq!(next_gap(Pass, 0, 0, 3, 2.5, 0), (3, 2.5), "first pass: the period");
        assert_eq!(next_gap(Ace, 0, 0, 2, 2.5, 0).0, 6, "first ace: three periods");
        // On time: the gap times the ease.
        assert_eq!(next_gap(Pass, 1, 4, 2, 2.5, 4).0, 10);
        // Aced: faster, and the ease goes up.
        let (gap, ease) = next_gap(Ace, 1, 4, 2, 2.5, 4);
        assert_eq!(gap, 14);
        assert!(ease > 2.5);
        // Early: from the time that actually passed — but never no growth.
        assert_eq!(next_gap(Pass, 2, 10, 2, 2.5, 7).0, 18);
        assert_eq!(next_gap(Pass, 2, 10, 2, 1.3, 1).0, 2, "at least the period");
        assert_eq!(next_gap(Pass, 5, 100, 3, 2.5, 100).0, MAX_INTERVAL);
        let (gap, ease) = next_gap(Partial, 4, 24, 3, 2.5, 24);
        assert_eq!(gap, 3);
        assert!(ease < 2.5);
        assert_eq!(next_gap(Fail, 4, 24, 3, 1.35, 24), (1, MIN_EASE), "ease has a floor");
    }

    #[test]
    fn drill_scores_become_results() {
        assert_eq!(result_from_score(9, 10), "pass");
        assert_eq!(result_from_score(7, 10), "partial");
        assert_eq!(result_from_score(3, 10), "fail");
        assert_eq!(result_from_score(0, 0), "pass");
    }

    #[test]
    fn every_built_in_lesson_parses() {
        assert!(crate::content::LESSONS.len() >= 6);
        for text in crate::content::LESSONS {
            let file = parse_lesson(text).unwrap();
            assert!(file.body.len() > 200, "{} is too thin", file.slug);
            assert!(file.objectives.len() >= 2, "{}", file.slug);
        }
    }
}
