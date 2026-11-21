//! The raw learning log: every answer and every event, newest first, a page
//! at a time, with every field — what the card showed (its prompt, the
//! options in their order, the tiles, its place in the run), what was given
//! and expected, timings, tries, plays, reveals, the words it involved and
//! their roles and forms, a check-in's verdict — filtered by item, course,
//! lesson, exercise, mistake, topic, grammar, right or wrong, source and
//! period. Progress → Insights → Raw log shows it; the tutor reads it
//! compactly (`get_raw_log`); Export writes it as CSV or JSON.
//!
//! Paging is by a cursor (the last row's time and id), so a page costs the
//! same however deep it is: each table is read newest first, past the
//! cursor, with its indexes, and the two are merged.

use std::collections::{HashMap, HashSet};

use diesel::prelude::*;
use diesel::sqlite::Sqlite;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::analytics::{self, Filter};
use crate::contexts::{self, Subject};
use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
#[allow(unused_imports)]
use crate::ids::*;
use crate::learnlog::{read_details, CardShown, EventDetails, Verdict};
use crate::mistakes::WordResult;
use crate::schema::{attempt_items, attempts, conversation_lines, courses, learning_events, lessons, sentence_tokens, sentences, words};

/// The most rows a page holds.
// Spec: LOG-6
pub const PAGE_MAX: i64 = 200;
/// The most rows an export writes.
// Spec: LOG-9
pub const EXPORT_MAX: usize = 100_000;

/// A drill-down from a contextual breakdown: the answers in one group.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextRef {
    pub group_by: String,
    pub key: String,
}

/// Which rows. Everything optional; filters that only answers have
/// (exercise, mistake, right or wrong, grammar, a drill-down) leave events out.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RawFilter {
    #[serde(default)]
    pub from_day: Option<i64>,
    #[serde(default)]
    pub to_day: Option<i64>,
    #[serde(default)]
    pub course_id: Option<CourseId>,
    #[serde(default)]
    pub lesson_id: Option<LessonId>,
    /// One word, sentence or line: its own answers and every answer it appeared in.
    #[serde(default)]
    pub item_id: Option<String>,
    /// With `item_id`: only where it had this role.
    #[serde(default)]
    pub role: Option<String>,
    /// Text: words and sentences whose Polish or English has it, and
    /// answers whose prompt, given or expected has it.
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub exercise: Option<String>,
    #[serde(default)]
    pub mistake: Option<String>,
    /// A topic's key (a domain takes its topics in).
    #[serde(default)]
    pub topic: Option<String>,
    /// A grammar tag's key.
    #[serde(default)]
    pub grammar: Option<String>,
    #[serde(default)]
    pub correct: Option<bool>,
    #[serde(default)]
    pub source: Option<String>,
    /// "answers", "events", or both (empty).
    #[serde(default)]
    pub kind: Option<String>,
    /// The answers in one group of a breakdown of `item_id`, `grammar` or `topic`.
    #[serde(default)]
    pub context: Option<ContextRef>,
}

/// Where a page ends: the next starts after it.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Cursor {
    pub at: i64,
    pub id: String,
}

/// A word the answer involved.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LinkView {
    pub item_id: String,
    pub item_kind: String,
    pub pl: String,
    pub en: String,
    pub role: String,
    pub role_label: String,
    pub form: String,
    pub position: i64,
    pub correct: Option<bool>,
    /// The grammar the sentence's annotation gives it there.
    pub case: String,
    pub reason: String,
}

/// Everything an answer kept, as the raw log shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnswerView {
    pub exercise: String,
    pub exercise_label: String,
    pub direction: String,
    pub prompt: String,
    pub card_key: String,
    pub position: Option<i64>,
    pub run: String,
    pub correct: bool,
    pub given: String,
    pub expected: String,
    pub mistake: String,
    pub mistake_label: String,
    pub detail: String,
    pub contrast: String,
    pub ms_first: Option<i64>,
    pub ms_answer: Option<i64>,
    pub tries: i64,
    pub hints: i64,
    pub plays: i64,
    pub slow_plays: i64,
    pub reveals: i64,
    pub changed: bool,
    pub first_pick: Option<String>,
    /// Every option, in the order shown.
    pub options: Vec<String>,
    pub words: Vec<WordResult>,
    pub shown: Option<CardShown>,
    pub checkin_id: Option<CheckinId>,
    pub verdict: Option<Verdict>,
    pub grammar: Vec<String>,
    pub note: String,
    pub links: Vec<LinkView>,
}

/// One row of the log: an answer or an event.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RawRow {
    /// "answer" or "event".
    pub kind: String,
    pub id: String,
    pub at: i64,
    pub day: i64,
    pub device: String,
    pub source: String,
    pub source_label: String,
    pub course_id: Option<CourseId>,
    pub course: String,
    pub lesson_id: Option<LessonId>,
    pub lesson: String,
    pub item_kind: String,
    pub item_id: String,
    /// The item's Polish and English (a lesson's title for a card about it).
    pub item: String,
    pub item_en: String,
    pub answer: Option<AnswerView>,
    /// An event's kind and what it kept.
    pub event: String,
    pub event_label: String,
    pub event_details: Option<EventDetails>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RawPage {
    pub rows: Vec<RawRow>,
    /// Where the next page starts; none at the end.
    pub next: Option<Cursor>,
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = attempts)]
struct AttemptRow {
    id: AttemptId,
    at: i64,
    day: i64,
    device: DeviceId,
    item_kind: String,
    item_id: String,
    lesson_id: Option<LessonId>,
    exercise: String,
    run: String,
    correct: i64,
    given: String,
    expected: String,
    mistake: String,
    detail: String,
    contrast: String,
    ms_first: Option<i64>,
    ms_answer: Option<i64>,
    tries: i64,
    hints: i64,
    plays: i64,
    slow_plays: i64,
    reveals: i64,
    changed: i64,
    details: String,
    course_id: Option<CourseId>,
    source: String,
    card_key: String,
    direction: String,
    prompt: String,
    position: Option<i64>,
    checkin_id: Option<CheckinId>,
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = learning_events)]
struct EventRow {
    id: LearningEventId,
    at: i64,
    day: i64,
    device: DeviceId,
    kind: String,
    item_kind: String,
    item_id: String,
    lesson_id: Option<LessonId>,
    details: String,
    course_id: Option<CourseId>,
    source: String,
}

/// The sets a filter needs, worked out once per page.
struct Resolved {
    course_lessons: Option<Vec<LessonId>>,
    /// Items found by the search, by topic, by grammar.
    search_items: Option<Vec<String>>,
    topic_items: Option<(Vec<String>, Vec<LessonId>)>,
    grammar_items: Option<Vec<String>>,
    context_ids: Option<Vec<AttemptId>>,
    answers: bool,
    events: bool,
}

fn like(text: &str) -> String {
    format!("%{}%", text.replace(['%', '_'], ""))
}

fn resolve(conn: &mut Conn, f: &RawFilter) -> Result<Resolved> {
    let course_lessons = match f.course_id {
        Some(c) => Some(analytics::course_lessons_of(conn, c)?),
        None => None,
    };
    let search_items = match f.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(q) => {
            let q = like(q);
            let mut ids: Vec<String> = words::table
                .filter(words::pl.like(&q).or(words::en.like(&q)))
                .select(words::id)
                .limit(2000)
                .load::<WordId>(conn)?
                .into_iter()
                .map(|w| w.to_string())
                .collect();
            ids.extend(
                sentences::table
                    .filter(sentences::pl.like(&q).or(sentences::en.like(&q)))
                    .select(sentences::id)
                    .limit(2000)
                    .load::<SentenceId>(conn)?
                    .into_iter()
                    .map(|s| s.to_string()),
            );
            Some(ids)
        }
        None => None,
    };
    let topic_items = match f.topic.as_deref().filter(|s| !s.is_empty()) {
        Some(key) => {
            let wanted = contexts::topic_ids(key)?;
            let links = crate::topics::all_links(conn)?;
            let mut items = Vec::new();
            let mut lessons = Vec::new();
            for (id, ts) in links {
                if ts.iter().any(|t| wanted.contains(t)) {
                    match LessonId::parse(&id) {
                        Ok(l) => lessons.push(l),
                        Err(_) => items.push(id),
                    }
                }
            }
            Some((items, lessons))
        }
        None => None,
    };
    let grammar_items = match f.grammar.as_deref().filter(|s| !s.is_empty()) {
        Some(key) => {
            let tag = crate::grammar::by_key(key).ok_or_else(|| Error::msg(format!("Unknown grammar tag '{key}'.")))?;
            let id = crate::grammar::id_of(tag.key);
            let mut items: Vec<String> =
                crate::grammar::all_links(conn)?.into_iter().filter(|(_, tags)| tags.contains(&id)).map(|(item, _)| item).collect();
            // Sentences whose annotation uses it (as a case or a reason).
            let annotated: Vec<SentenceId> = sentence_tokens::table
                .filter(sentence_tokens::reason.eq(tag.key).or(sentence_tokens::gram_case.eq(tag.key)))
                .select(sentence_tokens::sentence_id)
                .distinct()
                .load(conn)?;
            items.extend(annotated.into_iter().map(|s| s.to_string()));
            Some(items)
        }
        None => None,
    };
    let context_ids = match &f.context {
        Some(c) => {
            let subject = Subject { item_id: f.item_id.clone(), tag: f.grammar.clone(), topic: f.topic.clone() };
            let span = Filter { from_day: f.from_day, to_day: f.to_day, course_id: None };
            Some(contexts::attempt_ids(conn, &subject, &c.group_by, &c.key, &span)?)
        }
        None => None,
    };
    if let Some(source) = f.source.as_deref().filter(|s| !s.is_empty()) {
        if !crate::learnlog::SOURCES.iter().any(|(k, _)| *k == source) && source != "none" {
            return Err(Error::msg(format!("Unknown source '{source}'.")));
        }
    }
    if let Some(role) = f.role.as_deref().filter(|s| !s.is_empty()) {
        if !crate::wordlinks::ROLES.iter().any(|(k, _)| *k == role) {
            return Err(Error::msg(format!("Unknown role '{role}'.")));
        }
    }
    let answer_only = f.exercise.is_some() || f.mistake.is_some() || f.correct.is_some() || f.grammar.is_some() || f.context.is_some() || f.role.is_some();
    let kind = f.kind.as_deref().unwrap_or("");
    if !["", "answers", "events"].contains(&kind) {
        return Err(Error::msg(format!("Unknown kind '{kind}': answers or events.")));
    }
    Ok(Resolved {
        course_lessons,
        search_items,
        topic_items,
        grammar_items,
        context_ids,
        answers: kind != "events",
        events: kind != "answers" && !answer_only,
    })
}

fn attempts_query<'a>(f: &'a RawFilter, r: &'a Resolved, before: Option<&Cursor>) -> attempts::BoxedQuery<'a, Sqlite> {
    let mut q = attempts::table.into_boxed();
    if let Some(c) = before {
        q = match AttemptId::parse(&c.id) {
            Ok(id) => q.filter(attempts::at.lt(c.at).or(attempts::at.eq(c.at).and(attempts::id.lt(id)))),
            // An event's id sorts after every answer's at the same moment.
            Err(_) => q.filter(attempts::at.le(c.at)),
        };
    }
    if let Some(from) = f.from_day {
        q = q.filter(attempts::day.ge(from));
    }
    if let Some(to) = f.to_day {
        q = q.filter(attempts::day.le(to));
    }
    if let (Some(course), Some(lessons)) = (f.course_id, &r.course_lessons) {
        q = q.filter(attempts::course_id.eq(course).or(attempts::course_id.is_null().and(attempts::lesson_id.eq_any(lessons.clone()))));
    }
    if let Some(lesson) = f.lesson_id {
        q = q.filter(attempts::lesson_id.eq(lesson));
    }
    if let Some(item) = f.item_id.as_deref().map(str::trim).filter(|s| !s.is_empty()).filter(|_| f.context.is_none()) {
        let mut links = attempt_items::table.filter(attempt_items::item_id.eq(item)).into_boxed();
        if let Some(role) = f.role.as_deref().filter(|s| !s.is_empty()) {
            links = links.filter(attempt_items::role.eq(role));
            q = q.filter(attempts::id.eq_any(links.select(attempt_items::attempt_id)));
        } else {
            q = q.filter(attempts::item_id.eq(item).or(attempts::id.eq_any(links.select(attempt_items::attempt_id))));
        }
    }
    if let Some(items) = &r.search_items {
        let text = like(f.search.as_deref().unwrap_or_default().trim());
        q = q.filter(
            attempts::item_id
                .eq_any(items.clone())
                .or(attempts::id.eq_any(attempt_items::table.filter(attempt_items::item_id.eq_any(items.clone())).select(attempt_items::attempt_id)))
                .or(attempts::given.like(text.clone()))
                .or(attempts::expected.like(text.clone()))
                .or(attempts::prompt.like(text)),
        );
    }
    if let Some(exercise) = f.exercise.as_deref().filter(|s| !s.is_empty()) {
        q = q.filter(attempts::exercise.eq(exercise));
    }
    if let Some(mistake) = f.mistake.as_deref().filter(|s| !s.is_empty()) {
        q = q.filter(attempts::mistake.eq(mistake));
    }
    if let Some(correct) = f.correct {
        q = q.filter(attempts::correct.eq(i64::from(correct)));
    }
    if let Some(source) = f.source.as_deref().filter(|s| !s.is_empty()) {
        q = q.filter(attempts::source.eq(if source == "none" { "" } else { source }));
    }
    if let Some((items, lessons)) = &r.topic_items {
        q = q.filter(
            attempts::item_id
                .eq_any(items.clone())
                .or(attempts::lesson_id.eq_any(lessons.clone()))
                .or(attempts::id.eq_any(attempt_items::table.filter(attempt_items::item_id.eq_any(items.clone())).select(attempt_items::attempt_id))),
        );
    }
    if let Some(items) = &r.grammar_items {
        let key = format!("%\"{}\"%", f.grammar.as_deref().unwrap_or_default().trim().to_lowercase());
        q = q.filter(attempts::item_id.eq_any(items.clone()).or(attempts::exercise.eq("checkinMistake").and(attempts::details.like(key))));
    }
    if let Some(ids) = &r.context_ids {
        q = q.filter(attempts::id.eq_any(ids.clone()));
    }
    q
}

fn events_query<'a>(f: &'a RawFilter, r: &'a Resolved, before: Option<&Cursor>) -> learning_events::BoxedQuery<'a, Sqlite> {
    let mut q = learning_events::table.into_boxed();
    if let Some(c) = before {
        q = match LearningEventId::parse(&c.id) {
            Ok(id) => q.filter(learning_events::at.lt(c.at).or(learning_events::at.eq(c.at).and(learning_events::id.lt(id)))),
            Err(_) => q.filter(learning_events::at.lt(c.at)),
        };
    }
    if let Some(from) = f.from_day {
        q = q.filter(learning_events::day.ge(from));
    }
    if let Some(to) = f.to_day {
        q = q.filter(learning_events::day.le(to));
    }
    if let (Some(course), Some(lessons)) = (f.course_id, &r.course_lessons) {
        q = q.filter(
            learning_events::course_id.eq(course).or(learning_events::course_id.is_null().and(learning_events::lesson_id.eq_any(lessons.clone()))),
        );
    }
    if let Some(lesson) = f.lesson_id {
        q = q.filter(learning_events::lesson_id.eq(lesson));
    }
    if let Some(item) = f.item_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        q = q.filter(learning_events::item_id.eq(item));
    }
    if let Some(items) = &r.search_items {
        let text = like(f.search.as_deref().unwrap_or_default().trim());
        q = q.filter(learning_events::item_id.eq_any(items.clone()).or(learning_events::details.like(text)));
    }
    if let Some(source) = f.source.as_deref().filter(|s| !s.is_empty()) {
        q = q.filter(learning_events::source.eq(if source == "none" { "" } else { source }));
    }
    if let Some((items, lessons)) = &r.topic_items {
        q = q.filter(learning_events::item_id.eq_any(items.clone()).or(learning_events::lesson_id.eq_any(lessons.clone())));
    }
    q
}

/// Names for the ids on a page, read once.
struct Names {
    courses: HashMap<CourseId, String>,
    lessons: HashMap<LessonId, String>,
    items: HashMap<String, (String, String)>,
    lesson_course: HashMap<LessonId, CourseId>,
}

impl Names {
    fn load(conn: &mut Conn, ids: &HashSet<String>) -> Result<Names> {
        let courses = courses::table.select((courses::id, courses::title_en)).load::<(CourseId, String)>(conn)?.into_iter().collect();
        let lessons = lessons::table.select((lessons::id, lessons::title)).load::<(LessonId, String)>(conn)?.into_iter().collect();
        let word_ids: Vec<WordId> = ids.iter().filter_map(|i| WordId::parse(i).ok()).collect();
        let sentence_ids: Vec<SentenceId> = ids.iter().filter_map(|i| SentenceId::parse(i).ok()).collect();
        let line_ids: Vec<LineId> = ids.iter().filter_map(|i| LineId::parse(i).ok()).collect();
        let mut items: HashMap<String, (String, String)> = HashMap::new();
        for chunk in word_ids.chunks(500) {
            for (id, pl, en) in words::table.filter(words::id.eq_any(chunk)).select((words::id, words::pl, words::en)).load::<(WordId, String, String)>(conn)? {
                items.insert(id.to_string(), (pl, en));
            }
        }
        for chunk in sentence_ids.chunks(500) {
            for (id, pl, en) in
                sentences::table.filter(sentences::id.eq_any(chunk)).select((sentences::id, sentences::pl, sentences::en)).load::<(SentenceId, String, String)>(conn)?
            {
                items.insert(id.to_string(), (pl, en));
            }
        }
        for chunk in line_ids.chunks(500) {
            for (id, pl, en) in conversation_lines::table
                .filter(conversation_lines::id.eq_any(chunk))
                .select((conversation_lines::id, conversation_lines::pl, conversation_lines::en))
                .load::<(LineId, String, String)>(conn)?
            {
                items.insert(id.to_string(), (pl, en));
            }
        }
        let mut lesson_course: HashMap<LessonId, CourseId> = HashMap::new();
        for (lesson, course) in crate::schema::course_lessons::table
            .order(crate::schema::course_lessons::rowid)
            .select((crate::schema::course_lessons::lesson_id, crate::schema::course_lessons::course_id))
            .load::<(LessonId, CourseId)>(conn)?
        {
            lesson_course.entry(lesson).or_insert(course);
        }
        Ok(Names { courses, lessons, items, lesson_course })
    }

    /// The course a row belongs to: as kept, or (older rows) its lesson's.
    fn course(&self, kept: Option<CourseId>, lesson: Option<LessonId>) -> (Option<CourseId>, String) {
        let course = kept.or_else(|| lesson.and_then(|l| self.lesson_course.get(&l).copied()));
        (course, course.and_then(|c| self.courses.get(&c).cloned()).unwrap_or_default())
    }

    fn item(&self, id: &str, lesson: Option<LessonId>) -> (String, String) {
        if let Some(found) = self.items.get(id) {
            return found.clone();
        }
        let lesson = LessonId::parse(id).ok().or(lesson);
        (lesson.and_then(|l| self.lessons.get(&l).cloned()).unwrap_or_default(), String::new())
    }
}

type LinkRow = (AttemptId, String, String, String, String, i64, Option<i64>);

fn links_of(conn: &mut Conn, ids: &[AttemptId]) -> Result<HashMap<AttemptId, Vec<LinkRow>>> {
    let mut out: HashMap<AttemptId, Vec<LinkRow>> = HashMap::new();
    for chunk in ids.chunks(500) {
        let rows: Vec<LinkRow> = attempt_items::table
            .filter(attempt_items::attempt_id.eq_any(chunk))
            .order((attempt_items::attempt_id, attempt_items::role, attempt_items::position))
            .select((
                attempt_items::attempt_id,
                attempt_items::item_id,
                attempt_items::item_kind,
                attempt_items::role,
                attempt_items::form,
                attempt_items::position,
                attempt_items::correct,
            ))
            .load(conn)?;
        for row in rows {
            out.entry(row.0).or_default().push(row);
        }
    }
    Ok(out)
}

/// One page of the log, newest first, after `before` (from the top without).
pub fn page(conn: &mut Conn, f: &RawFilter, before: Option<&Cursor>, limit: i64) -> Result<RawPage> {
    let limit = limit.clamp(1, PAGE_MAX);
    let r = resolve(conn, f)?;
    let answers: Vec<AttemptRow> = if r.answers {
        attempts_query(f, &r, before)
            .select(AttemptRow::as_select())
            .order((attempts::at.desc(), attempts::id.desc()))
            .limit(limit + 1)
            .load(conn)?
    } else {
        vec![]
    };
    let events: Vec<EventRow> = if r.events {
        events_query(f, &r, before)
            .select(EventRow::as_select())
            .order((learning_events::at.desc(), learning_events::id.desc()))
            .limit(limit + 1)
            .load(conn)?
    } else {
        vec![]
    };
    // Merged newest first (an event after an answer at the same moment).
    enum Either {
        A(Box<AttemptRow>),
        E(Box<EventRow>),
    }
    let mut merged: Vec<(i64, String, Either)> = answers
        .into_iter()
        .map(|a| (a.at, a.id.to_string(), Either::A(Box::new(a))))
        .chain(events.into_iter().map(|e| (e.at, e.id.to_string(), Either::E(Box::new(e)))))
        .collect();
    merged.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let more = merged.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    merged.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next = if more { merged.last().map(|(at, id, _)| Cursor { at: *at, id: id.clone() }) } else { None };

    let attempt_ids: Vec<AttemptId> = merged.iter().filter_map(|(_, _, e)| if let Either::A(a) = e { Some(a.id) } else { None }).collect();
    let links = links_of(conn, &attempt_ids)?;
    let mut ids: HashSet<String> = HashSet::new();
    for (_, _, row) in &merged {
        match row {
            Either::A(a) => {
                ids.insert(a.item_id.clone());
            }
            Either::E(e) => {
                ids.insert(e.item_id.clone());
            }
        }
    }
    for list in links.values() {
        ids.extend(list.iter().map(|l| l.1.clone()));
    }
    let names = Names::load(conn, &ids)?;
    // The annotation of the sentences on the page, for the links' grammar.
    let sentence_ids: Vec<SentenceId> = ids.iter().filter_map(|i| SentenceId::parse(i).ok()).collect();
    let mut tokens: HashMap<(String, i64), (String, String)> = HashMap::new();
    for chunk in sentence_ids.chunks(500) {
        for (s, p, c, why) in sentence_tokens::table
            .filter(sentence_tokens::sentence_id.eq_any(chunk))
            .select((sentence_tokens::sentence_id, sentence_tokens::position, sentence_tokens::gram_case, sentence_tokens::reason))
            .load::<(SentenceId, i64, String, String)>(conn)?
        {
            tokens.insert((s.to_string(), p), (c, why));
        }
    }

    let rows = merged
        .into_iter()
        .map(|(_, _, row)| match row {
            Either::A(a) => answer_row(*a, &names, &links, &tokens),
            Either::E(e) => event_row(*e, &names),
        })
        .collect();
    Ok(RawPage { rows, next })
}

fn answer_row(a: AttemptRow, names: &Names, links: &HashMap<AttemptId, Vec<LinkRow>>, tokens: &HashMap<(String, i64), (String, String)>) -> RawRow {
    let details = read_details(&a.details);
    let (course_id, course) = names.course(a.course_id, a.lesson_id);
    let (item, item_en) = names.item(&a.item_id, a.lesson_id);
    let links = links
        .get(&a.id)
        .map(|list| {
            list.iter()
                .map(|(_, item_id, item_kind, role, form, position, correct)| {
                    let (pl, en) = names.items.get(item_id).cloned().unwrap_or_default();
                    let (case, reason) = tokens.get(&(a.item_id.clone(), *position)).filter(|_| role != "target" && role != "distractor").cloned().unwrap_or_default();
                    LinkView {
                        item_id: item_id.clone(),
                        item_kind: item_kind.clone(),
                        pl,
                        en,
                        role: role.clone(),
                        role_label: if role == "target" && item_kind != "word" { "The card's own".into() } else { crate::wordlinks::role_label(role).into() },
                        form: form.clone(),
                        position: *position,
                        correct: correct.map(|c| c != 0),
                        case,
                        reason,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    RawRow {
        kind: "answer".into(),
        id: a.id.to_string(),
        at: a.at,
        day: a.day,
        device: a.device.to_string(),
        source_label: contexts::source_label(&a.source).into(),
        source: a.source,
        course_id,
        course,
        lesson_id: a.lesson_id,
        lesson: a.lesson_id.and_then(|l| names.lessons.get(&l).cloned()).unwrap_or_default(),
        item_kind: a.item_kind,
        item_id: a.item_id,
        item,
        item_en,
        answer: Some(AnswerView {
            exercise_label: analytics::exercise_label(&a.exercise),
            exercise: a.exercise,
            direction: a.direction,
            prompt: a.prompt,
            card_key: a.card_key,
            position: a.position,
            run: a.run,
            correct: a.correct != 0,
            given: a.given,
            expected: a.expected,
            mistake_label: crate::mistakes::label(&a.mistake).into(),
            mistake: a.mistake,
            detail: a.detail,
            contrast: a.contrast,
            ms_first: a.ms_first,
            ms_answer: a.ms_answer,
            tries: a.tries,
            hints: a.hints,
            plays: a.plays,
            slow_plays: a.slow_plays,
            reveals: a.reveals,
            changed: a.changed != 0,
            first_pick: details.first_pick,
            options: details.options,
            words: details.words,
            shown: details.shown,
            checkin_id: a.checkin_id,
            verdict: details.verdict,
            grammar: details.grammar,
            note: details.note,
            links,
        }),
        event: String::new(),
        event_label: String::new(),
        event_details: None,
    }
}

fn event_row(e: EventRow, names: &Names) -> RawRow {
    let (course_id, course) = names.course(e.course_id, e.lesson_id);
    let (item, item_en) = if e.item_id.is_empty() { (String::new(), String::new()) } else { names.item(&e.item_id, e.lesson_id) };
    RawRow {
        kind: "event".into(),
        id: e.id.to_string(),
        at: e.at,
        day: e.day,
        device: e.device.to_string(),
        source_label: contexts::source_label(&e.source).into(),
        source: e.source,
        course_id,
        course,
        lesson_id: e.lesson_id,
        lesson: e.lesson_id.and_then(|l| names.lessons.get(&l).cloned()).unwrap_or_default(),
        item_kind: e.item_kind,
        item_id: e.item_id,
        item,
        item_en,
        answer: None,
        event_label: analytics::event_label(&e.kind),
        event: e.kind,
        event_details: Some(serde_json::from_str(&e.details).unwrap_or_default()),
    }
}

/// Every row a filter takes, newest first (an export), up to [`EXPORT_MAX`].
pub fn all_rows(conn: &mut Conn, f: &RawFilter) -> Result<Vec<RawRow>> {
    let mut out = Vec::new();
    let mut cursor: Option<Cursor> = None;
    loop {
        let page = page(conn, f, cursor.as_ref(), PAGE_MAX)?;
        out.extend(page.rows);
        match page.next {
            Some(next) if out.len() < EXPORT_MAX => cursor = Some(next),
            _ => break,
        }
    }
    out.truncate(EXPORT_MAX);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// The CSV's columns, in order.
pub const CSV_COLUMNS: &[&str] = &[
    "kind", "id", "date", "time", "at_ms", "device", "source", "course_id", "course", "lesson_id", "lesson", "item_kind", "item_id", "item",
    "item_en", "exercise", "direction", "card_key", "position", "of_cards", "prompt", "hint", "options", "tiles", "given", "expected", "correct",
    "mistake", "detail", "contrast", "ms_first", "ms_answer", "tries", "hints", "plays", "slow_plays", "reveals", "changed", "first_pick",
    "word_results", "linked_words", "checkin_id", "verdict", "xp", "grammar", "note", "event", "event_details",
];

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// The time of day of a moment, on the learner's clock ("14:05").
fn clock(at: i64, offset_minutes: i64) -> String {
    let secs = (at / 1000 + offset_minutes * 60).rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

fn csv_row(row: &RawRow, offset_minutes: i64) -> Vec<String> {
    let (y, m, d) = crate::commands::civil_date(row.day);
    let opt = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_default();
    let a = row.answer.as_ref();
    let shown = a.and_then(|a| a.shown.as_ref());
    let text = |f: &dyn Fn(&AnswerView) -> String| a.map(f).unwrap_or_default();
    vec![
        row.kind.clone(),
        row.id.clone(),
        format!("{y:04}-{m:02}-{d:02}"),
        clock(row.at, offset_minutes),
        row.at.to_string(),
        row.device.clone(),
        row.source.clone(),
        row.course_id.map(|c| c.to_string()).unwrap_or_default(),
        row.course.clone(),
        row.lesson_id.map(|l| l.to_string()).unwrap_or_default(),
        row.lesson.clone(),
        row.item_kind.clone(),
        row.item_id.clone(),
        row.item.clone(),
        row.item_en.clone(),
        text(&|a| a.exercise.clone()),
        text(&|a| a.direction.clone()),
        text(&|a| a.card_key.clone()),
        opt(a.and_then(|a| a.position)),
        opt(shown.and_then(|s| s.of_cards)),
        text(&|a| a.prompt.clone()),
        shown.map(|s| s.hint.clone()).unwrap_or_default(),
        text(&|a| a.options.join(" | ")),
        shown.map(|s| s.tokens.join(" | ")).unwrap_or_default(),
        text(&|a| a.given.clone()),
        text(&|a| a.expected.clone()),
        text(&|a| if a.correct { "1".into() } else { "0".into() }),
        text(&|a| a.mistake.clone()),
        text(&|a| a.detail.clone()),
        text(&|a| a.contrast.clone()),
        opt(a.and_then(|a| a.ms_first)),
        opt(a.and_then(|a| a.ms_answer)),
        text(&|a| a.tries.to_string()),
        text(&|a| a.hints.to_string()),
        text(&|a| a.plays.to_string()),
        text(&|a| a.slow_plays.to_string()),
        text(&|a| a.reveals.to_string()),
        text(&|a| if a.changed { "1".into() } else { "0".into() }),
        text(&|a| a.first_pick.clone().unwrap_or_default()),
        text(&|a| a.words.iter().map(|w| format!("{}:{}:{}", w.s, w.e, w.g)).collect::<Vec<_>>().join(" | ")),
        text(&|a| {
            a.links
                .iter()
                .map(|l| {
                    let result = match l.correct {
                        Some(true) => "right",
                        Some(false) => "wrong",
                        None => "",
                    };
                    format!("{}={}@{}#{}:{}{}", l.pl, l.form, l.role, l.position, result, if l.reason.is_empty() { String::new() } else { format!(":{}", l.reason) })
                })
                .collect::<Vec<_>>()
                .join(" | ")
        }),
        text(&|a| a.checkin_id.map(|c| c.to_string()).unwrap_or_default()),
        text(&|a| a.verdict.as_ref().map(|v| v.result.clone()).unwrap_or_default()),
        text(&|a| a.verdict.as_ref().map(|v| v.xp.to_string()).unwrap_or_default()),
        text(&|a| a.grammar.join(" | ")),
        text(&|a| a.note.clone()),
        row.event.clone(),
        row.event_details.as_ref().map(|d| serde_json::to_string(d).unwrap_or_default()).unwrap_or_default(),
    ]
}

/// The rows as a file: "csv" (one row per line, a header first; the words
/// as `dictionary=form@role#position:result:reason`) or "json" (every
/// field, as the raw log has it).
pub fn export_text(rows: &[RawRow], format: &str, offset_minutes: i64) -> Result<String> {
    match format {
        "json" => Ok(serde_json::to_string_pretty(rows)?),
        "csv" => {
            let mut out = CSV_COLUMNS.join(",");
            out.push('\n');
            for row in rows {
                out.push_str(&csv_row(row, offset_minutes).iter().map(|v| csv_field(v)).collect::<Vec<_>>().join(","));
                out.push('\n');
            }
            Ok(out)
        }
        other => Err(Error::msg(format!("Unknown export format '{other}': csv or json."))),
    }
}

#[tauri::command]
pub async fn learning_raw_log(db: State<'_, Db>, filter: RawFilter, before: Option<Cursor>, limit: Option<i64>) -> Result<RawPage> {
    page(&mut db.lock(), &filter, before.as_ref(), limit.unwrap_or(50))
}

/// Saves the filtered log where the learner picks, as CSV or JSON (the
/// same save dialog as the data export). Returns where it went, or `None`
/// if the dialog was cancelled.
#[tauri::command]
pub async fn learning_log_export(app: tauri::AppHandle, db: State<'_, Db>, filter: RawFilter, format: String) -> Result<Option<String>> {
    use std::io::Write;
    use tauri_plugin_dialog::DialogExt;
    use tauri_plugin_fs::FsExt;

    let (text, name) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let rows = all_rows(conn, &filter)?;
        let offset = db::get_setting_i64(conn, "tz_offset_minutes", 0);
        let (y, m, d) = crate::commands::civil_date(db::today(conn));
        (export_text(&rows, &format, offset)?, format!("polski-learning-log-{y:04}-{m:02}-{d:02}.{format}"))
    };
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Export your learning log")
        .set_file_name(&name)
        .add_filter(if format == "csv" { "CSV" } else { "JSON" }, &[format.as_str()])
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(target) = receiver.await.ok().flatten() else {
        return Ok(None);
    };
    let shown = target.to_string();
    let mut options = tauri_plugin_fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    let mut out = app.fs().open(target, options).map_err(|e| Error::msg(format!("Could not write the export: {e}")))?;
    out.write_all(text.as_bytes()).map_err(|e| Error::msg(format!("Could not write the export: {e}")))?;
    Ok(Some(shown))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_fields_are_quoted_when_they_must_be() {
        assert_eq!(csv_field("kot"), "kot");
        assert_eq!(csv_field("kot, pies"), "\"kot, pies\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(clock(0, 60), "01:00");
        assert_eq!(clock(86_399_000, 0), "23:59");
    }
}
