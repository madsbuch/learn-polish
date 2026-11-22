//! Lessons: the unit of learning.
//!
//! A lesson is small and has one job, and its **type** says which (see
//! lesson_types): a drill of exercises on a handful of words or sentences
//! (words, pictures, a scene, a story, typing, memory cards, sentences, gaps, a dialogue), or a
//! conversation with the examiner — a study text, a song to listen to, or a
//! situation played out in writing with a partner.
//!
//! Every lesson has a **complexity** (low, medium, high: how long and how
//! deep; minutes in `lesson_types::minutes`), and its own **check-in period**.
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
#[allow(unused_imports)]
use crate::ids::*;

/// Words per word drill when a course's words are cut into drills: enough to
/// be worth it, small enough to finish in a minute or two.
// Spec: CRS-2
pub const WORDS_PER_LESSON: usize = 8;
// Spec: CRS-2
pub const SENTENCES_PER_LESSON: usize = 5;
/// Longest gap between check-ins, however often a lesson was passed.
// Spec: SCHED-10
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
    pub id: LessonId,
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
    pub course_id: Option<CourseId>,
    /// That course's title.
    pub course_title: Option<String>,
    /// The course that lists it, as a list (a lesson is in one course; none
    /// only while it is being made or deleted).
    pub course_ids: Vec<CourseId>,
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
    pub song_id: Option<SongId>,
    /// About how many more passes it needs to be mastered (see [`passes_left`]).
    pub passes_left: PassesLeft,
}

/// About how many more passes a lesson needs to be mastered: `spaced`,
/// passing each check-in when it comes back; `now`, practising in one
/// sitting from now (with one more pass on another day when the last
/// stretch needs it: see [`MASTERY_HELD_AT`]). Both 0 once mastered.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PassesLeft {
    pub spaced: i64,
    pub now: i64,
}

/// A word of a lesson, with its picture if it has one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordPicture {
    pub word_id: WordId,
    pub word: String,
    pub meaning: String,
    pub image: Option<FileId>,
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
    /// Its course, as a list: (id, English title, Polish title).
    pub courses: Vec<(CourseId, String, String)>,
    /// A conversation drill's dialogue.
    pub lines: Vec<crate::models::DialogueLine>,
    /// What the last check against the lesson's sources changed, if anything.
    pub fact_note: Option<String>,
    /// When the lesson was last checked against its sources.
    pub fact_checked_at: Option<i64>,
    /// A scene lesson's picture, where its things are, and how far along it
    /// is (scenes.rs); None for every other lesson.
    pub scene: Option<crate::scenes::SceneInfo>,
    /// A story lesson's story, its panels and how far their pictures are
    /// (stories.rs); None for every other lesson.
    pub story: Option<crate::stories::StoryInfo>,
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
    id: LessonId,
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
    song_id: Option<SongId>,
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
    song_id: Option<SongId>,
}

fn insert(conn: &mut Conn, lesson: NewLesson) -> Result<LessonId> {
    Ok(diesel::insert_into(lessons::table)
        .values((lessons::id.eq(LessonId::new()), &lesson))
        .returning(lessons::id)
        .get_result(conn)?)
}

/// Completes lesson rows with what lives elsewhere: the course that lists
/// them (also by title) and how many items they own.
fn assemble(conn: &mut Conn, rows: Vec<LessonRow>) -> Result<Vec<Lesson>> {
    let ids: Vec<LessonId> = rows.iter().map(|r| r.id).collect();
    let today = db::today(conn);
    let first_today = crate::masterylog::first_passed_on(conn, today)?;
    let mut listed: HashMap<LessonId, Vec<(CourseId, String)>> = HashMap::new();
    for (lesson, course, title) in course_lessons::table
        .inner_join(courses::table)
        .filter(course_lessons::lesson_id.eq_any(&ids))
        .order(course_lessons::rowid)
        .select((course_lessons::lesson_id, courses::id, courses::title_en))
        .load::<(LessonId, CourseId, String)>(conn)?
    {
        listed.entry(lesson).or_default().push((course, title));
    }
    let mut items: HashMap<LessonId, i64> = HashMap::new();
    for (lesson, n) in words::table
        .filter(words::lesson_id.eq_any(&ids))
        .group_by(words::lesson_id)
        .select((words::lesson_id, count_star()))
        .load::<(LessonId, i64)>(conn)?
        .into_iter()
        .chain(
            sentences::table
                .filter(sentences::lesson_id.eq_any(&ids))
                .group_by(sentences::lesson_id)
                .select((sentences::lesson_id, count_star()))
                .load::<(LessonId, i64)>(conn)?,
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
                passes_left: PassesLeft::default(),
            }
        })
        .map(|mut lesson| {
            lesson.passes_left = passes_left(&lesson, first_today.contains(&lesson.id));
            lesson
        })
        .collect())
}

/// Lessons by id (in no particular order).
pub fn load_many(conn: &mut Conn, ids: &[LessonId]) -> Result<Vec<Lesson>> {
    let rows = lessons::table.filter(lessons::id.eq_any(ids)).select(LessonRow::as_select()).load(conn)?;
    assemble(conn, rows)
}

/// About how many more passes a lesson needs to be mastered, by the rules
/// as they are (`first_passed_today`: its first pass was today, so the last
/// stretch needs another day). Spaced: each pass when the check-in comes
/// back, its gap following [`next_gap`] from the lesson's own period, gap
/// and ease, and earning [`mastery_gain`] for that gap. Now: one sitting at
/// 10 a pass (30 for a first pass), up to [`MASTERY_HELD_AT`] when it is
/// held, then one pass on another day. A lesson mastered only without help
/// (writing) needs at least a pass per help level left.
pub fn passes_left(lesson: &Lesson, first_passed_today: bool) -> PassesLeft {
    if lesson.mastered {
        return PassesLeft::default();
    }
    let unaided = crate::lesson_types::get(&lesson.lesson_type).is_ok_and(|t| t.unaided_to_master);
    let steps = if unaided { (3 - lesson.support_level).max(0) } else { 0 };
    let start = lesson.mastery.clamp(0, MASTERY_FULL);

    let (mut points, mut passed, mut reps, mut interval, mut ease) = (start, lesson.passed, lesson.reps, lesson.interval_days, lesson.ease);
    let mut spaced = 0;
    while (points < MASTERY_FULL || spaced < steps) && spaced < 40 {
        let elapsed = if reps == 0 { 0 } else { interval.max(1) };
        if passed {
            points = (points + mastery_gain(Grade::Pass, true, elapsed)).min(MASTERY_FULL);
        } else {
            // The first pass, on its own day: held until another.
            points = (points + mastery_gain(Grade::Pass, false, elapsed)).min(MASTERY_HELD_AT.max(start));
            passed = true;
        }
        let (gap, next_ease) = next_gap(Grade::Pass, reps, interval, lesson.period_days, ease, elapsed);
        (reps, interval, ease) = (reps + 1, gap, next_ease);
        spaced += 1;
    }

    let held = !lesson.passed || first_passed_today;
    let (mut points, mut now) = (start, 0);
    if !lesson.passed {
        points += mastery_gain(Grade::Pass, false, 0);
        now += 1;
    }
    let limit = if held { MASTERY_HELD_AT.max(start) } else { MASTERY_FULL };
    while points < limit.min(MASTERY_FULL) {
        points += mastery_gain(Grade::Pass, true, 0);
        now += 1;
    }
    if points < MASTERY_FULL {
        // The last stretch, on another day.
        now += 1;
    }
    PassesLeft { spaced: spaced.max(steps), now: now.max(steps) }
}

pub fn load(conn: &mut Conn, id: LessonId) -> Result<Lesson> {
    let row = lessons::table
        .find(id)
        .select(LessonRow::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg(format!("There is no lesson with id {id}.")))?;
    Ok(assemble(conn, vec![row])?.remove(0))
}

pub fn detail(conn: &mut Conn, id: LessonId) -> Result<LessonDetail> {
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
        .load::<(WordId, String, String, Option<FileId>)>(conn)?
        .into_iter()
        .map(|(word_id, word, meaning, image)| WordPicture { word_id, word, meaning, image })
        .collect();
    let scene = crate::scenes::info(conn, id)?;
    let story = crate::stories::info(conn, id)?;
    Ok(LessonDetail {
        scene,
        story,
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
pub fn for_course(conn: &mut Conn, course_id: CourseId) -> Result<Vec<Lesson>> {
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
fn load_in_order(conn: &mut Conn, ids: &[LessonId]) -> Result<Vec<Lesson>> {
    let rows = lessons::table.filter(lessons::id.eq_any(ids)).select(LessonRow::as_select()).load(conn)?;
    let mut list = assemble(conn, rows)?;
    list.sort_by_key(|b| ids.iter().position(|id| *id == b.id));
    Ok(list)
}

// ---------------------------------------------------------------------------
// Scheduling
// ---------------------------------------------------------------------------

/// The share of a drill that passes it.
// Spec: DRILL-6
pub const PASS_SHARE: f64 = 0.85;
/// The share that is partly there.
// Spec: DRILL-7
const PARTIAL_SHARE: f64 = 0.6;
/// [`PASS_SHARE`] in percent, for the mastery credit's whole-number sums.
// Spec: MAST-5
const PASS_PERCENT: i64 = 85;
/// At or below this share right (percent), a drill earns no mastery points.
// Spec: MAST-5
pub const CREDIT_FROM_PERCENT: i64 = 50;
/// A drill this small (or smaller) passes with one miss: 85% of six cards
/// is more than five, so otherwise a single slip would cost the pass.
pub const SMALL_DRILL: i64 = 6;

/// Turns a drill's score into a result: right answers and answers almost
/// right (each half right) out of the cards scored. A pass is 85% — or, in
/// a drill of three to six scored cards, at most one miss (two almosts are
/// one miss); 60% is partly there; less is a fail. A drill with nothing
/// scored is not graded at all (see `practice::lesson_finish`), so it never
/// gets here with `total` 0 — if it does, it is a fail, never a free pass.
// Spec: DRILL-6, DRILL-7
pub fn result_from_score(correct: i64, almost: i64, total: i64) -> &'static str {
    if total <= 0 {
        return "fail";
    }
    let correct = correct.clamp(0, total);
    let almost = almost.clamp(0, total - correct);
    // In half points: a miss is 2, an almost 1.
    let (points, out_of) = (correct * 2 + almost, total * 2);
    let share = crate::convert::real(points) / crate::convert::real(out_of);
    let one_miss = (3..=SMALL_DRILL).contains(&total) && out_of - points <= 2;
    if share >= PASS_SHARE || one_miss {
        "pass"
    } else if share >= PARTIAL_SHARE {
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
    /// Its word in the mastery ledger and on the summary.
    pub fn key(self) -> &'static str {
        match self {
            Grade::Ace => "ace",
            Grade::Pass => "pass",
            Grade::Partial => "partial",
            Grade::Fail => "fail",
        }
    }

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
/// A new lesson's ease (the schema's default).
// Spec: SCHED-9
pub const START_EASE: f64 = 2.5;

/// The next gap and ease after a check-in.
///
/// The gap grows from the time that has actually passed since the last
/// check-in (`elapsed`), times the lesson's ease: on time that is the old
/// gap, early it is less (an early check-in is honest, not a shortcut), late
/// it is more — remembering it after longer is worth more. An ace grows it
/// faster and raises the ease; a partial repeats the lesson's period and
/// lowers it; a fail means tomorrow and lowers it more. The first check-in
/// starts from the period (an ace: three times it).
// Spec: SCHED-3, SCHED-4, SCHED-5, SCHED-6, SCHED-7, SCHED-8
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
        let base = crate::convert::real(elapsed.clamp(1, (interval * 2).max(1)));
        match grade {
            Grade::Ace => {
                let ease = ease + 0.15;
                (ease, crate::convert::round(base * ease * 1.3).max(period).max(elapsed + 1))
            }
            Grade::Pass => (ease, crate::convert::round(base * ease).max(period).max(elapsed + 1)),
            Grade::Partial => (ease - 0.15, period),
            Grade::Fail => (ease - 0.2, 1),
        }
    };
    (gap.min(MAX_INTERVAL), ease.clamp(MIN_EASE, MAX_EASE))
}

/// The mastery points at which a lesson is mastered.
// Spec: MAST-7, MAST-8
pub const MASTERY_FULL: i64 = 100;

/// The gap, in days, at which a pass earns its most mastery points.
const MASTERY_SPACED_DAYS: i64 = 14;

/// The mastery points a go earns, by its grade alone (a conversation; a
/// drill's partial and fail earn by their score: [`credit`]). Never fewer
/// than none: mastery only grows.
///
/// Mastery is earned by passing, not by waiting: practising now gets there,
/// it only takes more passes than letting the check-ins space them out.
/// The first pass earns 30. A later pass on the same day as the last
/// attempt earns 10; one a day or more later earns 15, plus up to 20 more
/// as the gap grows to two weeks — remembering it after a while is better
/// evidence than getting it right twice in a row. An ace earns a quarter
/// more; a partial half what a pass would in the same situation, rounded
/// down; a fail nothing — the schedule answers it, bringing the lesson
/// back tomorrow.
///
/// The last stretch takes two days: see [`MASTERY_HELD_AT`].
// Spec: MAST-1, MAST-2, MAST-3, MAST-4, MAST-5, MAST-6
pub fn mastery_gain(grade: Grade, passed_before: bool, elapsed: i64) -> i64 {
    let base = if !passed_before {
        30
    } else if elapsed <= 0 {
        10
    } else {
        15 + 20 * elapsed.min(MASTERY_SPACED_DAYS) / MASTERY_SPACED_DAYS
    };
    match grade {
        Grade::Ace => (base * 5 + 2) / 4,
        Grade::Pass => base,
        Grade::Partial => base / 2,
        Grade::Fail => 0,
    }
}

/// The mastery points a go short of a pass earns: a share of what a pass
/// would in the same situation (`pass`). A drill (`score`: right, almost,
/// scored) earns none at or below [`CREDIT_FROM_PERCENT`] right and all of
/// it at [`PASS_SHARE`], rising linearly between (an almost counts half),
/// rounded down — every right answer counts, with no cliff at the partial's
/// line. A conversation, without a score, earns by its grade.
// Spec: MAST-5, MAST-6
pub fn credit(pass: i64, grade: Grade, score: Option<(i64, i64, i64)>) -> i64 {
    match (grade, score) {
        (Grade::Ace | Grade::Pass, _) => pass,
        (_, Some((correct, almost, total))) if total > 0 => {
            // In half points and percent, to stay in whole numbers.
            let (points, out_of) = (correct.clamp(0, total) * 2 + almost.max(0), total * 2);
            let above = points * 100 - CREDIT_FROM_PERCENT * out_of;
            let span = (PASS_PERCENT - CREDIT_FROM_PERCENT) * out_of;
            (pass * above.max(0) / span).clamp(0, pass)
        }
        (Grade::Partial, _) => pass / 2,
        (Grade::Fail, _) => 0,
    }
}

/// Whether a lesson is mastered: passed, with full mastery points, and —
/// for a type that asks it (writing) — done without help (see examiner's
/// support levels).
// Spec: MAST-8, CONV-10
pub fn is_mastered(passed: bool, mastery: i64, lesson_type: &str, support_level: i64) -> bool {
    let unaided = crate::lesson_types::get(lesson_type).is_ok_and(|t| t.unaided_to_master);
    passed && mastery >= MASTERY_FULL && (!unaided || support_level >= 3)
}

/// Where a lesson's points stop until it has been passed on two different
/// days: 90. Mastering takes more than one sitting — a lesson first passed
/// today can reach 90 today, and the last stretch comes with a pass on
/// another day (any later day: no waiting for a check-in). Before its first
/// pass, partials stop there too. A lesson passed before the mastery ledger
/// began counts as passed on an earlier day.
// Spec: MAST-7
pub const MASTERY_HELD_AT: i64 = 90;

/// Marks a lesson mastered once it has earned it. Mastery is for good
/// (PRINCIPLES.md): once stamped, nothing — a fail included — takes it
/// away, and nothing changes its points or help level again. The stamp is
/// a row of its own in the mastery ledger.
#[cfg(test)]
pub fn stamp_mastered(conn: &mut Conn, lesson_id: LessonId) -> Result<bool> {
    stamp_mastered_for(conn, lesson_id, &Go::default())
}

/// [`stamp_mastered`], for a go (its row in the ledger names it).
pub fn stamp_mastered_for(conn: &mut Conn, lesson_id: LessonId, go: &Go) -> Result<bool> {
    let (passed, mastery, lesson_type, support, at): (i64, i64, String, i64, Option<i64>) = lessons::table
        .find(lesson_id)
        .select((lessons::passed, lessons::mastery, lessons::lesson_type, lessons::support_level, lessons::mastered_at))
        .first(conn)?;
    if at.is_some() || !is_mastered(passed == 1, mastery, &lesson_type, support) {
        return Ok(false);
    }
    diesel::update(lessons::table.find(lesson_id)).set(lessons::mastered_at.eq(crate::db::now_secs())).execute(conn)?;
    crate::masterylog::write(
        conn,
        &crate::masterylog::NewEntry {
            lesson_id,
            before: mastery,
            after: mastery,
            grade: "",
            days_since: None,
            go,
            reason: "mastered",
            details: crate::masterylog::Details { v: crate::masterylog::DETAILS_VERSION, ..Default::default() },
        },
    )?;
    Ok(true)
}

pub use crate::masterylog::Go;

/// What a go would do to a lesson's mastery points, by the rules, without
/// writing anything: the points after it, the ledger's reason and the
/// numbers behind it. [`credit_mastery`] writes what this decides, and the
/// summary's [`impact`] asks it what a better go would have done.
struct Planned {
    after: i64,
    reason: &'static str,
    details: crate::masterylog::Details,
}

fn plan_mastery(conn: &mut Conn, before: &Lesson, grade: Grade, score: Option<(i64, i64, i64)>, today: i64) -> Result<Planned> {
    let elapsed = before.last_day.map_or(0, |last| (today - last).max(0));
    let mut details = crate::masterylog::Details { v: crate::masterylog::DETAILS_VERSION, ..Default::default() };
    if before.mastered {
        return Ok(Planned { after: before.mastery, reason: "after_mastered", details });
    }
    let passing = matches!(grade, Grade::Ace | Grade::Pass);
    let first_pass = passing && !before.passed;
    if first_pass {
        details.first_pass_day = Some(today);
    }
    // A go short of a pass earns a share of what a pass would here (the
    // first pass's 30 for a lesson never passed); none takes points away.
    // Spec: MAST-5, MAST-6
    let pass = mastery_gain(Grade::Pass, before.passed, elapsed);
    let (gain, mut reason) = match grade {
        Grade::Fail => (credit(pass, grade, score), "fail"),
        Grade::Partial => (credit(pass, grade, score), "partial"),
        _ if first_pass => (mastery_gain(grade, false, elapsed), "first_pass"),
        _ if elapsed <= 0 => (mastery_gain(grade, true, elapsed), "pass_same_day"),
        _ => (mastery_gain(grade, true, elapsed), "pass_spaced"),
    };
    details.gain = Some(gain);
    let mut mastery = (before.mastery + gain).clamp(0, MASTERY_FULL);
    // Held until passes on two different days (see MASTERY_HELD_AT).
    let held = if !before.passed {
        true
    } else {
        match crate::masterylog::first_pass_day(conn, before.id)? {
            None => false,
            Some(day) if day >= today => true,
            Some(day) => !passing && !crate::masterylog::passed_after(conn, before.id, day)?,
        }
    };
    if held && mastery > MASTERY_HELD_AT {
        mastery = MASTERY_HELD_AT.max(before.mastery);
        details.cap = Some(MASTERY_HELD_AT);
        reason = "capped";
    }
    if gain > 0 && before.mastery >= MASTERY_FULL {
        reason = "full";
    }
    Ok(Planned { after: mastery, reason, details })
}

/// Adds a go's mastery points to a lesson, as it stood before the go, and
/// writes why in the mastery ledger (one row, every graded go: a go that
/// changes nothing says so). Every go counts, extra practice too, and none
/// takes points away. Points stay within 0–100, and at
/// [`MASTERY_HELD_AT`] until the lesson has passes on two different days.
// Spec: MAST-6, MAST-10
pub fn credit_mastery(conn: &mut Conn, before: &Lesson, result: &str, ace: bool, today: i64, go: &Go) -> Result<crate::masterylog::MasteryEntry> {
    let grade = Grade::of(result, ace);
    let planned = plan_mastery(conn, before, grade, go.score, today)?;
    let entry = crate::masterylog::NewEntry {
        lesson_id: before.id,
        before: before.mastery,
        after: planned.after,
        grade: grade.key(),
        days_since: before.last_day.map(|last| (today - last).max(0)),
        go,
        reason: planned.reason,
        details: planned.details,
    };
    if before.mastered {
        return crate::masterylog::write(conn, &entry);
    }
    diesel::update(lessons::table.find(before.id)).set(lessons::mastery.eq(planned.after)).execute(conn)?;
    let written = crate::masterylog::write(conn, &entry)?;
    stamp_mastered_for(conn, before.id, go)?;
    Ok(written)
}

/// What a go's misses cost, for the summary: the next grade up (a pass, or
/// from a pass an ace), how many more right answers it would have taken,
/// and what that go would have done to the mastery points — by the same
/// rules, from the lesson as it stood before the go.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Impact {
    /// "pass" | "ace"
    pub grade: String,
    /// A drill: how many more right answers it would have taken. None for
    /// a conversation, which has no score.
    pub more_right: Option<i64>,
    /// The mastery change that go would have made, and its ledger reason.
    pub delta: i64,
    pub after: i64,
    pub reason: String,
}

/// The fewest more right answers that would have passed a drill — misses
/// turned right first (a whole point each), then almosts — and the score
/// that makes; None when it passed already or nothing was scored.
// Spec: SUM-8
pub fn more_right_to_pass(correct: i64, almost: i64, total: i64) -> Option<(i64, i64, i64)> {
    if total <= 0 || result_from_score(correct, almost, total) == "pass" {
        return None;
    }
    let wrong = (total - correct - almost).max(0);
    (1..=total - correct).map(|k| (k, correct + k, almost - (k - wrong).max(0))).find(|&(_, c, a)| result_from_score(c, a, total) == "pass")
}

/// The impact of a go's misses (see [`Impact`]): `score` is a drill's
/// (right, almost, scored), None for a conversation. Call it before the go
/// is recorded. None when there is no grade above it to miss: an ace, a
/// conversation passed, nothing scored.
// Spec: SUM-8
pub fn impact(conn: &mut Conn, before: &Lesson, result: &str, ace: bool, score: Option<(i64, i64, i64)>, today: i64) -> Result<Option<Impact>> {
    let (grade, more_right) = match score {
        Some((_, _, total)) if total <= 0 => return Ok(None),
        Some((correct, _, total)) if result == "pass" => {
            if ace {
                return Ok(None);
            }
            (Grade::Ace, Some(total - correct))
        }
        Some((correct, almost, total)) => match more_right_to_pass(correct, almost, total) {
            Some((k, c, a)) => (Grade::of("pass", a == 0 && c >= total), Some(k)),
            None => return Ok(None),
        },
        None if result == "pass" => return Ok(None),
        None => (Grade::Pass, None),
    };
    // The better grade passes: its score earns what a pass does.
    let planned = plan_mastery(conn, before, grade, None, today)?;
    Ok(Some(Impact {
        grade: grade.key().into(),
        more_right,
        delta: planned.after - before.mastery,
        after: planned.after,
        reason: planned.reason.into(),
    }))
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
pub fn record(conn: &mut Conn, lesson_id: LessonId, result: &str) -> Result<Lesson> {
    record_graded(conn, lesson_id, result, false)
}

/// Records a go at a lesson with no more said about it (tests).
#[cfg(test)]
pub fn record_graded(conn: &mut Conn, lesson_id: LessonId, result: &str, ace: bool) -> Result<Lesson> {
    Ok(record_go(conn, lesson_id, result, ace, &Go { via: "drill", ..Default::default() })?.0)
}

/// Records a go at a lesson — mastery (and its ledger row), schedule,
/// passed, minutes — all or nothing (one transaction; inside another, a
/// savepoint).
pub fn record_go(conn: &mut Conn, lesson_id: LessonId, result: &str, ace: bool, go: &Go) -> Result<(Lesson, crate::masterylog::MasteryEntry)> {
    conn.transaction(|tx| record_graded_steps(tx, lesson_id, result, ace, go))
}

fn record_graded_steps(conn: &mut Conn, lesson_id: LessonId, result: &str, ace: bool, go: &Go) -> Result<(Lesson, crate::masterylog::MasteryEntry)> {
    if !["pass", "partial", "fail"].contains(&result) {
        return Err(Error::msg(format!("Unknown result '{result}'.")));
    }
    let lesson = load(conn, lesson_id)?;
    let today = db::today(conn);
    let entry = credit_mastery(conn, &lesson, result, ace, today, go)?;
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
    stamp_mastered_for(conn, lesson_id, go)?;
    Ok((load(conn, lesson_id)?, entry))
}

/// Makes a lesson due today (the learner or the tutor wants it now).
pub fn due_now(conn: &mut Conn, lesson_id: LessonId) -> Result<Lesson> {
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
    pub course_id: Option<CourseId>,
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
    pub course_id: Option<CourseId>,
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
/// for, else in "Other lessons".
pub fn create_listening(
    conn: &mut Conn,
    song_id: SongId,
    complexity: Option<&str>,
    focus: Option<&str>,
    course: Option<CourseId>,
    origin: &str,
) -> Result<Lesson> {
    if let Some(id) =
        lessons::table.filter(lessons::song_id.eq(song_id)).select(lessons::id).first::<LessonId>(conn).optional()?
    {
        return load(conn, id);
    }
    let (title, song_course): (String, Option<CourseId>) = songs::table
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
    course_id: CourseId,
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

pub fn update(conn: &mut Conn, id: LessonId, changes: LessonChanges) -> Result<Lesson> {
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
pub fn delete(conn: &mut Conn, id: LessonId) -> Result<String> {
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
        let added: Option<LessonId> = diesel::insert_or_ignore_into(lessons::table)
            .values((lessons::id.eq(LessonId::derived("lesson", &slug)), NewLesson {
                slug: Some(&slug),
                lesson_type: "study",
                ord: crate::convert::count(ord),
                title: &file.title,
                summary: &file.summary,
                complexity: &file.complexity,
                content: &file.body,
                objectives: &serde_json::to_string(&file.objectives)?,
                period_days: file.period,
                origin: "starter",
                created_at: db::now_secs(),
                ..Default::default()
            }))
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
    #[allow(unused_imports)]
    use crate::ids::*;
    use tauri::State;

    use crate::db::Db;
    use crate::error::Result;

    #[tauri::command]
    pub async fn lessons_overview(db: State<'_, Db>) -> Result<super::Overview> {
        super::overview(&mut db.lock())
    }

    #[tauri::command]
    pub async fn lessons_for_course(db: State<'_, Db>, course_id: CourseId) -> Result<Vec<super::Lesson>> {
        super::for_course(&mut db.lock(), course_id)
    }

    /// A course's lessons, most important first, the mastered ones last —
    /// its page's list (see plan::order_for_course).
    #[tauri::command]
    pub async fn lessons_by_importance(db: State<'_, Db>, course_id: CourseId) -> Result<Vec<super::Lesson>> {
        let mut guard = db.lock();
        let lessons = super::for_course(&mut guard, course_id)?;
        crate::plan::order_for_course(&mut guard, lessons)
    }

    #[tauri::command]
    pub async fn lesson_detail(db: State<'_, Db>, lesson_id: LessonId) -> Result<super::LessonDetail> {
        super::detail(&mut db.lock(), lesson_id)
    }

    /// The learner changes how often a lesson checks in.
    #[tauri::command]
    pub async fn lesson_set_period(db: State<'_, Db>, lesson_id: LessonId, period_days: i64) -> Result<super::Lesson> {
        super::update(
            &mut db.lock(),
            lesson_id,
            super::LessonChanges { period_days: Some(period_days), ..Default::default() },
        )
    }

    /// The learner marks a lesson as a challenge (or back into the course).
    #[tauri::command]
    pub async fn lesson_set_challenge(db: State<'_, Db>, lesson_id: LessonId, challenge: bool) -> Result<super::Lesson> {
        super::update(&mut db.lock(), lesson_id, super::LessonChanges { challenge: Some(challenge), ..Default::default() })
    }

    /// The listening lesson for a song, made on first ask.
    #[tauri::command]
    pub async fn lesson_for_song(db: State<'_, Db>, song_id: SongId) -> Result<super::Lesson> {
        super::create_listening(&mut db.lock(), song_id, None, None, None, "you")
    }

    #[tauri::command]
    pub async fn lesson_delete(db: State<'_, Db>, lesson_id: LessonId) -> Result<()> {
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
        // A conversation's partial: half a pass in the same situation,
        // rounded down; a fail nothing, never less.
        assert_eq!(mastery_gain(Partial, true, 14), 17);
        assert_eq!(mastery_gain(Partial, true, 0), 5);
        assert_eq!(mastery_gain(Partial, true, 1), 8);
        assert_eq!(mastery_gain(Partial, false, 0), 15);
        for gap in 0..30 {
            for passed in [false, true] {
                assert!(mastery_gain(Partial, passed, gap) < mastery_gain(Pass, passed, gap), "gap {gap}, passed {passed}");
                assert_eq!(mastery_gain(Fail, passed, gap), 0);
            }
        }
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

    // Spec: MAST-5, MAST-6
    #[test]
    fn a_drill_short_of_a_pass_earns_in_proportion_to_its_score() {
        use Grade::*;
        // Out of 20 cards, a pass two weeks on would earn 35.
        let earn = |right: i64, almost: i64| credit(35, Grade::of(result_from_score(right, almost, 20), false), Some((right, almost, 20)));
        // Nothing at or below half right, and never less than nothing.
        assert_eq!(earn(0, 0), 0);
        assert_eq!(earn(6, 0), 0);
        assert_eq!(earn(10, 0), 0);
        // Rising linearly from half to the pass share (85%): 60% earns 10/35
        // of a pass, 75% earns 25/35; an almost counts half.
        assert_eq!(earn(12, 0), 10);
        assert_eq!(earn(15, 0), 25);
        assert_eq!(earn(14, 2), 25);
        // At the pass share it is a pass: the whole of it.
        assert_eq!(earn(17, 0), 35);
        // Every right answer counts: no cliff at the partial's line.
        let curve: Vec<i64> = (0..=17).map(|right| earn(right, 0)).collect();
        assert!(curve.windows(2).all(|w| w[0] <= w[1]), "{curve:?}");
        assert!(curve.windows(2).all(|w| w[1] - w[0] <= 5), "{curve:?}");
        // A conversation has no score: a partial earns half, a fail nothing.
        assert_eq!(credit(35, Partial, None), 17);
        assert_eq!(credit(35, Fail, None), 0);
        assert_eq!(credit(35, Fail, Some((0, 0, 0))), 0);
        assert_eq!(credit(30, Pass, Some((5, 0, 6))), 30, "a small drill passed with one miss");
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
        assert_eq!(result_from_score(9, 0, 10), "pass");
        assert_eq!(result_from_score(8, 1, 10), "pass", "85% with an almost as half");
        assert_eq!(result_from_score(8, 0, 10), "partial");
        assert_eq!(result_from_score(7, 0, 10), "partial");
        assert_eq!(result_from_score(3, 0, 10), "fail");
        // A small drill absorbs one miss (or two almosts)...
        assert_eq!(result_from_score(5, 0, 6), "pass");
        assert_eq!(result_from_score(4, 2, 6), "pass");
        assert_eq!(result_from_score(3, 0, 4), "pass");
        assert_eq!(result_from_score(2, 0, 3), "pass");
        // ...but not two.
        assert_eq!(result_from_score(4, 0, 6), "partial");
        assert_eq!(result_from_score(4, 1, 6), "partial");
        assert_eq!(result_from_score(3, 1, 6), "fail");
        assert_eq!(result_from_score(2, 0, 4), "fail");
        // Two cards or one: all right, or an almost at most partly.
        assert_eq!(result_from_score(1, 0, 2), "fail");
        assert_eq!(result_from_score(1, 1, 2), "partial");
        // Seven and up: 85% as before (6 of 7 is 86%).
        assert_eq!(result_from_score(6, 0, 7), "pass");
        assert_eq!(result_from_score(11, 0, 13), "partial");
        // Nothing scored is never a free pass; more right than asked is capped.
        assert_eq!(result_from_score(0, 0, 0), "fail");
        assert_eq!(result_from_score(12, 3, 10), "pass");
    }

    fn lesson(passed: bool, mastery: i64, reps: i64, interval: i64, period: i64) -> Lesson {
        Lesson {
            id: LessonId::new(),
            lesson_type: "words".into(),
            engine: "exercises",
            title: String::new(),
            summary: String::new(),
            complexity: "low".into(),
            course_id: None,
            course_title: None,
            course_ids: vec![],
            origin: "you".into(),
            period_days: period,
            interval_days: interval,
            due_day: None,
            last_day: None,
            last_result: None,
            reps,
            ease: 2.5,
            support_level: 0,
            challenge: false,
            passed,
            mastery,
            mastered: false,
            level: None,
            items: 0,
            song_id: None,
            passes_left: PassesLeft::default(),
        }
    }

    #[test]
    fn passes_left_follow_the_real_schedule() {
        // New, checked in weekly: 30 + 25 + 25 + 25 (the gap is the period, then grows).
        assert_eq!(passes_left(&lesson(false, 0, 0, 0, 7), false), PassesLeft { spaced: 4, now: 8 });
        // New, daily: gaps 1, 3, 8, 20 — 30, 16, 19, 26, 35 is five passes. Now:
        // 30 and six tens reach the 90 it is held at, then one on another day.
        assert_eq!(passes_left(&lesson(false, 0, 0, 0, 1), false).spaced, 5);
        // Passed on an earlier day: one sitting can finish it.
        assert_eq!(passes_left(&lesson(true, 70, 2, 7, 7), false), PassesLeft { spaced: 2, now: 3 });
        // First passed today: up to 90 today, the rest another day.
        assert_eq!(passes_left(&lesson(true, 70, 1, 7, 7), true), PassesLeft { spaced: 2, now: 3 });
        assert_eq!(passes_left(&lesson(true, 90, 1, 7, 7), true), PassesLeft { spaced: 1, now: 1 });
        let mut writing = lesson(true, 100, 3, 7, 7);
        writing.lesson_type = "writing".into();
        writing.support_level = 1;
        assert_eq!(passes_left(&writing, false), PassesLeft { spaced: 2, now: 2 }, "a pass per help level left");
        let mut done = lesson(true, 100, 3, 7, 7);
        done.mastered = true;
        assert_eq!(passes_left(&done, false), PassesLeft::default());
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
