//! Blocks: the unit of learning.
//!
//! A block is small and has one job. A **drill** is mechanical practice on a
//! handful of words or sentences (flashcards, memory cards, gaps, a
//! dialogue). A **study** block is a short lesson — a conjugation, a grammar
//! point, a bit of culture — checked by an examiner agent that interviews the
//! learner.
//!
//! Every block has a **complexity** (low ≈ 1–2 minutes, medium ≈ 5–10, high
//! ≈ 20+), set by how hard its subject is, and its own **check-in period**.
//! Passing a check-in pushes the next one further out; struggling brings it
//! back sooner.

use std::collections::HashMap;

use diesel::dsl::count_star;
use diesel::helper_types::{Asc, Desc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::db::{self, Conn};
use crate::schema::{blocks, checkins, course_blocks, sentences, songs, topics, words};
use crate::text::clean;
use crate::error::{Error, Result};

/// Words per word drill when a course's words are cut into drills: enough to
/// be worth it, small enough to finish in a minute or two.
pub const WORDS_PER_BLOCK: usize = 8;
pub const SENTENCES_PER_BLOCK: usize = 5;
/// Longest gap between check-ins, however often a block was passed.
const MAX_INTERVAL: i64 = 120;

pub const FORMATS: &[&str] = &["words", "memory", "typing", "sentences", "gaps", "dialogue"];

/// What a listening block checks, by complexity: understanding first, then
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

/// A sensible check-in period for a new block, in days.
pub fn default_period(kind: &str, complexity: &str) -> i64 {
    match (kind, complexity) {
        ("drill", "low") => 2,
        ("drill", "medium") => 4,
        ("drill", _) => 7,
        (_, "low") => 5,
        (_, "medium") => 7,
        _ => 10,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: i64,
    /// "drill" | "study"
    pub kind: String,
    /// For drills: words | memory | typing | sentences | gaps | dialogue.
    pub format: String,
    pub title: String,
    pub summary: String,
    /// "low" | "medium" | "high"
    pub complexity: String,
    /// The first course it was listed in, for display. None only for a block
    /// that is about to go.
    pub topic_id: Option<i64>,
    /// That course's title.
    pub topic_title: Option<String>,
    /// Every course that lists it.
    pub course_ids: Vec<i64>,
    /// Who made it: "starter" (came with the app) | "tutor" | "you".
    /// Informational only — every block can be edited and deleted.
    pub origin: String,
    pub period_days: i64,
    pub interval_days: i64,
    /// None until the block has been done once.
    pub due_day: Option<i64>,
    pub last_day: Option<i64>,
    /// "pass" | "partial" | "fail"
    pub last_result: Option<String>,
    pub reps: i64,
    pub items: i64,
    /// A listening block's song.
    pub song_id: Option<i64>,
}

/// A block with everything its page shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockDetail {
    #[serde(flatten)]
    pub block: Block,
    /// Study blocks: the lesson, in markdown.
    pub lesson: String,
    pub objectives: Vec<String>,
    /// Drills: what is practised, Polish and English.
    pub words: Vec<(String, String)>,
    pub sentences: Vec<(String, String)>,
    /// The last check-in's feedback, if any.
    pub last_feedback: Option<String>,
    /// The courses that list it: (id, English title, Polish title).
    pub courses: Vec<(i64, String, String)>,
    /// A conversation drill's dialogue.
    pub lines: Vec<crate::models::DialogueLine>,
}

/// What the home screen shows: what is due, and what to start next.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub today: i64,
    pub due: Vec<Block>,
    pub next: Vec<Block>,
    /// Blocks done today.
    pub done_today: i64,
}

/// What a `blocks` row stores about a block itself.
#[derive(Queryable, Selectable)]
#[diesel(table_name = blocks, check_for_backend(diesel::sqlite::Sqlite))]
struct BlockRow {
    id: i64,
    kind: String,
    format: String,
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
    song_id: Option<i64>,
}

/// A new `blocks` row.
#[derive(Insertable, Default)]
#[diesel(table_name = blocks)]
struct NewBlock<'a> {
    slug: Option<&'a str>,
    kind: &'a str,
    format: &'a str,
    ord: i64,
    title: &'a str,
    summary: &'a str,
    complexity: &'a str,
    lesson: &'a str,
    objectives: &'a str,
    period_days: i64,
    origin: &'a str,
    created_at: i64,
    song_id: Option<i64>,
}

fn insert(conn: &mut Conn, block: NewBlock) -> Result<i64> {
    Ok(diesel::insert_into(blocks::table).values(&block).returning(blocks::id).get_result(conn)?)
}

/// Completes block rows with what lives elsewhere: the courses that list
/// them (the first one also by title) and how many items they own.
fn assemble(conn: &mut Conn, rows: Vec<BlockRow>) -> Result<Vec<Block>> {
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let mut listed: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
    for (block, course, title) in course_blocks::table
        .inner_join(topics::table)
        .filter(course_blocks::block_id.eq_any(&ids))
        .order(course_blocks::rowid)
        .select((course_blocks::block_id, topics::id, topics::title_en))
        .load::<(i64, i64, String)>(conn)?
    {
        listed.entry(block).or_default().push((course, title));
    }
    let mut items: HashMap<i64, i64> = HashMap::new();
    for (block, n) in words::table
        .filter(words::block_id.eq_any(&ids))
        .group_by(words::block_id)
        .select((words::block_id, count_star()))
        .load::<(i64, i64)>(conn)?
        .into_iter()
        .chain(
            sentences::table
                .filter(sentences::block_id.eq_any(&ids))
                .group_by(sentences::block_id)
                .select((sentences::block_id, count_star()))
                .load::<(i64, i64)>(conn)?,
        )
    {
        *items.entry(block).or_default() += n;
    }
    Ok(rows
        .into_iter()
        .map(|row| {
            let courses = listed.remove(&row.id).unwrap_or_default();
            Block {
                topic_id: courses.first().map(|c| c.0),
                topic_title: courses.first().map(|c| c.1.clone()),
                course_ids: courses.iter().map(|c| c.0).collect(),
                items: items.get(&row.id).copied().unwrap_or(0),
                id: row.id,
                kind: row.kind,
                format: row.format,
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
                song_id: row.song_id,
            }
        })
        .collect())
}

pub fn load(conn: &mut Conn, id: i64) -> Result<Block> {
    let row = blocks::table
        .find(id)
        .select(BlockRow::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg(format!("There is no block with id {id}.")))?;
    Ok(assemble(conn, vec![row])?.remove(0))
}

pub fn detail(conn: &mut Conn, id: i64) -> Result<BlockDetail> {
    let block = load(conn, id)?;
    let (lesson, objectives): (String, String) =
        blocks::table.find(id).select((blocks::lesson, blocks::objectives)).first(conn)?;
    let words = words::table
        .filter(words::block_id.eq(id))
        .order((words::ord, words::id))
        .select((words::pl, words::en))
        .load(conn)?;
    let sentences = sentences::table
        .filter(sentences::block_id.eq(id))
        .order((sentences::ord, sentences::id))
        .select((sentences::pl, sentences::en))
        .load(conn)?;
    let courses = course_blocks::table
        .inner_join(topics::table)
        .filter(course_blocks::block_id.eq(id))
        .order(course_blocks::rowid)
        .select((topics::id, topics::title_en, topics::title_pl))
        .load(conn)?;
    let last_feedback = checkins::table
        .filter(checkins::block_id.eq(id))
        .filter(checkins::finished_at.is_not_null())
        .order(checkins::id.desc())
        .select(checkins::feedback)
        .first::<Option<String>>(conn)
        .optional()?
        .flatten();
    Ok(BlockDetail {
        words,
        sentences,
        lines: crate::session::block_lines(conn, id)?,
        courses,
        lesson,
        objectives: serde_json::from_str(&objectives).unwrap_or_default(),
        last_feedback,
        block,
    })
}

/// How courses are listed everywhere: the newest first (the starter courses
/// are dated 0), then in their course order.
pub fn course_order() -> (Desc<topics::created_at>, Asc<topics::ord>, Asc<topics::id>) {
    (topics::created_at.desc(), topics::ord.asc(), topics::id.asc())
}

/// A course's blocks, in the course's order.
pub fn for_topic(conn: &mut Conn, topic_id: i64) -> Result<Vec<Block>> {
    let rows = course_blocks::table
        .inner_join(blocks::table)
        .filter(course_blocks::topic_id.eq(topic_id))
        .order((course_blocks::ord, blocks::id))
        .select(BlockRow::as_select())
        .load(conn)?;
    assemble(conn, rows)
}

/// The home screen: blocks due now, then a few to start.
pub fn overview(conn: &mut Conn) -> Result<Overview> {
    let today = db::today(conn);
    let rows = blocks::table.filter(blocks::due_day.le(today)).select(BlockRow::as_select()).load(conn)?;
    let mut due = assemble(conn, rows)?;
    let weight = |c: &str| match c {
        "low" => 0,
        "medium" => 1,
        _ => 2,
    };
    due.sort_by_key(|b| (b.due_day, weight(&b.complexity), b.id));

    // The next couple of blocks from each of the first few courses with
    // something left to start, in the order the course list shows them: the
    // newest courses first (the tutor just made them for a reason).
    let mut next: Vec<Block> = Vec::new();
    let mut courses_used = 0;
    for course in topics::table.order(course_order()).select(topics::id).load::<i64>(conn)? {
        if courses_used == 3 {
            break;
        }
        let rows = course_blocks::table
            .inner_join(blocks::table)
            .filter(course_blocks::topic_id.eq(course))
            .filter(blocks::due_day.is_null())
            .order((course_blocks::ord, blocks::id))
            .limit(2)
            .select(BlockRow::as_select())
            .load(conn)?;
        if rows.is_empty() {
            continue;
        }
        courses_used += 1;
        for block in assemble(conn, rows)? {
            // A block in two courses is offered once.
            if !next.iter().any(|b| b.id == block.id) {
                next.push(block);
            }
        }
    }

    let done_today = blocks::table.filter(blocks::last_day.eq(today)).count().get_result(conn)?;
    Ok(Overview { today, due, next, done_today })
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

/// The next interval after a check-in. Passing doubles it (never below the
/// block's period); a partial pass repeats the period; failing means tomorrow.
pub fn next_interval(result: &str, reps_before: i64, interval: i64, period: i64) -> i64 {
    match result {
        "pass" if reps_before == 0 => period,
        "pass" => (interval * 2).max(period).min(MAX_INTERVAL),
        "partial" => period.max(1),
        _ => 1,
    }
}

/// Records a check-in result and schedules the next one.
pub fn record(conn: &mut Conn, block_id: i64, result: &str) -> Result<Block> {
    if !["pass", "partial", "fail"].contains(&result) {
        return Err(Error::msg(format!("Unknown result '{result}'.")));
    }
    let block = load(conn, block_id)?;
    let today = db::today(conn);
    let interval = next_interval(result, block.reps, block.interval_days, block.period_days);
    diesel::update(blocks::table.find(block_id))
        .set((
            blocks::interval_days.eq(interval),
            blocks::due_day.eq(today + interval),
            blocks::last_day.eq(today),
            blocks::last_result.eq(result),
            blocks::reps.eq(blocks::reps + 1),
        ))
        .execute(conn)?;
    load(conn, block_id)
}

/// Makes a block due today (the learner or the tutor wants it now).
pub fn due_now(conn: &mut Conn, block_id: i64) -> Result<Block> {
    let today = db::today(conn);
    diesel::update(blocks::table.find(block_id)).set(blocks::due_day.eq(today)).execute(conn)?;
    load(conn, block_id)
}

// ---------------------------------------------------------------------------
// Making and changing blocks
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
    /// The lesson, in markdown.
    pub lesson: String,
    #[serde(default)]
    pub objectives: Vec<String>,
    #[serde(default)]
    pub period_days: Option<i64>,
    /// The course to file it under; None files it in the "Lessons" course.
    #[serde(default, alias = "course_id")]
    pub topic_id: Option<i64>,
}

pub fn create_study(conn: &mut Conn, input: NewStudy, origin: &str) -> Result<Block> {
    let title = clean(&input.title);
    if title.is_empty() || input.lesson.trim().is_empty() {
        return Err(Error::msg("A study block needs a title and a lesson."));
    }
    check_complexity(&input.complexity)?;
    let objectives: Vec<String> =
        input.objectives.iter().map(|o| clean(o)).filter(|o| !o.is_empty()).collect();
    if objectives.is_empty() {
        return Err(Error::msg("A study block needs objectives: what the learner should be able to do."));
    }
    let period = input
        .period_days
        .unwrap_or_else(|| default_period("study", &input.complexity))
        .clamp(1, 60);
    let topic_id = match input.topic_id {
        Some(id) => {
            crate::courses::require_course(conn, id)?;
            id
        }
        None => crate::courses::lessons_course(conn)?,
    };
    let id = insert(
        conn,
        NewBlock {
            kind: "study",
            title: &title,
            summary: &clean(&input.summary),
            complexity: &input.complexity,
            lesson: input.lesson.trim(),
            objectives: &serde_json::to_string(&objectives)?,
            period_days: period,
            origin,
            created_at: db::now_secs(),
            ..Default::default()
        },
    )?;
    crate::courses::add_block(conn, topic_id, id)?;
    load(conn, id)
}

/// A listening block: listen to a song, then the examiner asks about it.
/// One per song — asking again returns the existing block.
/// It is filed in `course` if given, else in the course the song was written
/// from, else in "Listening". Like any block, it can be moved or shared later.
pub fn create_listening(
    conn: &mut Conn,
    song_id: i64,
    complexity: Option<&str>,
    focus: Option<&str>,
    course: Option<i64>,
    origin: &str,
) -> Result<Block> {
    if let Some(id) =
        blocks::table.filter(blocks::song_id.eq(song_id)).select(blocks::id).first::<i64>(conn).optional()?
    {
        return load(conn, id);
    }
    let (title, song_course): (String, Option<i64>) = songs::table
        .find(song_id)
        .select((songs::title, songs::topic_id))
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg("That song no longer exists."))?;
    let complexity = complexity.unwrap_or("medium");
    check_complexity(complexity)?;
    // The "lesson" is only a note: the lyrics stay hidden until asked for,
    // since the point is to understand them by ear.
    let lesson = match focus.map(clean).filter(|f| !f.is_empty()) {
        Some(focus) => format!("Listen to the song a couple of times. Pay attention to: {focus}"),
        None => "Listen to the song a couple of times, then check in. Try it without the lyrics first.".into(),
    };
    let topic_id = match course.or(song_course) {
        Some(id) => {
            crate::courses::require_course(conn, id)?;
            id
        }
        None => crate::courses::listening_course(conn)?,
    };
    let id = insert(
        conn,
        NewBlock {
            kind: "study",
            format: "song",
            title: &format!("Listen: {title}"),
            summary: "Listen to the song, then answer questions about what you heard.",
            complexity,
            lesson: &lesson,
            objectives: &serde_json::to_string(&listening_objectives(complexity))?,
            period_days: default_period("study", complexity),
            origin,
            created_at: db::now_secs(),
            song_id: Some(song_id),
            ..Default::default()
        },
    )?;
    crate::courses::add_block(conn, topic_id, id)?;
    load(conn, id)
}

/// A drill that owns the words, sentences or dialogue lines it practises,
/// listed in a course. Items it already holds (by their Polish) are skipped;
/// the count of skipped items is returned with the block.
#[allow(clippy::too_many_arguments)]
pub fn create_drill(
    conn: &mut Conn,
    topic_id: i64,
    title: &str,
    summary: &str,
    format: &str,
    complexity: &str,
    items: &crate::courses::Items,
    period_days: Option<i64>,
    origin: &str,
) -> Result<(Block, i64)> {
    if !FORMATS.contains(&format) {
        return Err(Error::msg(format!("Unknown drill format '{format}'.")));
    }
    check_complexity(complexity)?;
    crate::courses::require_course(conn, topic_id)?;
    let period = period_days.unwrap_or_else(|| default_period("drill", complexity)).clamp(1, 60);
    let id = insert(
        conn,
        NewBlock {
            kind: "drill",
            format,
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
    crate::courses::add_block(conn, topic_id, id)?;
    Ok((load(conn, id)?, skipped))
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BlockChanges {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub complexity: Option<String>,
    #[serde(default)]
    pub lesson: Option<String>,
    #[serde(default)]
    pub objectives: Option<Vec<String>>,
    #[serde(default)]
    pub period_days: Option<i64>,
}

/// The columns an edit may change; `None` leaves one as it is.
#[derive(AsChangeset, Default, PartialEq)]
#[diesel(table_name = blocks)]
struct BlockEdit {
    period_days: Option<i64>,
    title: Option<String>,
    summary: Option<String>,
    complexity: Option<String>,
    lesson: Option<String>,
    objectives: Option<String>,
}

pub fn update(conn: &mut Conn, id: i64, changes: BlockChanges) -> Result<Block> {
    load(conn, id)?;
    if let Some(complexity) = &changes.complexity {
        check_complexity(complexity)?;
    }
    let edit = BlockEdit {
        period_days: changes.period_days.map(|p| p.clamp(1, 60)),
        title: changes.title.map(|t| clean(&t)).filter(|t| !t.is_empty()),
        summary: changes.summary.map(|s| clean(&s)),
        complexity: changes.complexity,
        lesson: changes.lesson.map(|l| l.trim().to_string()).filter(|l| !l.is_empty()),
        objectives: changes
            .objectives
            .map(|list| list.iter().map(|o| clean(o)).filter(|o| !o.is_empty()).collect::<Vec<_>>())
            .map(|list| serde_json::to_string(&list))
            .transpose()?,
    };
    if edit != BlockEdit::default() {
        diesel::update(blocks::table.find(id)).set(&edit).execute(conn)?;
    }
    load(conn, id)
}

/// Deletes a block everywhere, with its words, sentences, dialogue and
/// check-in history.
pub fn delete(conn: &mut Conn, id: i64) -> Result<String> {
    let block = load(conn, id)?;
    diesel::delete(blocks::table.find(id)).execute(conn)?;
    Ok(block.title)
}

// ---------------------------------------------------------------------------
// Starter lessons
// ---------------------------------------------------------------------------

pub struct LessonFile {
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub complexity: String,
    pub period: i64,
    pub objectives: Vec<String>,
    pub body: String,
}

/// Reads a lesson: a `---` front matter block of `key: value` lines (and a
/// `- ` list under `objectives:`), then the markdown lesson.
pub fn parse_lesson(text: &str) -> Result<LessonFile> {
    let rest = text
        .strip_prefix("---\n")
        .ok_or_else(|| Error::msg("lesson without front matter"))?;
    let (head, body) = rest
        .split_once("\n---\n")
        .ok_or_else(|| Error::msg("lesson front matter is not closed"))?;
    let mut lesson = LessonFile {
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
            lesson.objectives.push(item.trim().to_string());
            continue;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim().to_string();
        match key.trim() {
            "slug" => lesson.slug = value,
            "title" => lesson.title = value,
            "summary" => lesson.summary = value,
            "complexity" => lesson.complexity = value,
            "period" => lesson.period = value.parse().unwrap_or(7),
            _ => {}
        }
    }
    if lesson.slug.is_empty() || lesson.title.is_empty() || lesson.objectives.is_empty() {
        return Err(Error::msg(format!("lesson '{}' is missing slug, title or objectives", lesson.slug)));
    }
    if !COMPLEXITIES.contains(&lesson.complexity.as_str()) {
        return Err(Error::msg(format!("lesson '{}' has complexity '{}'", lesson.slug, lesson.complexity)));
    }
    Ok(lesson)
}

/// Writes the starter lessons, on a fresh install (filed in a course by
/// `db::unify_courses`).
pub fn seed_lessons(conn: &mut Conn) -> Result<()> {
    for (ord, text) in crate::content::LESSONS.iter().enumerate() {
        let lesson = parse_lesson(text)?;
        let slug = format!("lesson/{}", lesson.slug);
        // Written once: a lesson already there (by its slug) is left alone.
        let added: Option<i64> = diesel::insert_or_ignore_into(blocks::table)
            .values(NewBlock {
                slug: Some(&slug),
                kind: "study",
                ord: ord as i64,
                title: &lesson.title,
                summary: &lesson.summary,
                complexity: &lesson.complexity,
                lesson: &lesson.body,
                objectives: &serde_json::to_string(&lesson.objectives)?,
                period_days: lesson.period,
                origin: "starter",
                created_at: db::now_secs(),
                ..Default::default()
            })
            .returning(blocks::id)
            .get_result(conn)
            .optional()?;
        if let Some(block) = added {
            let grammar = crate::courses::grammar_course(conn)?;
            crate::courses::add_block(conn, grammar, block)?;
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
    pub async fn blocks_overview(db: State<'_, Db>) -> Result<super::Overview> {
        super::overview(&mut db.lock())
    }

    #[tauri::command]
    pub async fn blocks_for_topic(db: State<'_, Db>, topic_id: i64) -> Result<Vec<super::Block>> {
        super::for_topic(&mut db.lock(), topic_id)
    }

    #[tauri::command]
    pub async fn block_detail(db: State<'_, Db>, block_id: i64) -> Result<super::BlockDetail> {
        super::detail(&mut db.lock(), block_id)
    }

    /// The learner changes how often a block checks in.
    #[tauri::command]
    pub async fn block_set_period(db: State<'_, Db>, block_id: i64, period_days: i64) -> Result<super::Block> {
        super::update(
            &mut db.lock(),
            block_id,
            super::BlockChanges { period_days: Some(period_days), ..Default::default() },
        )
    }

    /// The listening block for a song, made on first ask.
    #[tauri::command]
    pub async fn block_for_song(db: State<'_, Db>, song_id: i64) -> Result<super::Block> {
        super::create_listening(&mut db.lock(), song_id, None, None, None, "you")
    }

    #[tauri::command]
    pub async fn block_delete(db: State<'_, Db>, block_id: i64) -> Result<()> {
        super::delete(&mut db.lock(), block_id).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_ins_spread_out_when_passed_and_come_back_when_failed() {
        assert_eq!(next_interval("pass", 0, 0, 3), 3, "first pass: the period");
        assert_eq!(next_interval("pass", 1, 3, 3), 6);
        assert_eq!(next_interval("pass", 5, 100, 3), MAX_INTERVAL);
        assert_eq!(next_interval("partial", 4, 24, 3), 3);
        assert_eq!(next_interval("fail", 4, 24, 3), 1);
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
            let lesson = parse_lesson(text).unwrap();
            assert!(lesson.body.len() > 200, "{} is too thin", lesson.slug);
            assert!(lesson.objectives.len() >= 2, "{}", lesson.slug);
        }
    }
}
