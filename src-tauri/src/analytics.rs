//! Learning analytics: what the learning log (learnlog.rs) says about what
//! the learner finds hard — words, sentences, grammar structures, sounds,
//! kinds of mistake — and how they practise. One implementation serves both
//! the Progress pages (the commands at the bottom) and the tutor's tools
//! (tutor_analytics.rs): what the tutor can see, the learner can browse.
//!
//! Everything is worked out from the rows of a period, in memory: a period
//! holds at most [`ROW_LIMIT`] answers, which aggregate in milliseconds.

use std::collections::{BTreeMap, HashMap, HashSet};

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::convert::{count, real, round};
use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
use crate::grammar::{self, TagInfo};
#[allow(unused_imports)]
use crate::ids::*;
use crate::learnlog::{read_details, EventDetails};
use crate::mistakes::{self, WordResult};
use crate::schema::{attempts, course_lessons, courses, day_totals, learning_events, lessons, reviews, sentences, word_examples, words};
use crate::srs::ReviewState;

/// The most answers a period is read with (the newest).
pub const ROW_LIMIT: i64 = 50_000;

// ---------------------------------------------------------------------------
// Filters and rows
// ---------------------------------------------------------------------------

/// Which answers: a span of days (inclusive; open ends are open) and one
/// course. Shared by every page and tool.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    #[serde(default)]
    pub from_day: Option<i64>,
    #[serde(default)]
    pub to_day: Option<i64>,
    #[serde(default)]
    pub course_id: Option<CourseId>,
}

impl Filter {
    /// A named period ending today: week, month, quarter, year or all.
    pub fn period(conn: &mut Conn, period: &str, course_id: Option<CourseId>) -> Result<Filter> {
        let today = db::today(conn);
        let days = match period {
            "week" => Some(7),
            "" | "month" => Some(30),
            "quarter" => Some(91),
            "year" => Some(365),
            "all" => None,
            other => return Err(Error::msg(format!("Unknown period '{other}': week, month, quarter, year or all."))),
        };
        Ok(Filter { from_day: days.map(|d| today - d + 1), to_day: None, course_id })
    }

    fn len(&self, today: i64) -> Option<i64> {
        self.from_day.map(|from| self.to_day.unwrap_or(today) - from + 1)
    }
}

/// One answer as analytics reads it.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = attempts)]
pub struct Row {
    pub id: AttemptId,
    pub at: i64,
    pub day: i64,
    pub item_kind: String,
    pub item_id: String,
    pub lesson_id: Option<LessonId>,
    pub exercise: String,
    pub correct: i64,
    pub given: String,
    pub expected: String,
    pub mistake: String,
    pub detail: String,
    pub contrast: String,
    pub ms_answer: Option<i64>,
    pub plays: i64,
    pub reveals: i64,
    pub details: String,
    pub course_id: Option<CourseId>,
    pub source: String,
}

impl Row {
    fn right(&self) -> bool {
        self.correct != 0
    }
}

pub(crate) fn course_lessons_of(conn: &mut Conn, course_id: CourseId) -> Result<Vec<LessonId>> {
    Ok(course_lessons::table.filter(course_lessons::course_id.eq(course_id)).select(course_lessons::lesson_id).load(conn)?)
}

/// The answers a filter takes, oldest first.
pub fn load(conn: &mut Conn, f: &Filter) -> Result<Vec<Row>> {
    let mut query = attempts::table.select(Row::as_select()).order(attempts::at.desc()).limit(ROW_LIMIT).into_boxed();
    if let Some(from) = f.from_day {
        query = query.filter(attempts::day.ge(from));
    }
    if let Some(to) = f.to_day {
        query = query.filter(attempts::day.le(to));
    }
    if let Some(course) = f.course_id {
        // The course it was practised from; answers from before that was
        // kept (schema 14) count for their lesson's courses.
        let lessons = course_lessons_of(conn, course)?;
        query = query.filter(attempts::course_id.eq(course).or(attempts::course_id.is_null().and(attempts::lesson_id.eq_any(lessons))));
    }
    let mut rows: Vec<Row> = query.load(conn)?;
    rows.reverse();
    Ok(rows)
}

/// What the ids in the log name: words, sentences, lessons, courses.
pub struct Titles {
    pub words: HashMap<String, WordInfo>,
    pub sentences: HashMap<String, (String, String, LessonId)>,
    pub lessons: HashMap<LessonId, String>,
}

#[derive(Debug, Clone)]
pub struct WordInfo {
    pub pl: String,
    pub en: String,
    pub pos: String,
    pub image: Option<FileId>,
    pub lesson: LessonId,
    pub near: (String, String),
}

impl Titles {
    pub fn load(conn: &mut Conn) -> Result<Titles> {
        let words = words::table
            .select((words::id, words::pl, words::en, words::pos, words::image_file_id, words::lesson_id, words::near_pl, words::near_en))
            .load::<(WordId, String, String, String, Option<FileId>, LessonId, String, String)>(conn)?
            .into_iter()
            .map(|(id, pl, en, pos, image, lesson, npl, nen)| (id.to_string(), WordInfo { pl, en, pos, image, lesson, near: (npl, nen) }))
            .collect();
        let sentences = sentences::table
            .select((sentences::id, sentences::pl, sentences::en, sentences::lesson_id))
            .load::<(SentenceId, String, String, LessonId)>(conn)?
            .into_iter()
            .map(|(id, pl, en, lesson)| (id.to_string(), (pl, en, lesson)))
            .collect();
        let lessons = lessons::table.select((lessons::id, lessons::title)).load::<(LessonId, String)>(conn)?.into_iter().collect();
        Ok(Titles { words, sentences, lessons })
    }

    /// The item's Polish and English, or the lesson's title for a card
    /// about a whole lesson; what was expected when it is gone.
    fn of(&self, row: &Row) -> (String, String) {
        if let Some(w) = self.words.get(&row.item_id) {
            return (w.pl.clone(), w.en.clone());
        }
        if let Some((pl, en, _)) = self.sentences.get(&row.item_id) {
            return (pl.clone(), en.clone());
        }
        let lesson = row.lesson_id.and_then(|l| self.lessons.get(&l)).cloned().unwrap_or_default();
        (row.expected.clone(), lesson)
    }
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// A class of mistake and how often.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassCount {
    pub class: String,
    pub label: String,
    pub count: i64,
}

/// A day (or a week, by its Monday) and its answers.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayPoint {
    pub day: i64,
    pub attempts: i64,
    pub correct: i64,
}

/// One answer, as a page or the tutor shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AttemptView {
    pub id: AttemptId,
    pub at: i64,
    pub day: i64,
    pub exercise: String,
    pub exercise_label: String,
    pub item_kind: String,
    pub item_id: String,
    /// The item's Polish (or what was expected, when it is gone).
    pub item: String,
    pub lesson_id: Option<LessonId>,
    pub correct: bool,
    pub given: String,
    pub expected: String,
    pub mistake: String,
    pub mistake_label: String,
    pub detail: String,
    pub contrast: String,
    pub ms_answer: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<WordResult>,
}

/// What an exercise kind is called on the pages.
pub fn exercise_label(kind: &str) -> String {
    match kind {
        "recognise" => "Pick the meaning",
        "produce" => "Pick the Polish",
        "type" => "Write it",
        "wordOrder" => "Build the sentence",
        "dialogueGap" => "What comes next",
        "gapFill" => "Fill the gaps",
        "memory" => "Memory",
        "wordPick" | "picturePick" => "Pictures",
        "dictation" => "Dictation",
        "soundPair" => "Sound pair",
        "sceneFind" => "Find it",
        "sceneName" | "sceneType" => "Name it",
        "sceneWhere" => "Where is it?",
        "storyQuestion" => "Story question",
        "sayIt" => "Say it",
        "dialogueSay" => "Say your line",
        "checkin" => "Conversation",
        "checkinMistake" => "Conversation slip",
        "game:dictation" => "Dictation game",
        "game:sounds" => "Sound pairs game",
        "game:say" => "Say it game",
        "game:match" => "Match pairs",
        other => other,
    }
    .to_string()
}

pub(crate) fn view(row: &Row, titles: &Titles, with_words: bool) -> AttemptView {
    AttemptView {
        id: row.id,
        at: row.at,
        day: row.day,
        exercise: row.exercise.clone(),
        exercise_label: exercise_label(&row.exercise),
        item_kind: row.item_kind.clone(),
        item_id: row.item_id.clone(),
        item: titles.of(row).0,
        lesson_id: row.lesson_id,
        correct: row.right(),
        given: row.given.clone(),
        expected: row.expected.clone(),
        mistake: row.mistake.clone(),
        mistake_label: mistakes::label(&row.mistake).into(),
        detail: row.detail.clone(),
        contrast: row.contrast.clone(),
        ms_answer: row.ms_answer,
        words: if with_words { read_details(&row.details).words } else { vec![] },
    }
}

/// Mistakes by class, most first. A right answer with its Polish letters
/// forgiven is not counted: it was right.
pub fn class_counts<'a>(rows: impl Iterator<Item = &'a Row>) -> Vec<ClassCount> {
    let mut counts: BTreeMap<&str, i64> = BTreeMap::new();
    for row in rows.filter(|r| !r.right() && !r.mistake.is_empty()) {
        *counts.entry(row.mistake.as_str()).or_default() += 1;
    }
    let mut out: Vec<ClassCount> =
        counts.into_iter().map(|(class, count)| ClassCount { class: class.into(), label: mistakes::label(class).into(), count }).collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.class.cmp(&b.class)));
    out
}

pub(crate) fn accuracy(correct: i64, attempts: i64) -> f64 {
    if attempts == 0 { 0.0 } else { real(correct) / real(attempts) }
}

/// How tricky something is, from its answers oldest first: the share
/// missed, the latest counting most (each older one 0.8 of the next), and
/// a little doubt for few answers (one miss in one go is not 100%).
pub fn trickiness(results: &[bool]) -> f64 {
    let (mut missed, mut weight, mut w) = (0.0, 0.0, 1.0);
    for right in results.iter().rev() {
        if !right {
            missed += w;
        }
        weight += w;
        w *= 0.8;
    }
    missed / (weight + 1.0)
}

/// How well a word is known: new, learning, solid, or tricky.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    New,
    Learning,
    Solid,
    Tricky,
}

/// From its schedule (SRS) and its latest answers, oldest first. Tricky:
/// at least a third of the last eight missed (three answers or more), the
/// last two both missed, or forgotten three times and missed lately. Solid:
/// three weeks between reviews, or the last four right after three good
/// reviews in a row. New: never answered.
pub fn strength(review: Option<&ReviewState>, results: &[bool]) -> Strength {
    let recent: Vec<bool> = results.iter().rev().take(8).copied().collect();
    let missed = recent.iter().filter(|r| !**r).count();
    if recent.is_empty() && !review.is_some_and(|r| r.reps > 0 || r.lapses > 0) {
        return Strength::New;
    }
    let lapses = review.map_or(0, |r| r.lapses);
    let last_two_missed = recent.len() >= 2 && !recent[0] && !recent[1];
    if (recent.len() >= 3 && missed * 3 >= recent.len()) || last_two_missed || (lapses >= 3 && recent.iter().take(5).any(|r| !r)) {
        return Strength::Tricky;
    }
    let interval = review.map_or(0, |r| r.interval);
    let reps = review.map_or(0, |r| r.reps);
    if interval >= 21 || (recent.len() >= 4 && recent.iter().take(4).all(|r| *r) && reps >= 3) {
        return Strength::Solid;
    }
    Strength::Learning
}

fn monday(day: i64) -> i64 {
    day - (day + 3).rem_euclid(7)
}

/// Answers per week (by Monday), oldest first.
fn weeks<'a>(rows: impl Iterator<Item = &'a Row>) -> Vec<DayPoint> {
    let mut by: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    for row in rows {
        let entry = by.entry(monday(row.day)).or_default();
        entry.0 += 1;
        entry.1 += i64::from(row.right());
    }
    by.into_iter().map(|(day, (attempts, correct))| DayPoint { day, attempts, correct }).collect()
}

/// Every item's tags (the grammar links) and every tag's items.
struct Tagged {
    of_item: HashMap<String, Vec<GrammarTagId>>,
}

impl Tagged {
    fn load(conn: &mut Conn) -> Result<Tagged> {
        Ok(Tagged { of_item: grammar::all_links(conn)? })
    }

    /// The tags an answer counts toward: its item's, and the ones an
    /// examiner gave a check-in slip.
    fn of(&self, row: &Row) -> Vec<GrammarTagId> {
        let mut tags = self.of_item.get(&row.item_id).cloned().unwrap_or_default();
        if row.exercise == "checkinMistake" {
            for key in read_details(&row.details).grammar {
                if grammar::by_key(&key).is_some() && !tags.contains(&grammar::id_of(&key)) {
                    tags.push(grammar::id_of(&key));
                }
            }
        }
        tags
    }
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

/// A structure, a sound or a kind of exercise, and how it goes.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Area {
    /// structure | sound | exercise
    pub kind: String,
    pub key: String,
    pub name: String,
    pub attempts: i64,
    pub accuracy: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub filter: Filter,
    pub attempts: i64,
    pub correct: i64,
    pub accuracy: f64,
    /// Different things answered.
    pub items: i64,
    pub days_active: i64,
    /// Minutes practised (the day log's, over the period).
    pub minutes: i64,
    pub mistakes: Vec<ClassCount>,
    /// Right answers whose Polish letters were forgiven.
    pub letters_forgiven: i64,
    pub daily: Vec<DayPoint>,
    /// The same length of time just before, for the trend (none for "all").
    pub accuracy_before: Option<f64>,
    /// improving | slipping | steady | new
    pub trend: String,
    pub strongest: Vec<Area>,
    pub weakest: Vec<Area>,
    /// Over all time, for the "What you've met" cards.
    pub words_met: i64,
    pub structures_met: i64,
    pub sounds_met: i64,
    /// Topics with something tagged (topics.rs).
    pub topics_met: i64,
    pub untagged: i64,
}

fn areas(rows: &[Row], tagged: &Tagged) -> Vec<Area> {
    let mut by: HashMap<(String, String), (i64, i64)> = HashMap::new();
    for row in rows {
        let right = i64::from(row.right());
        for tag in tagged.of(row) {
            let e = by.entry(("structure".into(), tag.to_string())).or_default();
            e.0 += 1;
            e.1 += right;
        }
        if !row.contrast.is_empty() {
            let e = by.entry(("sound".into(), row.contrast.clone())).or_default();
            e.0 += 1;
            e.1 += right;
        }
        let e = by.entry(("exercise".into(), row.exercise.clone())).or_default();
        e.0 += 1;
        e.1 += right;
    }
    by.into_iter()
        .filter(|(_, (n, _))| *n >= 3)
        .map(|((kind, key), (attempts, correct))| {
            let name = match kind.as_str() {
                "structure" => GrammarTagId::parse(&key).ok().and_then(grammar::by_id).map_or(key.clone(), |t| t.name.to_string()),
                "exercise" => exercise_label(&key),
                _ => key.clone(),
            };
            let key = if kind == "structure" { GrammarTagId::parse(&key).ok().and_then(grammar::by_id).map_or(key.clone(), |t| t.key.to_string()) } else { key };
            Area { kind, key, name, attempts, accuracy: accuracy(correct, attempts) }
        })
        .collect()
}

/// Minutes practised in a filter's days (the day log's, every device).
fn minutes_in(conn: &mut Conn, f: &Filter) -> Result<i64> {
    let mut q = day_totals::table.select(day_totals::minutes).into_boxed();
    if let Some(from) = f.from_day {
        q = q.filter(day_totals::day.ge(from));
    }
    if let Some(to) = f.to_day {
        q = q.filter(day_totals::day.le(to));
    }
    Ok(q.load::<i64>(conn)?.into_iter().sum())
}

pub fn overview(conn: &mut Conn, f: &Filter) -> Result<Overview> {
    let rows = load(conn, f)?;
    let today = db::today(conn);
    let tagged = Tagged::load(conn)?;
    let correct = count(rows.iter().filter(|r| r.right()).count());
    let attempts = count(rows.len());
    let items: HashSet<&str> = rows.iter().map(|r| r.item_id.as_str()).collect();
    let days: HashSet<i64> = rows.iter().map(|r| r.day).collect();
    let mut daily: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    for row in &rows {
        let e = daily.entry(row.day).or_default();
        e.0 += 1;
        e.1 += i64::from(row.right());
    }
    let minutes = minutes_in(conn, f)?;
    // The trend: this period against the same length just before it (the
    // last two weeks against the two before, for all time).
    let (span, end) = match f.len(today) {
        Some(len) => (len, f.from_day.unwrap_or(today) - 1),
        None => (14, today - 14),
    };
    let before = load(conn, &Filter { from_day: Some(end - span + 1), to_day: Some(end), course_id: f.course_id })?;
    let accuracy_now = if f.from_day.is_some() {
        accuracy(correct, attempts)
    } else {
        let recent: Vec<&Row> = rows.iter().filter(|r| r.day > today - 14).collect();
        accuracy(count(recent.iter().filter(|r| r.right()).count()), count(recent.len()))
    };
    let accuracy_before = (!before.is_empty()).then(|| accuracy(count(before.iter().filter(|r| r.right()).count()), count(before.len())));
    let trend = match accuracy_before {
        _ if attempts == 0 => "new",
        None => "new",
        Some(b) if accuracy_now - b >= 0.05 => "improving",
        Some(b) if b - accuracy_now >= 0.05 => "slipping",
        Some(_) => "steady",
    };
    let mut all = areas(&rows, &tagged);
    all.sort_by(|a, b| b.accuracy.total_cmp(&a.accuracy).then(b.attempts.cmp(&a.attempts)));
    let strongest: Vec<Area> = all.iter().filter(|a| a.kind != "exercise" || a.attempts >= 5).take(4).cloned().collect();
    let weakest: Vec<Area> = all.iter().rev().filter(|a| a.accuracy < 0.9).take(4).cloned().collect();
    let met = met_counts(conn)?;
    Ok(Overview {
        filter: *f,
        attempts,
        correct,
        accuracy: accuracy(correct, attempts),
        items: count(items.len()),
        days_active: count(days.len()),
        minutes,
        mistakes: class_counts(rows.iter()),
        letters_forgiven: count(rows.iter().filter(|r| r.right() && r.mistake == "diacritics").count()),
        daily: daily.into_iter().map(|(day, (attempts, correct))| DayPoint { day, attempts, correct }).collect(),
        accuracy_before,
        trend: trend.into(),
        strongest,
        weakest,
        words_met: met.0,
        structures_met: met.1,
        sounds_met: met.2,
        topics_met: count(crate::topics::all_links(conn)?.into_values().flatten().collect::<HashSet<_>>().len()),
        untagged: grammar::untagged_count(conn)?,
    })
}

/// Words met (answered or scheduled), structures with a met item, sounds
/// practised: the three cards of "What you've met".
fn met_counts(conn: &mut Conn) -> Result<(i64, i64, i64)> {
    let words = met_word_ids(conn)?;
    let mut structures: HashSet<GrammarTagId> = HashSet::new();
    let sentences: HashSet<String> = reviews::table.filter(reviews::kind.eq("sentence")).select(reviews::item_id).load::<String>(conn)?.into_iter().collect();
    for (item, tags) in grammar::all_links(conn)? {
        if words.contains(&item) || sentences.contains(&item) {
            structures.extend(tags);
        }
    }
    let sounds = attempts::table.filter(attempts::contrast.ne("")).select(attempts::contrast).distinct().load::<String>(conn)?.len();
    Ok((count(words.len()), count(structures.len()), count(sounds)))
}

fn met_word_ids(conn: &mut Conn) -> Result<HashSet<String>> {
    let mut ids: HashSet<String> = reviews::table.filter(reviews::kind.eq("word")).select(reviews::item_id).load::<String>(conn)?.into_iter().collect();
    ids.extend(attempts::table.filter(attempts::item_kind.eq("word")).select(attempts::item_id).distinct().load::<String>(conn)?);
    // Only words still there.
    let existing: HashSet<String> = words::table.select(words::id).load::<WordId>(conn)?.into_iter().map(|w| w.to_string()).collect();
    ids.retain(|id| existing.contains(id));
    Ok(ids)
}

// ---------------------------------------------------------------------------
// Hardest items
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HardItem {
    /// word | sentence | structure | sound
    pub kind: String,
    /// A word's or sentence's id, a structure's key, a sound's contrast.
    pub key: String,
    pub title: String,
    pub subtitle: String,
    pub attempts: i64,
    pub wrong: i64,
    pub accuracy: f64,
    pub score: f64,
    pub mistakes: Vec<ClassCount>,
    /// The latest misses, newest first.
    pub last: Vec<AttemptView>,
}

pub const HARD_KINDS: &[&str] = &["word", "sentence", "structure", "sound"];

pub fn hard_items(conn: &mut Conn, kind: &str, f: &Filter, limit: usize) -> Result<Vec<HardItem>> {
    if !HARD_KINDS.contains(&kind) {
        return Err(Error::msg(format!("Unknown kind '{kind}': word, sentence, structure or sound.")));
    }
    let rows = load(conn, f)?;
    let titles = Titles::load(conn)?;
    let tagged = Tagged::load(conn)?;
    let mut groups: HashMap<String, Vec<&Row>> = HashMap::new();
    for row in &rows {
        match kind {
            "word" | "sentence" if row.item_kind == kind => groups.entry(row.item_id.clone()).or_default().push(row),
            "structure" => {
                for tag in tagged.of(row) {
                    groups.entry(tag.to_string()).or_default().push(row);
                }
            }
            "sound" if !row.contrast.is_empty() => groups.entry(row.contrast.clone()).or_default().push(row),
            _ => {}
        }
    }
    let mut out: Vec<HardItem> = groups
        .into_iter()
        .filter(|(_, rows)| rows.iter().any(|r| !r.right()))
        .map(|(key, rows)| {
            let results: Vec<bool> = rows.iter().map(|r| r.right()).collect();
            let wrong = count(results.iter().filter(|r| !**r).count());
            let (key, title, subtitle) = match kind {
                "structure" => {
                    let tag = GrammarTagId::parse(&key).ok().and_then(grammar::by_id);
                    (tag.map_or(key.clone(), |t| t.key.into()), tag.map_or(key.clone(), |t| t.name.into()), tag.map_or(String::new(), |t| t.explain.into()))
                }
                "sound" => (key.clone(), key.clone(), "Heard in sound pairs and speaking".into()),
                _ => {
                    let (pl, en) = titles.of(rows[0]);
                    (key.clone(), pl, en)
                }
            };
            HardItem {
                kind: kind.into(),
                key,
                title,
                subtitle,
                attempts: count(rows.len()),
                wrong,
                accuracy: accuracy(count(rows.len()) - wrong, count(rows.len())),
                score: trickiness(&results),
                mistakes: class_counts(rows.iter().copied()).into_iter().take(3).collect(),
                last: rows.iter().rev().filter(|r| !r.right()).take(3).map(|r| view(r, &titles, false)).collect(),
            }
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.wrong.cmp(&a.wrong)).then(a.title.cmp(&b.title)));
    out.truncate(limit.clamp(1, 200));
    Ok(out)
}

// ---------------------------------------------------------------------------
// One item's history
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EventView {
    pub at: i64,
    pub kind: String,
    pub label: String,
    pub details: EventDetails,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemHistory {
    pub item_kind: String,
    pub item_id: String,
    pub pl: String,
    pub en: String,
    pub lesson_id: Option<LessonId>,
    pub strength: Strength,
    pub attempts: i64,
    pub wrong: i64,
    pub mistakes: Vec<ClassCount>,
    /// Every answer, newest first.
    pub history: Vec<AttemptView>,
    /// Everything else done with it, newest first.
    pub events: Vec<EventView>,
    pub grammar: Vec<TagInfo>,
}

pub(crate) fn event_label(kind: &str) -> String {
    match kind {
        "item_viewed" => "Seen",
        "spot_explored" => "Tapped in the picture",
        "meaning_tapped" => "Meaning looked up",
        "talk_opened" => "Talked about it",
        "audio_played" => "Listened",
        "card_skipped" => "Skipped",
        "lesson_opened" => "Lesson opened",
        "lesson_started" => "Lesson started",
        "lesson_left" => "Left early",
        "lesson_finished" => "Lesson finished",
        "game_started" => "Game started",
        "game_finished" => "Game finished",
        "checkin_started" => "Conversation started",
        "report_asked" => "Report asked for",
        other => other,
    }
    .to_string()
}

fn events_of(conn: &mut Conn, item_id: &str, limit: i64) -> Result<Vec<EventView>> {
    Ok(learning_events::table
        .filter(learning_events::item_id.eq(item_id))
        .order(learning_events::at.desc())
        .limit(limit)
        .select((learning_events::at, learning_events::kind, learning_events::details))
        .load::<(i64, String, String)>(conn)?
        .into_iter()
        .map(|(at, kind, details)| EventView { at, label: event_label(&kind), kind, details: serde_json::from_str(&details).unwrap_or_default() })
        .collect())
}

fn review_of(conn: &mut Conn, kind: &str, id: &str) -> Result<Option<ReviewState>> {
    crate::srs::load(conn, kind, id)
}

pub fn item_history(conn: &mut Conn, item_id: &str) -> Result<ItemHistory> {
    let item_id = item_id.trim();
    let titles = Titles::load(conn)?;
    let rows: Vec<Row> = attempts::table.filter(attempts::item_id.eq(item_id)).order(attempts::at.asc()).limit(2000).select(Row::as_select()).load(conn)?;
    let (item_kind, pl, en, lesson_id, tags) = if let Some(w) = titles.words.get(item_id) {
        let tags = grammar::all_links(conn)?.remove(item_id).unwrap_or_default();
        ("word", w.pl.clone(), w.en.clone(), Some(w.lesson), tags)
    } else if let Some((pl, en, lesson)) = titles.sentences.get(item_id) {
        let tags = grammar::all_links(conn)?.remove(item_id).unwrap_or_default();
        ("sentence", pl.clone(), en.clone(), Some(*lesson), tags)
    } else if let Some(row) = rows.first() {
        let (pl, en) = titles.of(row);
        ("gone", pl, en, row.lesson_id, vec![])
    } else {
        return Err(Error::msg(format!("Nothing in the log about {item_id}.")));
    };
    let review = review_of(conn, item_kind, item_id)?;
    let results: Vec<bool> = rows.iter().map(Row::right).collect();
    Ok(ItemHistory {
        item_kind: item_kind.into(),
        item_id: item_id.into(),
        pl,
        en,
        lesson_id,
        strength: strength(review.as_ref(), &results),
        attempts: count(rows.len()),
        wrong: count(results.iter().filter(|r| !**r).count()),
        mistakes: class_counts(rows.iter()),
        history: rows.iter().rev().map(|r| view(r, &titles, true)).collect(),
        events: events_of(conn, item_id, 200)?,
        grammar: tags.into_iter().filter_map(grammar::by_id).map(TagInfo::from).collect(),
    })
}

// ---------------------------------------------------------------------------
// Words
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Ref<T> {
    pub id: T,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetWord {
    pub id: WordId,
    pub pl: String,
    pub en: String,
    pub pos: String,
    pub courses: Vec<Ref<CourseId>>,
    pub strength: Strength,
    pub attempts: i64,
    pub wrong: i64,
    pub score: f64,
    pub last_at: Option<i64>,
    pub image: Option<FileId>,
}

/// Every word met, trickiest first.
pub fn met_words(conn: &mut Conn) -> Result<Vec<MetWord>> {
    let ids = met_word_ids(conn)?;
    let titles = Titles::load(conn)?;
    let mut results: HashMap<String, (Vec<bool>, i64)> = HashMap::new();
    for (item, correct, at) in attempts::table
        .filter(attempts::item_kind.eq("word"))
        .order(attempts::at.asc())
        .select((attempts::item_id, attempts::correct, attempts::at))
        .load::<(String, i64, i64)>(conn)?
    {
        let e = results.entry(item).or_default();
        e.0.push(correct != 0);
        e.1 = at;
    }
    let reviews: HashMap<String, ReviewState> = reviews::table
        .filter(reviews::kind.eq("word"))
        .select((reviews::item_id, ReviewState::as_select()))
        .load::<(String, ReviewState)>(conn)?
        .into_iter()
        .collect();
    let course_titles: HashMap<CourseId, String> = courses::table.select((courses::id, courses::title_en)).load::<(CourseId, String)>(conn)?.into_iter().collect();
    let mut lesson_courses: HashMap<LessonId, Vec<CourseId>> = HashMap::new();
    for (lesson, course) in course_lessons::table.select((course_lessons::lesson_id, course_lessons::course_id)).load::<(LessonId, CourseId)>(conn)? {
        lesson_courses.entry(lesson).or_default().push(course);
    }
    let mut out: Vec<MetWord> = ids
        .into_iter()
        .filter_map(|id| {
            let w = titles.words.get(&id)?;
            let (answers, last) = results.get(&id).cloned().unwrap_or_default();
            let review = reviews.get(&id);
            Some(MetWord {
                id: WordId::parse(&id).ok()?,
                pl: w.pl.clone(),
                en: w.en.clone(),
                pos: w.pos.clone(),
                courses: lesson_courses
                    .get(&w.lesson)
                    .map(|cs| cs.iter().map(|c| Ref { id: *c, title: course_titles.get(c).cloned().unwrap_or_default() }).collect())
                    .unwrap_or_default(),
                strength: strength(review, &answers),
                attempts: count(answers.len()),
                wrong: count(answers.iter().filter(|r| !**r).count()),
                score: trickiness(&answers),
                last_at: (last > 0).then_some(last),
                image: w.image,
            })
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.pl.cmp(&b.pl)));
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Example {
    pub pl: String,
    pub en: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetWordDetail {
    pub word: MetWord,
    pub lessons: Vec<Ref<LessonId>>,
    /// Sentences using it: its own examples and the courses' sentences.
    pub examples: Vec<Example>,
    pub near: Option<Example>,
    pub history: ItemHistory,
    /// Every time it appeared, grouped by course, role, grammatical
    /// context, topic, exercise and source (contexts.rs).
    pub contexts: crate::contexts::WordContexts,
    /// Its topics.
    pub topics: Vec<crate::topics::TopicInfo>,
}

pub fn met_word(conn: &mut Conn, word_id: WordId) -> Result<MetWordDetail> {
    let id = word_id.to_string();
    let word = met_words(conn)?.into_iter().find(|w| w.id == word_id);
    let titles = Titles::load(conn)?;
    let info = titles.words.get(&id).cloned().ok_or_else(|| Error::msg("That word is gone."))?;
    let word = match word {
        Some(w) => w,
        None => MetWord { id: word_id, pl: info.pl.clone(), en: info.en.clone(), pos: info.pos.clone(), courses: vec![], strength: Strength::New, attempts: 0, wrong: 0, score: 0.0, last_at: None, image: info.image },
    };
    // Every lesson that teaches the same word (the same Polish), where it
    // is met.
    let mut lessons: Vec<Ref<LessonId>> = Vec::new();
    for other in titles.words.values().filter(|w| w.pl.to_lowercase() == info.pl.to_lowercase()) {
        if !lessons.iter().any(|l| l.id == other.lesson) {
            lessons.push(Ref { id: other.lesson, title: titles.lessons.get(&other.lesson).cloned().unwrap_or_default() });
        }
    }
    let mut examples: Vec<Example> = word_examples::table
        .find(word_id)
        .select(word_examples::examples)
        .first::<String>(conn)
        .optional()?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    // Course sentences with the word in them (its stem, so "kawę" counts for "kawa").
    let stem: String = crate::practice::normalize(&info.pl).chars().take(crate::practice::normalize(&info.pl).chars().count().saturating_sub(1).max(3)).collect();
    if !info.pl.contains(' ') && stem.chars().count() >= 3 {
        for (pl, en, _) in titles.sentences.values() {
            if examples.len() >= 5 {
                break;
            }
            if crate::practice::normalize(pl).split(' ').any(|w| w.starts_with(&stem)) && !examples.iter().any(|e| e.pl == *pl) {
                examples.push(Example { pl: pl.clone(), en: en.clone() });
            }
        }
    }
    let near = (!info.near.0.is_empty()).then(|| Example { pl: info.near.0.clone(), en: info.near.1.clone() });
    let contexts = crate::contexts::item_contexts(conn, &id, &Filter::default())?;
    let topics = crate::topics::all_links(conn)?
        .remove(&id)
        .unwrap_or_default()
        .into_iter()
        .filter_map(crate::topics::by_id)
        .map(crate::topics::TopicInfo::from)
        .collect();
    Ok(MetWordDetail { word, lessons, examples, near, history: item_history(conn, &id).or_else(|_| empty_history(conn, &id, &info))?, contexts, topics })
}

fn empty_history(conn: &mut Conn, id: &str, info: &WordInfo) -> Result<ItemHistory> {
    let tags = grammar::all_links(conn)?.remove(id).unwrap_or_default();
    Ok(ItemHistory {
        item_kind: "word".into(),
        item_id: id.into(),
        pl: info.pl.clone(),
        en: info.en.clone(),
        lesson_id: Some(info.lesson),
        strength: Strength::New,
        attempts: 0,
        wrong: 0,
        mistakes: vec![],
        history: vec![],
        events: events_of(conn, id, 200)?,
        grammar: tags.into_iter().filter_map(grammar::by_id).map(TagInfo::from).collect(),
    })
}

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StructureRow {
    pub tag: TagInfo,
    /// Words and sentences with it.
    pub items: i64,
    pub attempts: i64,
    pub correct: i64,
    pub accuracy: Option<f64>,
    pub score: f64,
    pub weeks: Vec<DayPoint>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Structures {
    pub tags: Vec<StructureRow>,
    /// Words and sentences in open courses with no tag yet.
    pub untagged: i64,
    /// Sentences in open courses not yet annotated word by word.
    pub unannotated: i64,
}

fn structure_rows(conn: &mut Conn, f: &Filter) -> Result<(Vec<StructureRow>, Vec<Row>, Tagged)> {
    let rows = load(conn, f)?;
    let tagged = Tagged::load(conn)?;
    let mut items: HashMap<GrammarTagId, i64> = HashMap::new();
    for tags in tagged.of_item.values() {
        for tag in tags {
            *items.entry(*tag).or_default() += 1;
        }
    }
    let mut answers: HashMap<GrammarTagId, Vec<&Row>> = HashMap::new();
    for row in &rows {
        for tag in tagged.of(row) {
            answers.entry(tag).or_default().push(row);
        }
    }
    let mut out: Vec<StructureRow> = grammar::TAGS
        .iter()
        .filter_map(|t| {
            let id = grammar::id_of(t.key);
            let n = items.get(&id).copied().unwrap_or(0);
            let rows = answers.get(&id).cloned().unwrap_or_default();
            if n == 0 && rows.is_empty() {
                return None;
            }
            let results: Vec<bool> = rows.iter().map(|r| r.right()).collect();
            let correct = count(results.iter().filter(|r| **r).count());
            Some(StructureRow {
                tag: t.into(),
                items: n,
                attempts: count(rows.len()),
                correct,
                accuracy: (!rows.is_empty()).then(|| accuracy(correct, count(rows.len()))),
                score: trickiness(&results),
                weeks: weeks(rows.iter().copied()),
            })
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.items.cmp(&a.items)));
    Ok((out, rows, tagged))
}

pub fn structures(conn: &mut Conn, f: &Filter) -> Result<Structures> {
    let (tags, _, _) = structure_rows(conn, f)?;
    Ok(Structures { tags, untagged: grammar::untagged_count(conn)?, unannotated: grammar::unannotated_count(conn)? })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaggedItem {
    pub item_kind: String,
    pub item_id: String,
    pub pl: String,
    pub en: String,
    pub attempts: i64,
    pub wrong: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StructureDetail {
    pub row: StructureRow,
    /// Its words and sentences, the most missed first.
    pub examples: Vec<TaggedItem>,
    pub classes: Vec<ClassCount>,
    /// Misses on its items, newest first.
    pub mistakes: Vec<AttemptView>,
    /// The words met in it, each with how it went there (contexts.rs).
    pub words: Vec<crate::contexts::WordInContext>,
}

pub fn structure(conn: &mut Conn, key: &str, f: &Filter) -> Result<StructureDetail> {
    let tag = grammar::by_key(key).ok_or_else(|| Error::msg(format!("Unknown grammar tag '{key}'.")))?;
    let id = grammar::id_of(tag.key);
    let (rows_all, rows, tagged) = structure_rows(conn, f)?;
    let titles = Titles::load(conn)?;
    let row = rows_all.into_iter().find(|r| r.tag.id == id).unwrap_or(StructureRow {
        tag: tag.into(),
        items: 0,
        attempts: 0,
        correct: 0,
        accuracy: None,
        score: 0.0,
        weeks: vec![],
    });
    let mine: Vec<&Row> = rows.iter().filter(|r| tagged.of(r).contains(&id)).collect();
    let mut examples: Vec<TaggedItem> = tagged
        .of_item
        .iter()
        .filter(|(_, tags)| tags.contains(&id))
        .filter_map(|(item, _)| {
            let (kind, pl, en) = if let Some(w) = titles.words.get(item) {
                ("word", w.pl.clone(), w.en.clone())
            } else {
                let (pl, en, _) = titles.sentences.get(item)?;
                ("sentence", pl.clone(), en.clone())
            };
            let theirs: Vec<&&Row> = mine.iter().filter(|r| r.item_id == *item).collect();
            Some(TaggedItem {
                item_kind: kind.into(),
                item_id: item.clone(),
                pl,
                en,
                attempts: count(theirs.len()),
                wrong: count(theirs.iter().filter(|r| !r.right()).count()),
            })
        })
        .collect();
    examples.sort_by(|a, b| b.wrong.cmp(&a.wrong).then(b.attempts.cmp(&a.attempts)).then(a.pl.cmp(&b.pl)));
    examples.truncate(60);
    Ok(StructureDetail {
        row,
        examples,
        classes: class_counts(mine.iter().copied()),
        mistakes: mine.iter().rev().filter(|r| !r.right()).take(30).map(|r| view(r, &titles, true)).collect(),
        words: crate::contexts::structure_words(conn, key, f)?,
    })
}

// ---------------------------------------------------------------------------
// Sounds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SoundSide {
    /// The sound said (its spelling: "ś").
    pub said: String,
    pub attempts: i64,
    pub missed: i64,
    /// What it was most often heard as.
    pub heard_as: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SoundRow {
    pub label: String,
    pub attempts: i64,
    pub hits: i64,
    pub accuracy: f64,
    pub sides: Vec<SoundSide>,
    /// Misses, newest first (sound pairs and speech).
    pub examples: Vec<AttemptView>,
}

pub fn sounds(conn: &mut Conn, f: &Filter) -> Result<Vec<SoundRow>> {
    let rows = load(conn, f)?;
    let titles = Titles::load(conn)?;
    let mut by: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for row in rows.iter().filter(|r| !r.contrast.is_empty()) {
        by.entry(row.contrast.clone()).or_default().push(row);
    }
    let mut out: Vec<SoundRow> = by
        .into_iter()
        .map(|(label, rows)| {
            let mut sides: BTreeMap<String, (i64, i64, HashMap<String, i64>)> = BTreeMap::new();
            for row in &rows {
                // Right: the detail is the sound said. Missed: "said→heard"
                // (a sound pair, or one word heard); a sentence's per-word
                // code says nothing about the side.
                let (said, heard) = match row.detail.split_once('→') {
                    Some((s, h)) => (s.to_string(), Some(h.to_string())),
                    None if row.right() => (row.detail.clone(), None),
                    None => (String::new(), None),
                };
                if said.is_empty() || said.chars().count() > 3 {
                    continue;
                }
                let e = sides.entry(said).or_default();
                e.0 += 1;
                if let Some(h) = heard {
                    e.1 += 1;
                    *e.2.entry(h).or_default() += 1;
                }
            }
            let hits = count(rows.iter().filter(|r| r.right()).count());
            SoundRow {
                accuracy: accuracy(hits, count(rows.len())),
                label,
                attempts: count(rows.len()),
                hits,
                sides: sides
                    .into_iter()
                    .map(|(said, (attempts, missed, heard))| SoundSide {
                        said,
                        attempts,
                        missed,
                        heard_as: heard.into_iter().max_by_key(|(_, n)| *n).map(|(h, _)| h).unwrap_or_default(),
                    })
                    .collect(),
                examples: rows.iter().rev().filter(|r| !r.right()).take(12).map(|r| view(r, &titles, true)).collect(),
            }
        })
        .collect();
    out.sort_by(|a, b| a.accuracy.total_cmp(&b.accuracy).then(b.attempts.cmp(&a.attempts)));
    Ok(out)
}

// ---------------------------------------------------------------------------
// Mistakes by class
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemCount {
    pub item_kind: String,
    pub item_id: String,
    pub title: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MistakePage {
    pub class: String,
    pub label: String,
    pub count: i64,
    /// Its share of every mistake in the period.
    pub share: f64,
    /// The items it happened on most.
    pub items: Vec<ItemCount>,
    /// Newest first.
    pub examples: Vec<AttemptView>,
}

pub fn mistakes_of(conn: &mut Conn, class: &str, f: &Filter, limit: usize) -> Result<MistakePage> {
    if !mistakes::CLASSES.iter().any(|(c, _)| *c == class) {
        let known: Vec<&str> = mistakes::CLASSES.iter().map(|(c, _)| *c).collect();
        return Err(Error::msg(format!("Unknown mistake class '{class}': {}.", known.join(", "))));
    }
    let rows = load(conn, f)?;
    let titles = Titles::load(conn)?;
    let all = count(rows.iter().filter(|r| !r.right() && !r.mistake.is_empty()).count());
    let mine: Vec<&Row> = rows.iter().filter(|r| !r.right() && r.mistake == class).collect();
    let mut items: HashMap<String, ItemCount> = HashMap::new();
    for row in &mine {
        let e = items.entry(row.item_id.clone()).or_insert_with(|| ItemCount {
            item_kind: row.item_kind.clone(),
            item_id: row.item_id.clone(),
            title: titles.of(row).0,
            count: 0,
        });
        e.count += 1;
    }
    let mut items: Vec<ItemCount> = items.into_values().collect();
    items.sort_by(|a, b| b.count.cmp(&a.count).then(a.title.cmp(&b.title)));
    items.truncate(20);
    Ok(MistakePage {
        class: class.into(),
        label: mistakes::label(class).into(),
        count: count(mine.len()),
        share: accuracy(count(mine.len()), all),
        items,
        examples: mine.iter().rev().take(limit.clamp(1, 200)).map(|r| view(r, &titles, true)).collect(),
    })
}

// ---------------------------------------------------------------------------
// Two periods compared
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub filter: Filter,
    pub attempts: i64,
    pub correct: i64,
    pub accuracy: f64,
    pub days_active: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// word | sentence | structure | sound
    pub kind: String,
    pub key: String,
    pub title: String,
    pub attempts_a: i64,
    pub accuracy_a: f64,
    pub attempts_b: i64,
    pub accuracy_b: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassChange {
    pub class: String,
    pub label: String,
    pub a: i64,
    pub b: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    /// The earlier period and the later one.
    pub a: Summary,
    pub b: Summary,
    pub improved: Vec<Change>,
    pub slipped: Vec<Change>,
    pub mistakes: Vec<ClassChange>,
}

fn summary(f: &Filter, rows: &[Row]) -> Summary {
    let correct = count(rows.iter().filter(|r| r.right()).count());
    Summary {
        filter: *f,
        attempts: count(rows.len()),
        correct,
        accuracy: accuracy(correct, count(rows.len())),
        days_active: count(rows.iter().map(|r| r.day).collect::<HashSet<_>>().len()),
    }
}

/// Every word, sentence, structure and sound with its answers in a period.
fn groups(rows: &[Row], tagged: &Tagged) -> HashMap<(String, String), (i64, i64)> {
    let mut by: HashMap<(String, String), (i64, i64)> = HashMap::new();
    for row in rows {
        let right = i64::from(row.right());
        let mut add = |kind: &str, key: String| {
            let e = by.entry((kind.to_string(), key)).or_default();
            e.0 += 1;
            e.1 += right;
        };
        if row.item_kind == "word" || row.item_kind == "sentence" {
            add(&row.item_kind, row.item_id.clone());
        }
        for tag in tagged.of(row) {
            add("structure", tag.to_string());
        }
        if !row.contrast.is_empty() {
            add("sound", row.contrast.clone());
        }
    }
    by
}

pub fn compare(conn: &mut Conn, a: &Filter, b: &Filter) -> Result<Comparison> {
    let (ra, rb) = (load(conn, a)?, load(conn, b)?);
    let tagged = Tagged::load(conn)?;
    let titles = Titles::load(conn)?;
    let (ga, gb) = (groups(&ra, &tagged), groups(&rb, &tagged));
    let mut changes: Vec<(f64, Change)> = Vec::new();
    for (key, (na, ca)) in &ga {
        let Some((nb, cb)) = gb.get(key) else { continue };
        if *na < 2 || *nb < 2 {
            continue;
        }
        let (acc_a, acc_b) = (accuracy(*ca, *na), accuracy(*cb, *nb));
        let (kind, k) = key;
        let (key_out, title) = match kind.as_str() {
            "structure" => {
                let t = GrammarTagId::parse(k).ok().and_then(grammar::by_id);
                (t.map_or(k.clone(), |t| t.key.into()), t.map_or(k.clone(), |t| t.name.into()))
            }
            "word" => (k.clone(), titles.words.get(k).map_or(k.clone(), |w| w.pl.clone())),
            "sentence" => (k.clone(), titles.sentences.get(k).map_or(k.clone(), |s| s.0.clone())),
            _ => (k.clone(), k.clone()),
        };
        changes.push((acc_b - acc_a, Change { kind: kind.clone(), key: key_out, title, attempts_a: *na, accuracy_a: acc_a, attempts_b: *nb, accuracy_b: acc_b }));
    }
    changes.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.title.cmp(&y.1.title)));
    let improved: Vec<Change> = changes.iter().filter(|(d, _)| *d >= 0.15).take(12).map(|(_, c)| c.clone()).collect();
    let slipped: Vec<Change> = changes.iter().rev().filter(|(d, _)| *d <= -0.15).take(12).map(|(_, c)| c.clone()).collect();
    let (ca, cb) = (class_counts(ra.iter()), class_counts(rb.iter()));
    let mut classes: Vec<ClassChange> = mistakes::CLASSES
        .iter()
        .map(|(class, label)| ClassChange {
            class: (*class).into(),
            label: (*label).into(),
            a: ca.iter().find(|c| c.class == *class).map_or(0, |c| c.count),
            b: cb.iter().find(|c| c.class == *class).map_or(0, |c| c.count),
        })
        .filter(|c| c.a + c.b > 0)
        .collect();
    classes.sort_by_key(|c| std::cmp::Reverse(c.a + c.b));
    Ok(Comparison { a: summary(a, &ra), b: summary(b, &rb), improved, slipped, mistakes: classes })
}

// ---------------------------------------------------------------------------
// Activity and habits
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Exit {
    pub lesson_id: Option<LessonId>,
    pub title: String,
    pub at: i64,
    pub at_card: Option<i64>,
    pub of_cards: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LessonCount {
    pub lesson_id: LessonId,
    pub title: String,
    pub attempts: i64,
    pub accuracy: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub filter: Filter,
    pub days_active: i64,
    pub minutes: i64,
    pub attempts: i64,
    /// Drills started, finished, left before the end.
    pub sessions: i64,
    pub finished: i64,
    pub left_early: i64,
    pub games: i64,
    pub checkins: i64,
    /// Answers by hour of the day (local, 0–23) and by weekday (Monday first).
    pub by_hour: Vec<i64>,
    pub by_weekday: Vec<i64>,
    /// The median time to answer, in milliseconds.
    pub median_ms: Option<i64>,
    pub audio_plays: i64,
    pub reveals: i64,
    pub exits: Vec<Exit>,
    pub lessons: Vec<LessonCount>,
    /// Everything else done, by kind.
    pub events: Vec<ClassCount>,
}

pub fn activity(conn: &mut Conn, f: &Filter) -> Result<Activity> {
    let rows = load(conn, f)?;
    let titles = Titles::load(conn)?;
    let offset_ms = db::get_setting_i64(conn, "tz_offset_minutes", 0) * 60_000;
    let mut by_hour = vec![0i64; 24];
    let mut by_weekday = vec![0i64; 7];
    for row in &rows {
        let local = row.at + offset_ms;
        let hour = usize::try_from((local / 3_600_000).rem_euclid(24)).unwrap_or(0);
        by_hour[hour] += 1;
        let weekday = usize::try_from((row.day + 3).rem_euclid(7)).unwrap_or(0);
        by_weekday[weekday] += 1;
    }
    let mut times: Vec<i64> = rows.iter().filter_map(|r| r.ms_answer).filter(|ms| *ms > 0).collect();
    times.sort_unstable();
    let median_ms = times.get(times.len() / 2).copied();
    let mut events_q = learning_events::table.select((learning_events::kind, learning_events::at, learning_events::lesson_id, learning_events::details)).into_boxed();
    if let Some(from) = f.from_day {
        events_q = events_q.filter(learning_events::day.ge(from));
    }
    if let Some(to) = f.to_day {
        events_q = events_q.filter(learning_events::day.le(to));
    }
    if let Some(course) = f.course_id {
        let lessons = course_lessons_of(conn, course)?;
        events_q = events_q.filter(learning_events::lesson_id.eq_any(lessons));
    }
    let events: Vec<(String, i64, Option<LessonId>, String)> = events_q.order(learning_events::at.desc()).limit(ROW_LIMIT).load(conn)?;
    let kinds = |k: &str| count(events.iter().filter(|e| e.0 == k).count());
    let mut by_kind: BTreeMap<&str, i64> = BTreeMap::new();
    for e in &events {
        *by_kind.entry(e.0.as_str()).or_default() += 1;
    }
    let mut event_counts: Vec<ClassCount> = by_kind.into_iter().map(|(k, n)| ClassCount { class: k.into(), label: event_label(k), count: n }).collect();
    event_counts.sort_by_key(|c| std::cmp::Reverse(c.count));
    let exits: Vec<Exit> = events
        .iter()
        .filter(|e| e.0 == "lesson_left")
        .take(20)
        .map(|(_, at, lesson, details)| {
            let d: EventDetails = serde_json::from_str(details).unwrap_or_default();
            Exit { lesson_id: *lesson, title: lesson.and_then(|l| titles.lessons.get(&l).cloned()).unwrap_or_default(), at: *at, at_card: d.at_card, of_cards: d.of_cards }
        })
        .collect();
    let mut per_lesson: HashMap<LessonId, (i64, i64)> = HashMap::new();
    for row in &rows {
        if let Some(lesson) = row.lesson_id {
            let e = per_lesson.entry(lesson).or_default();
            e.0 += 1;
            e.1 += i64::from(row.right());
        }
    }
    let mut lessons: Vec<LessonCount> = per_lesson
        .into_iter()
        .map(|(lesson_id, (n, c))| LessonCount { lesson_id, title: titles.lessons.get(&lesson_id).cloned().unwrap_or_default(), attempts: n, accuracy: accuracy(c, n) })
        .collect();
    lessons.sort_by(|a, b| b.attempts.cmp(&a.attempts).then(a.title.cmp(&b.title)));
    lessons.truncate(12);
    let overview_minutes = minutes_in(conn, f)?;
    let answering_minutes = round(real(rows.iter().filter_map(|r| r.ms_answer).map(|ms| ms.min(120_000)).sum::<i64>()) / 60_000.0);
    Ok(Activity {
        filter: *f,
        days_active: count(rows.iter().map(|r| r.day).collect::<HashSet<_>>().len()),
        minutes: overview_minutes.max(answering_minutes),
        attempts: count(rows.len()),
        sessions: kinds("lesson_started"),
        finished: kinds("lesson_finished"),
        left_early: kinds("lesson_left"),
        games: kinds("game_finished"),
        checkins: kinds("checkin_started"),
        by_hour,
        by_weekday,
        median_ms,
        audio_plays: rows.iter().map(|r| r.plays).sum::<i64>() + kinds("audio_played"),
        reveals: rows.iter().map(|r| r.reveals).sum(),
        exits,
        lessons,
        events: event_counts,
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn analytics_overview(db: State<'_, Db>, filter: Filter) -> Result<Overview> {
    overview(&mut db.lock(), &filter)
}

#[tauri::command]
pub async fn analytics_hard(db: State<'_, Db>, kind: String, filter: Filter, limit: Option<i64>) -> Result<Vec<HardItem>> {
    hard_items(&mut db.lock(), &kind, &filter, crate::convert::index(limit.unwrap_or(50)))
}

#[tauri::command]
pub async fn analytics_mistakes(db: State<'_, Db>, class: String, filter: Filter) -> Result<MistakePage> {
    mistakes_of(&mut db.lock(), &class, &filter, 60)
}

#[tauri::command]
pub async fn analytics_compare(db: State<'_, Db>, a: Filter, b: Filter) -> Result<Comparison> {
    compare(&mut db.lock(), &a, &b)
}

#[tauri::command]
pub async fn analytics_activity(db: State<'_, Db>, filter: Filter) -> Result<Activity> {
    activity(&mut db.lock(), &filter)
}

#[tauri::command]
pub async fn analytics_item(db: State<'_, Db>, item_id: String) -> Result<ItemHistory> {
    item_history(&mut db.lock(), &item_id)
}

#[tauri::command]
pub async fn met_words_list(db: State<'_, Db>) -> Result<Vec<MetWord>> {
    met_words(&mut db.lock())
}

#[tauri::command]
pub async fn met_word_detail(db: State<'_, Db>, word_id: WordId) -> Result<MetWordDetail> {
    met_word(&mut db.lock(), word_id)
}

#[tauri::command]
pub async fn met_structures(db: State<'_, Db>, filter: Filter) -> Result<Structures> {
    structures(&mut db.lock(), &filter)
}

#[tauri::command]
pub async fn met_structure(db: State<'_, Db>, key: String, filter: Filter) -> Result<StructureDetail> {
    structure(&mut db.lock(), &key, &filter)
}

#[tauri::command]
pub async fn met_sounds(db: State<'_, Db>, filter: Filter) -> Result<Vec<SoundRow>> {
    sounds(&mut db.lock(), &filter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(interval: i64, reps: i64, lapses: i64) -> ReviewState {
        ReviewState { due_day: 0, interval, ease: 2.5, reps, lapses, last_day: 0 }
    }

    #[test]
    fn strength_from_the_schedule_and_the_answers() {
        assert_eq!(strength(None, &[]), Strength::New);
        assert_eq!(strength(Some(&review(0, 0, 0)), &[]), Strength::New);
        assert_eq!(strength(None, &[true]), Strength::Learning);
        assert_eq!(strength(None, &[true, false, false]), Strength::Tricky, "the last two missed");
        assert_eq!(strength(None, &[false, true, false, true]), Strength::Tricky, "half missed");
        assert_eq!(strength(Some(&review(30, 5, 0)), &[true, true]), Strength::Solid, "three weeks apart");
        assert_eq!(strength(Some(&review(6, 3, 1)), &[false, true, true, true, true]), Strength::Solid, "the last four right");
        assert_eq!(strength(Some(&review(30, 5, 3)), &[true, false, true, true]), Strength::Tricky, "forgotten often, missed lately");
        assert_eq!(strength(Some(&review(3, 1, 0)), &[false, true, true, true, true, true, true, true, true]), Strength::Learning);
    }

    #[test]
    fn trickiness_weighs_the_latest_most() {
        assert_eq!(trickiness(&[]), 0.0);
        assert!(trickiness(&[true, false]) > trickiness(&[false, true]), "a recent miss counts more");
        assert!(trickiness(&[false, false, false]) > trickiness(&[false]), "more evidence, more sure");
        assert!(trickiness(&[true; 5]) == 0.0);
    }

    #[test]
    fn weeks_start_on_monday() {
        // Day 0 was a Thursday; day 4 a Monday.
        assert_eq!(monday(4), 4);
        assert_eq!(monday(10), 4);
        assert_eq!(monday(11), 11);
        assert_eq!(monday(3), -3);
    }
}
