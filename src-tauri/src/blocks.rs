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

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db;
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

const SELECT: &str = "SELECT b.id, b.kind, b.format, b.title, b.summary, b.complexity,
        (SELECT c.topic_id FROM course_blocks c WHERE c.block_id = b.id ORDER BY c.rowid LIMIT 1),
        (SELECT t.title_en FROM course_blocks c JOIN topics t ON t.id = c.topic_id
          WHERE c.block_id = b.id ORDER BY c.rowid LIMIT 1),
        b.origin, b.period_days, b.interval_days, b.due_day, b.last_day,
        b.last_result, b.reps,
        (SELECT COUNT(*) FROM words w WHERE w.block_id = b.id)
          + (SELECT COUNT(*) FROM sentences s WHERE s.block_id = b.id),
        b.song_id,
        (SELECT group_concat(c.topic_id) FROM course_blocks c WHERE c.block_id = b.id)
     FROM blocks b";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Block> {
    Ok(Block {
        id: r.get(0)?,
        kind: r.get(1)?,
        format: r.get(2)?,
        title: r.get(3)?,
        summary: r.get(4)?,
        complexity: r.get(5)?,
        topic_id: r.get(6)?,
        topic_title: r.get(7)?,
        origin: r.get(8)?,
        period_days: r.get(9)?,
        interval_days: r.get(10)?,
        due_day: r.get(11)?,
        last_day: r.get(12)?,
        last_result: r.get(13)?,
        reps: r.get(14)?,
        items: r.get(15)?,
        song_id: r.get(16)?,
        course_ids: r
            .get::<_, Option<String>>(17)?
            .unwrap_or_default()
            .split(',')
            .filter_map(|id| id.parse().ok())
            .collect(),
    })
}

fn query(conn: &Connection, filter: &str, params: impl rusqlite::Params) -> Result<Vec<Block>> {
    let mut stmt = conn.prepare(&format!("{SELECT} {filter}"))?;
    let rows = stmt.query_map(params, row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn load(conn: &Connection, id: i64) -> Result<Block> {
    query(conn, "WHERE b.id = ?1", [id])?
        .into_iter()
        .next()
        .ok_or_else(|| Error::msg(format!("There is no block with id {id}.")))
}

pub fn detail(conn: &Connection, id: i64) -> Result<BlockDetail> {
    let block = load(conn, id)?;
    let (lesson, objectives): (String, String) = conn.query_row(
        "SELECT lesson, objectives FROM blocks WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let pairs = |table: &str| -> Result<Vec<(String, String)>> {
        let mut stmt =
            conn.prepare(&format!("SELECT pl, en FROM {table} WHERE block_id = ?1 ORDER BY ord, id"))?;
        let rows = stmt
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    };
    let courses = {
        let mut stmt = conn.prepare(
            "SELECT t.id, t.title_en, t.title_pl FROM course_blocks c JOIN topics t ON t.id = c.topic_id
             WHERE c.block_id = ?1 ORDER BY c.rowid",
        )?;
        let rows = stmt
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let last_feedback = conn
        .query_row(
            "SELECT feedback FROM checkins WHERE block_id = ?1 AND finished_at IS NOT NULL
             ORDER BY id DESC LIMIT 1",
            [id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    Ok(BlockDetail {
        words: pairs("words")?,
        sentences: pairs("sentences")?,
        courses,
        lesson,
        objectives: serde_json::from_str(&objectives).unwrap_or_default(),
        last_feedback,
        block,
    })
}

/// How courses are listed everywhere: the newest first (the starter courses
/// are dated 0), then in their course order.
pub const COURSE_ORDER: &str = "t.created_at DESC, t.ord, t.id";

/// A course's blocks, in the course's order.
pub fn for_topic(conn: &Connection, topic_id: i64) -> Result<Vec<Block>> {
    query(
        conn,
        "JOIN course_blocks cb ON cb.block_id = b.id WHERE cb.topic_id = ?1 ORDER BY cb.ord, b.id",
        [topic_id],
    )
}

/// The home screen: blocks due now, then a few to start.
pub fn overview(conn: &Connection) -> Result<Overview> {
    let today = db::today(conn);
    let due = query(
        conn,
        "WHERE b.due_day IS NOT NULL AND b.due_day <= ?1
         ORDER BY b.due_day, CASE b.complexity WHEN 'low' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END, b.id",
        [today],
    )?;

    // The next couple of blocks from each of the first few courses with
    // something left to start, in the order the course list shows them: the
    // newest courses first (the tutor just made them for a reason).
    let mut next = Vec::new();
    let courses: Vec<i64> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT t.id FROM topics t
             WHERE EXISTS (SELECT 1 FROM course_blocks cb JOIN blocks b ON b.id = cb.block_id
                           WHERE cb.topic_id = t.id AND b.due_day IS NULL)
             ORDER BY {COURSE_ORDER} LIMIT 3"
        ))?;
        let ids = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids
    };
    for course in courses {
        for block in query(
            conn,
            "JOIN course_blocks cb ON cb.block_id = b.id
             WHERE cb.topic_id = ?1 AND b.due_day IS NULL ORDER BY cb.ord, b.id LIMIT 2",
            [course],
        )? {
            // A block in two courses is offered once.
            if !next.iter().any(|b: &Block| b.id == block.id) {
                next.push(block);
            }
        }
    }

    let done_today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM blocks WHERE last_day = ?1",
        [today],
        |r| r.get(0),
    )?;
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
pub fn record(conn: &Connection, block_id: i64, result: &str) -> Result<Block> {
    if !["pass", "partial", "fail"].contains(&result) {
        return Err(Error::msg(format!("Unknown result '{result}'.")));
    }
    let block = load(conn, block_id)?;
    let today = db::today(conn);
    let interval = next_interval(result, block.reps, block.interval_days, block.period_days);
    conn.execute(
        "UPDATE blocks SET interval_days = ?2, due_day = ?3, last_day = ?4, last_result = ?5,
             reps = reps + 1
         WHERE id = ?1",
        rusqlite::params![block_id, interval, today + interval, today, result],
    )?;
    load(conn, block_id)
}

/// Makes a block due today (the learner or the tutor wants it now).
pub fn due_now(conn: &Connection, block_id: i64) -> Result<Block> {
    let today = db::today(conn);
    conn.execute("UPDATE blocks SET due_day = ?2 WHERE id = ?1", rusqlite::params![block_id, today])?;
    load(conn, block_id)
}

// ---------------------------------------------------------------------------
// Making and changing blocks
// ---------------------------------------------------------------------------

fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

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

pub fn create_study(conn: &Connection, input: NewStudy, origin: &str) -> Result<Block> {
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
    conn.execute(
        "INSERT INTO blocks (kind, format, ord, title, summary, complexity, lesson,
             objectives, period_days, origin, created_at)
         VALUES ('study', '', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            0,
            title,
            clean(&input.summary),
            input.complexity,
            input.lesson.trim(),
            serde_json::to_string(&objectives)?,
            period,
            origin,
            db::now_secs()
        ],
    )?;
    let id = conn.last_insert_rowid();
    crate::courses::add_block(conn, topic_id, id)?;
    load(conn, id)
}

/// A listening block: listen to a song, then the examiner asks about it.
/// One per song — asking again returns the existing block.
pub fn create_listening(
    conn: &Connection,
    song_id: i64,
    complexity: Option<&str>,
    focus: Option<&str>,
    origin: &str,
) -> Result<Block> {
    if let Some(id) = conn
        .query_row("SELECT id FROM blocks WHERE song_id = ?1", [song_id], |r| r.get::<_, i64>(0))
        .optional()?
    {
        return load(conn, id);
    }
    let title: String = conn
        .query_row("SELECT title FROM songs WHERE id = ?1", [song_id], |r| r.get(0))
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
    let topic_id = crate::courses::listening_course(conn)?;
    conn.execute(
        "INSERT INTO blocks (kind, format, ord, title, summary, complexity, lesson,
             objectives, period_days, origin, created_at, song_id)
         VALUES ('study', 'song', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            0,
            format!("Listen: {title}"),
            "Listen to the song, then answer questions about what you heard.",
            complexity,
            lesson,
            serde_json::to_string(&listening_objectives(complexity))?,
            default_period("study", complexity),
            origin,
            db::now_secs(),
            song_id
        ],
    )?;
    let id = conn.last_insert_rowid();
    crate::courses::add_block(conn, topic_id, id)?;
    load(conn, id)
}

/// A drill that owns the words, sentences or dialogue lines it practises,
/// listed in a course. Items it already holds (by their Polish) are skipped;
/// the count of skipped items is returned with the block.
#[allow(clippy::too_many_arguments)]
pub fn create_drill(
    conn: &Connection,
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
    conn.execute(
        "INSERT INTO blocks (kind, format, title, summary, complexity, period_days, origin, created_at)
         VALUES ('drill', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![format, clean(title), clean(summary), complexity, period, origin, db::now_secs()],
    )?;
    let id = conn.last_insert_rowid();
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

pub fn update(conn: &Connection, id: i64, changes: BlockChanges) -> Result<Block> {
    load(conn, id)?;
    if let Some(period) = changes.period_days {
        conn.execute(
            "UPDATE blocks SET period_days = ?2 WHERE id = ?1",
            rusqlite::params![id, period.clamp(1, 60)],
        )?;
    }
    if let Some(title) = changes.title.map(|t| clean(&t)).filter(|t| !t.is_empty()) {
        conn.execute("UPDATE blocks SET title = ?2 WHERE id = ?1", rusqlite::params![id, title])?;
    }
    if let Some(summary) = changes.summary {
        conn.execute("UPDATE blocks SET summary = ?2 WHERE id = ?1", rusqlite::params![id, clean(&summary)])?;
    }
    if let Some(complexity) = changes.complexity {
        check_complexity(&complexity)?;
        conn.execute("UPDATE blocks SET complexity = ?2 WHERE id = ?1", rusqlite::params![id, complexity])?;
    }
    if let Some(lesson) = changes.lesson.filter(|l| !l.trim().is_empty()) {
        conn.execute("UPDATE blocks SET lesson = ?2 WHERE id = ?1", rusqlite::params![id, lesson.trim()])?;
    }
    if let Some(objectives) = changes.objectives {
        let objectives: Vec<String> = objectives.iter().map(|o| clean(o)).filter(|o| !o.is_empty()).collect();
        conn.execute(
            "UPDATE blocks SET objectives = ?2 WHERE id = ?1",
            rusqlite::params![id, serde_json::to_string(&objectives)?],
        )?;
    }
    load(conn, id)
}

/// Deletes a block everywhere, with its words, sentences, dialogue and
/// check-in history.
pub fn delete(conn: &Connection, id: i64) -> Result<String> {
    let block = load(conn, id)?;
    conn.execute("DELETE FROM blocks WHERE id = ?1", [id])?;
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
pub fn seed_lessons(conn: &Connection) -> Result<()> {
    for (ord, text) in crate::content::LESSONS.iter().enumerate() {
        let lesson = parse_lesson(text)?;
        let slug = format!("lesson/{}", lesson.slug);
        conn.execute(
            "INSERT INTO blocks (slug, kind, format, ord, title, summary, complexity, lesson,
                 objectives, period_days, origin, created_at)
             VALUES (?1, 'study', '', ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'starter', ?9)
             ON CONFLICT(slug) DO NOTHING",
            rusqlite::params![
                slug,
                ord as i64,
                lesson.title,
                lesson.summary,
                lesson.complexity,
                lesson.body,
                serde_json::to_string(&lesson.objectives)?,
                lesson.period,
                db::now_secs()
            ],
        )?;
        if conn.changes() > 0 {
            let block = conn.last_insert_rowid();
            let grammar = crate::courses::grammar_course(conn)?;
            crate::courses::add_block(conn, grammar, block)?;
        }
    }
    Ok(())
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
