//! Statistics in context: how a word, sentence, grammar structure or topic
//! goes by the course it was practised from, the role a word had (on its
//! own, in a sentence, a dialogue, a story; a wrong option), its grammatical
//! context in a sentence (the case and the reason the tutor annotated, the
//! form it had), the topic around it, the exercise and how it was reached.
//! "kawa: right as the subject, wrong after z."
//!
//! It reads the learning log's answers and the words each involved
//! (wordlinks.rs: `attempt_items`), the sentences' annotations
//! (grammar.rs: `sentence_tokens`) and the topics (topics.rs). One
//! implementation serves the word page, the Structures and Topics pages,
//! the raw log's drill-downs (rawlog.rs) and the tutor (tutor_analytics.rs).

use std::collections::{BTreeMap, HashMap, HashSet};

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::analytics::{self, accuracy, trickiness, AttemptView, Filter, Row, Titles};
use crate::convert::count;
use crate::db::{Conn, Db};
use crate::error::{Error, Result};
#[allow(unused_imports)]
use crate::ids::*;
use crate::schema::{attempt_items, attempts, course_lessons, courses, sentence_tokens};
use crate::topics::{self, TopicInfo};

/// Every way answers can be grouped.
pub const GROUPS: &[(&str, &str)] = &[
    ("course", "the course it was practised from"),
    ("role", "on its own, in a sentence, a dialogue, a story, a song, or as a wrong option"),
    ("case", "the case it had in the sentence (as annotated)"),
    ("reason", "why it had that form (as annotated: after z, as the object…)"),
    ("form", "the form it had"),
    ("topic", "the topics of the sentence or lesson around it"),
    ("exercise", "the kind of exercise"),
    ("source", "how the practice was reached (course page, daily five, game…)"),
];

/// What the statistics are about: one word or sentence, one grammar
/// structure, one topic, or (none given) every answer.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    #[serde(default)]
    pub item_id: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
}

/// One time something appeared in an answer.
#[derive(Debug, Clone)]
pub struct Appearance {
    pub attempt: AttemptId,
    pub at: i64,
    pub exercise: String,
    pub source: String,
    pub course: Option<CourseId>,
    pub lesson: Option<LessonId>,
    /// What the card was about (the answer's item).
    pub carrier: String,
    /// What appeared (the word, for a word's links).
    pub item: String,
    pub role: String,
    pub form: String,
    pub position: i64,
    pub right: bool,
}

/// A group and how it went.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextGroup {
    pub key: String,
    pub label: String,
    pub attempts: i64,
    pub correct: i64,
    pub accuracy: f64,
    pub last_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextStats {
    pub subject: String,
    pub group_by: String,
    pub attempts: i64,
    pub groups: Vec<ContextGroup>,
}

/// What the groupings need, loaded once.
struct Lookups {
    course_titles: HashMap<CourseId, String>,
    lesson_course: HashMap<LessonId, CourseId>,
    tokens: HashMap<(String, i64), (String, String)>,
    topics: HashMap<String, Vec<TopicId>>,
}

impl Lookups {
    fn load(conn: &mut Conn) -> Result<Lookups> {
        let course_titles = courses::table.select((courses::id, courses::title_en)).load::<(CourseId, String)>(conn)?.into_iter().collect();
        let mut lesson_course: HashMap<LessonId, CourseId> = HashMap::new();
        for (lesson, course) in course_lessons::table
            .order(course_lessons::rowid)
            .select((course_lessons::lesson_id, course_lessons::course_id))
            .load::<(LessonId, CourseId)>(conn)?
        {
            lesson_course.entry(lesson).or_insert(course);
        }
        let tokens = sentence_tokens::table
            .select((sentence_tokens::sentence_id, sentence_tokens::position, sentence_tokens::gram_case, sentence_tokens::reason))
            .load::<(SentenceId, i64, String, String)>(conn)?
            .into_iter()
            .map(|(s, p, c, r)| ((s.to_string(), p), (c, r)))
            .collect();
        Ok(Lookups { course_titles, lesson_course, tokens, topics: topics::all_links(conn)? })
    }

    /// The topics around an appearance: its card's item's and its lesson's.
    fn topics_of(&self, a: &Appearance) -> Vec<TopicId> {
        let mut out: Vec<TopicId> = self.topics.get(&a.carrier).cloned().unwrap_or_default();
        if let Some(lesson) = a.lesson {
            for t in self.topics.get(&lesson.to_string()).into_iter().flatten() {
                if !out.contains(t) {
                    out.push(*t);
                }
            }
        }
        out
    }

    /// The groups an appearance falls in (a topic grouping can give several).
    fn keys(&self, a: &Appearance, group_by: &str) -> Vec<(String, String)> {
        let token = || self.tokens.get(&(a.carrier.clone(), a.position));
        let alone = || match a.role.as_str() {
            "distractor" => vec![("distractor".to_string(), "As a wrong option".to_string())],
            _ => vec![("alone".to_string(), "On its own".to_string())],
        };
        let unknown = || vec![("unannotated".to_string(), "Sentence not annotated".to_string())];
        match group_by {
            "course" => {
                let course = a.course.or_else(|| a.lesson.and_then(|l| self.lesson_course.get(&l).copied()));
                match course {
                    Some(c) => vec![(c.to_string(), self.course_titles.get(&c).cloned().unwrap_or_else(|| "A course since deleted".into()))],
                    None => vec![("none".into(), "No course (games)".into())],
                }
            }
            "role" => vec![(a.role.clone(), crate::wordlinks::role_label(&a.role).into())],
            "case" | "reason" if a.role == "target" || a.role == "distractor" => alone(),
            "case" => match token() {
                Some((case, _)) if !case.is_empty() => vec![(case.clone(), crate::grammar::value_label("case", case))],
                Some(_) => vec![("none".into(), "No case".into())],
                None => unknown(),
            },
            "reason" => match token() {
                Some((_, why)) if !why.is_empty() => vec![(why.clone(), crate::grammar::value_label("reason", why))],
                Some(_) => vec![("none".into(), "No reason given".into())],
                None => unknown(),
            },
            "form" => {
                let form = a.form.to_lowercase();
                match a.role.as_str() {
                    "target" => vec![(form.clone(), format!("{form} (on its own)"))],
                    "distractor" => vec![("distractor".into(), "As a wrong option".into())],
                    _ => vec![(form.clone(), form)],
                }
            }
            "topic" => {
                let ts = self.topics_of(a);
                if ts.is_empty() {
                    vec![("none".into(), "No topic".into())]
                } else {
                    ts.into_iter().filter_map(topics::by_id).map(|t| (t.key.to_string(), t.name.to_string())).collect()
                }
            }
            "exercise" => vec![(a.exercise.clone(), analytics::exercise_label(&a.exercise))],
            "source" => vec![(a.source.clone(), source_label(&a.source).into())],
            _ => vec![],
        }
    }
}

/// How a source reads on the pages.
pub fn source_label(source: &str) -> &'static str {
    match source {
        "course" => "From its course",
        "daily" => "Today's lessons",
        "other" => "Other lessons",
        "game" => "Quick games",
        "tutor" => "From the tutor",
        "music" => "From Music",
        "link" => "From a link",
        "search" => "From Search",
        "insights" => "From Progress",
        _ => "Not recorded",
    }
}

type LinkRow = (AttemptId, i64, String, String, Option<CourseId>, Option<LessonId>, String, i64, String, String, String, i64, Option<i64>);

/// The appearances of some items (by their links), in a filter's span.
pub fn appearances_of(conn: &mut Conn, items: &[String], f: &Filter) -> Result<Vec<Appearance>> {
    let mut query = attempt_items::table
        .inner_join(attempts::table)
        .filter(attempt_items::item_id.eq_any(items))
        .select((
            attempts::id,
            attempts::at,
            attempts::exercise,
            attempts::source,
            attempts::course_id,
            attempts::lesson_id,
            attempts::item_id,
            attempts::correct,
            attempt_items::item_id,
            attempt_items::role,
            attempt_items::form,
            attempt_items::position,
            attempt_items::correct,
        ))
        .order(attempts::at.asc())
        .into_boxed();
    if let Some(from) = f.from_day {
        query = query.filter(attempts::day.ge(from));
    }
    if let Some(to) = f.to_day {
        query = query.filter(attempts::day.le(to));
    }
    if let Some(course) = f.course_id {
        let lessons = analytics::course_lessons_of(conn, course)?;
        query = query.filter(attempts::course_id.eq(course).or(attempts::course_id.is_null().and(attempts::lesson_id.eq_any(lessons))));
    }
    let rows: Vec<LinkRow> = query.limit(analytics::ROW_LIMIT).load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(attempt, at, exercise, source, course, lesson, carrier, correct, item, role, form, position, link)| Appearance {
            attempt,
            at,
            exercise,
            source,
            course,
            lesson,
            carrier,
            item,
            role,
            form,
            position,
            right: link.map_or(correct != 0, |c| c != 0),
        })
        .collect())
}

/// Answers as appearances of what they were about (no links needed).
fn answers_as_appearances(rows: &[Row]) -> Vec<Appearance> {
    rows.iter()
        .map(|r| Appearance {
            attempt: r.id,
            at: r.at,
            exercise: r.exercise.clone(),
            source: r.source.clone(),
            course: r.course_id,
            lesson: r.lesson_id,
            carrier: r.item_id.clone(),
            item: r.item_id.clone(),
            role: "target".into(),
            form: String::new(),
            position: 0,
            right: r.correct != 0,
        })
        .collect()
}

/// The ids a grammar tag covers: the items tagged with it.
fn tag_items(conn: &mut Conn, key: &str) -> Result<HashSet<String>> {
    let tag = crate::grammar::by_key(key).ok_or_else(|| Error::msg(format!("Unknown grammar tag '{key}'.")))?;
    let id = crate::grammar::id_of(tag.key);
    Ok(crate::grammar::all_links(conn)?.into_iter().filter(|(_, tags)| tags.contains(&id)).map(|(item, _)| item).collect())
}

/// The topic ids a key covers: itself, and a domain's topics too.
pub fn topic_ids(key: &str) -> Result<Vec<TopicId>> {
    let topic = topics::by_key(key).ok_or_else(|| Error::msg(format!("Unknown topic '{key}'.")))?;
    Ok(if topic.key == topic.domain {
        topics::TOPICS.iter().filter(|t| t.domain == topic.key).map(|t| topics::id_of(t.key)).collect()
    } else {
        vec![topics::id_of(topic.key)]
    })
}

/// What a subject covers, as appearances.
fn subject_appearances(conn: &mut Conn, subject: &Subject, f: &Filter) -> Result<(String, Vec<Appearance>)> {
    if let Some(item) = subject.item_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !matches!(crate::ids::kind_of(item), Some("word" | "sentence" | "dialogue line")) {
            return Err(Error::msg(format!("{item}: give a word (wd_…), sentence (sn_…) or dialogue line (dl_…).")));
        }
        return Ok((item.to_string(), appearances_of(conn, &[item.to_string()], f)?));
    }
    let rows = analytics::load(conn, f)?;
    if let Some(key) = subject.tag.as_deref().filter(|s| !s.is_empty()) {
        let items = tag_items(conn, key)?;
        let key = crate::grammar::by_key(key).map(|t| t.key).unwrap_or_default();
        let mine: Vec<Row> = rows
            .into_iter()
            .filter(|r| items.contains(&r.item_id) || (r.exercise == "checkinMistake" && crate::learnlog::read_details(&r.details).grammar.iter().any(|g| g == key)))
            .collect();
        return Ok((key.to_string(), answers_as_appearances(&mine)));
    }
    if let Some(key) = subject.topic.as_deref().filter(|s| !s.is_empty()) {
        let wanted = topic_ids(key)?;
        let links = topics::all_links(conn)?;
        let has = |id: &str| links.get(id).is_some_and(|ts| ts.iter().any(|t| wanted.contains(t)));
        let mine: Vec<Row> = rows.into_iter().filter(|r| has(&r.item_id) || r.lesson_id.is_some_and(|l| has(&l.to_string()))).collect();
        return Ok((key.to_string(), answers_as_appearances(&mine)));
    }
    Ok(("all".into(), answers_as_appearances(&rows)))
}

fn check_group(group_by: &str) -> Result<()> {
    if GROUPS.iter().any(|(g, _)| *g == group_by) {
        Ok(())
    } else {
        Err(Error::msg(format!("Unknown grouping '{group_by}': one of {}.", GROUPS.iter().map(|(g, _)| *g).collect::<Vec<_>>().join(", "))))
    }
}

fn group(apps: &[Appearance], lookups: &Lookups, group_by: &str) -> Vec<ContextGroup> {
    let mut by: BTreeMap<String, (String, i64, i64, i64)> = BTreeMap::new();
    for a in apps {
        for (key, label) in lookups.keys(a, group_by) {
            let e = by.entry(key).or_insert((label, 0, 0, 0));
            e.1 += 1;
            e.2 += i64::from(a.right);
            e.3 = e.3.max(a.at);
        }
    }
    let mut out: Vec<ContextGroup> = by
        .into_iter()
        .map(|(key, (label, attempts, correct, last_at))| ContextGroup { key, label, attempts, correct, accuracy: accuracy(correct, attempts), last_at })
        .collect();
    out.sort_by(|a, b| b.attempts.cmp(&a.attempts).then(a.label.cmp(&b.label)));
    out
}

/// A subject's answers grouped one way.
pub fn stats(conn: &mut Conn, subject: &Subject, group_by: &str, f: &Filter) -> Result<ContextStats> {
    check_group(group_by)?;
    let (label, apps) = subject_appearances(conn, subject, f)?;
    let lookups = Lookups::load(conn)?;
    Ok(ContextStats { subject: label, group_by: group_by.into(), attempts: count(apps.len()), groups: group(&apps, &lookups, group_by) })
}

/// The answers behind one group (the raw log's drill-down).
pub fn attempt_ids(conn: &mut Conn, subject: &Subject, group_by: &str, key: &str, f: &Filter) -> Result<Vec<AttemptId>> {
    check_group(group_by)?;
    let (_, apps) = subject_appearances(conn, subject, f)?;
    let lookups = Lookups::load(conn)?;
    let mut ids: Vec<AttemptId> = apps.iter().filter(|a| lookups.keys(a, group_by).iter().any(|(k, _)| k == key)).map(|a| a.attempt).collect();
    ids.dedup();
    Ok(ids)
}

// ---------------------------------------------------------------------------
// A word in its contexts (the word page)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WordContexts {
    /// Every appearance, on its own or elsewhere.
    pub appearances: i64,
    pub by_course: Vec<ContextGroup>,
    pub by_role: Vec<ContextGroup>,
    pub by_case: Vec<ContextGroup>,
    pub by_reason: Vec<ContextGroup>,
    pub by_form: Vec<ContextGroup>,
    pub by_topic: Vec<ContextGroup>,
    pub by_exercise: Vec<ContextGroup>,
    pub by_source: Vec<ContextGroup>,
}

/// A word's (or sentence's) every appearance, grouped every way.
pub fn item_contexts(conn: &mut Conn, item_id: &str, f: &Filter) -> Result<WordContexts> {
    let apps = appearances_of(conn, &[item_id.to_string()], f)?;
    let lookups = Lookups::load(conn)?;
    let g = |by: &str| group(&apps, &lookups, by);
    Ok(WordContexts {
        appearances: count(apps.len()),
        by_course: g("course"),
        by_role: g("role"),
        by_case: g("case"),
        by_reason: g("reason"),
        by_form: g("form"),
        by_topic: g("topic"),
        by_exercise: g("exercise"),
        by_source: g("source"),
    })
}

// ---------------------------------------------------------------------------
// Words within a structure (the Structures page)
// ---------------------------------------------------------------------------

/// A word as it went in one context.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WordInContext {
    pub word_id: WordId,
    pub pl: String,
    pub en: String,
    /// The forms it had there ("kawą", "kawy").
    pub forms: Vec<String>,
    pub attempts: i64,
    pub correct: i64,
    pub accuracy: f64,
}

fn words_from(apps: &[&Appearance], titles: &Titles) -> Vec<WordInContext> {
    let mut by: HashMap<String, (Vec<String>, i64, i64)> = HashMap::new();
    for a in apps {
        let e = by.entry(a.item.clone()).or_default();
        let form = a.form.to_lowercase();
        if !form.is_empty() && !e.0.contains(&form) {
            e.0.push(form);
        }
        e.1 += 1;
        e.2 += i64::from(a.right);
    }
    let mut out: Vec<WordInContext> = by
        .into_iter()
        .filter_map(|(id, (forms, attempts, correct))| {
            let w = titles.words.get(&id)?;
            Some(WordInContext {
                word_id: WordId::parse(&id).ok()?,
                pl: w.pl.clone(),
                en: w.en.clone(),
                forms,
                attempts,
                correct,
                accuracy: accuracy(correct, attempts),
            })
        })
        .collect();
    // The most missed first.
    out.sort_by(|a, b| (b.attempts - b.correct).cmp(&(a.attempts - a.correct)).then(b.attempts.cmp(&a.attempts)).then(a.pl.cmp(&b.pl)));
    out.truncate(60);
    out
}

/// The words met in a structure: in annotated sentences where the token's
/// reason or case is this tag ("kawą" after z for z-instrumental), and the
/// words tagged with it, each with how it went there.
pub fn structure_words(conn: &mut Conn, key: &str, f: &Filter) -> Result<Vec<WordInContext>> {
    let tag = crate::grammar::by_key(key).ok_or_else(|| Error::msg(format!("Unknown grammar tag '{key}'.")))?;
    let places: Vec<(SentenceId, i64)> = sentence_tokens::table
        .filter(sentence_tokens::reason.eq(tag.key).or(sentence_tokens::gram_case.eq(tag.key)))
        .select((sentence_tokens::sentence_id, sentence_tokens::position))
        .load(conn)?;
    let places: HashSet<(String, i64)> = places.into_iter().map(|(s, p)| (s.to_string(), p)).collect();
    let tagged = tag_items(conn, tag.key)?;
    let mut words: HashSet<String> = tagged.iter().filter(|i| i.starts_with("wd_")).cloned().collect();
    let sentences: Vec<String> = places.iter().map(|(s, _)| s.clone()).collect::<HashSet<_>>().into_iter().collect();
    // The words at those places: from the links of answers on those sentences.
    if !sentences.is_empty() {
        let linked: Vec<String> = attempt_items::table
            .inner_join(attempts::table)
            .filter(attempts::item_id.eq_any(&sentences))
            .filter(attempt_items::item_kind.eq("word"))
            .select(attempt_items::item_id)
            .distinct()
            .load(conn)?;
        words.extend(linked);
    }
    let words: Vec<String> = words.into_iter().collect();
    let apps = appearances_of(conn, &words, f)?;
    let titles = Titles::load(conn)?;
    let mine: Vec<&Appearance> = apps
        .iter()
        .filter(|a| (a.role == "target" && tagged.contains(&a.item)) || places.contains(&(a.carrier.clone(), a.position)))
        .collect();
    Ok(words_from(&mine, &titles))
}

// ---------------------------------------------------------------------------
// Topics (What you've met → Topics)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopicRow {
    pub topic: TopicInfo,
    /// Words, sentences and lessons with it (a domain: with any of its topics).
    pub items: i64,
    pub attempts: i64,
    pub correct: i64,
    pub accuracy: Option<f64>,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopicDomain {
    pub domain: TopicRow,
    pub topics: Vec<TopicRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopicsPage {
    pub domains: Vec<TopicDomain>,
    /// Words, sentences and lessons in open courses without a topic yet.
    pub untagged: i64,
}

/// Every answer's topics (with their domains), oldest first.
fn answer_topics(rows: &[Row], links: &HashMap<String, Vec<TopicId>>) -> Vec<Vec<TopicId>> {
    rows.iter()
        .map(|r| {
            let mut ts: Vec<TopicId> = Vec::new();
            let own = links.get(&r.item_id).into_iter().flatten();
            let lesson = r.lesson_id.and_then(|l| links.get(&l.to_string())).into_iter().flatten();
            for t in own.chain(lesson) {
                for x in topics::with_domain(*t) {
                    if !ts.contains(&x) {
                        ts.push(x);
                    }
                }
            }
            ts
        })
        .collect()
}

/// Every topic's row, the period's answers, and each answer's topics.
type TopicRows = (HashMap<TopicId, TopicRow>, Vec<Row>, Vec<Vec<TopicId>>);

fn topic_rows(conn: &mut Conn, f: &Filter) -> Result<TopicRows> {
    let rows = analytics::load(conn, f)?;
    let links = topics::all_links(conn)?;
    let mut items: HashMap<TopicId, i64> = HashMap::new();
    for ts in links.values() {
        let mut seen: HashSet<TopicId> = HashSet::new();
        for t in ts {
            seen.extend(topics::with_domain(*t));
        }
        for t in seen {
            *items.entry(t).or_default() += 1;
        }
    }
    let per_answer = answer_topics(&rows, &links);
    let mut results: HashMap<TopicId, Vec<bool>> = HashMap::new();
    for (row, ts) in rows.iter().zip(&per_answer) {
        for t in ts {
            results.entry(*t).or_default().push(row.correct != 0);
        }
    }
    let mut out = HashMap::new();
    for t in topics::TOPICS {
        let id = topics::id_of(t.key);
        let r = results.get(&id).cloned().unwrap_or_default();
        let correct = count(r.iter().filter(|x| **x).count());
        out.insert(
            id,
            TopicRow {
                topic: t.into(),
                items: items.get(&id).copied().unwrap_or(0),
                attempts: count(r.len()),
                correct,
                accuracy: (!r.is_empty()).then(|| accuracy(correct, count(r.len()))),
                score: trickiness(&r),
            },
        );
    }
    Ok((out, rows, per_answer))
}

/// Every topic met (items or answers), by domain, trickiest first within.
pub fn topics_page(conn: &mut Conn, f: &Filter) -> Result<TopicsPage> {
    let (mut rows, _, _) = topic_rows(conn, f)?;
    let met = |r: &TopicRow| r.items > 0 || r.attempts > 0;
    let mut domains = Vec::new();
    for d in topics::TOPICS.iter().filter(|t| t.key == t.domain) {
        let Some(domain) = rows.remove(&topics::id_of(d.key)) else { continue };
        if !met(&domain) {
            continue;
        }
        let mut children: Vec<TopicRow> = topics::TOPICS
            .iter()
            .filter(|t| t.domain == d.key && t.key != t.domain)
            .filter_map(|t| rows.remove(&topics::id_of(t.key)))
            .filter(met)
            .collect();
        children.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.attempts.cmp(&a.attempts)).then(a.topic.name.cmp(&b.topic.name)));
        domains.push(TopicDomain { domain, topics: children });
    }
    domains.sort_by(|a, b| b.domain.attempts.cmp(&a.domain.attempts).then(b.domain.items.cmp(&a.domain.items)));
    Ok(TopicsPage { domains, untagged: topics::untagged_count(conn)? })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopicDetail {
    pub row: TopicRow,
    /// Its topics (a domain's).
    pub topics: Vec<TopicRow>,
    /// Each word as it went in this topic: on its own when the word is
    /// about it, and wherever it appeared in the topic's sentences and lessons.
    pub words: Vec<WordInContext>,
    pub by_exercise: Vec<ContextGroup>,
    pub by_course: Vec<ContextGroup>,
    /// Misses in it, newest first.
    pub mistakes: Vec<AttemptView>,
}

pub fn topic_detail(conn: &mut Conn, key: &str, f: &Filter) -> Result<TopicDetail> {
    let topic = topics::by_key(key).ok_or_else(|| Error::msg(format!("Unknown topic '{key}'.")))?;
    let id = topics::id_of(topic.key);
    let wanted = topic_ids(topic.key)?;
    let (mut all, rows, per_answer) = topic_rows(conn, f)?;
    let row = all.remove(&id).ok_or_else(|| Error::msg("No such topic."))?;
    let children: Vec<TopicRow> = if topic.key == topic.domain {
        topics::TOPICS.iter().filter(|t| t.domain == topic.key && t.key != t.domain).filter_map(|t| all.remove(&topics::id_of(t.key))).filter(|r| r.items > 0 || r.attempts > 0).collect()
    } else {
        vec![]
    };
    let mine: Vec<Row> = rows.iter().zip(&per_answer).filter(|(_, ts)| ts.contains(&id)).map(|(r, _)| r.clone()).collect();
    let lookups = Lookups::load(conn)?;
    let apps = answers_as_appearances(&mine);
    // Words in the topic: tagged with it, or appearing where it is the topic.
    let in_topic = |id: &str| lookups.topics.get(id).is_some_and(|ts| ts.iter().any(|t| wanted.contains(t)));
    let word_ids: Vec<String> = attempt_items::table
        .filter(attempt_items::item_kind.eq("word"))
        .filter(attempt_items::attempt_id.eq_any(mine.iter().map(|r| r.id).collect::<Vec<_>>()))
        .select(attempt_items::item_id)
        .distinct()
        .load::<String>(conn)?
        .into_iter()
        .chain(lookups.topics.keys().filter(|k| k.starts_with("wd_") && in_topic(k)).cloned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let word_apps = appearances_of(conn, &word_ids, f)?;
    let titles = Titles::load(conn)?;
    let words_here: Vec<&Appearance> = word_apps
        .iter()
        .filter(|a| (a.role == "target" && in_topic(&a.item)) || in_topic(&a.carrier) || a.lesson.is_some_and(|l| in_topic(&l.to_string())))
        .collect();
    Ok(TopicDetail {
        row,
        topics: children,
        words: words_from(&words_here, &titles),
        by_exercise: group(&apps, &lookups, "exercise"),
        by_course: group(&apps, &lookups, "course"),
        mistakes: mine.iter().rev().filter(|r| r.correct == 0).take(30).map(|r| analytics::view(r, &titles, true)).collect(),
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn context_stats(db: State<'_, Db>, subject: Subject, group_by: String, filter: Filter) -> Result<ContextStats> {
    stats(&mut db.lock(), &subject, &group_by, &filter)
}

#[tauri::command]
pub async fn item_contexts_cmd(db: State<'_, Db>, item_id: String, filter: Filter) -> Result<WordContexts> {
    item_contexts(&mut db.lock(), &item_id, &filter)
}

#[tauri::command]
pub async fn met_topics(db: State<'_, Db>, filter: Filter) -> Result<TopicsPage> {
    topics_page(&mut db.lock(), &filter)
}

#[tauri::command]
pub async fn met_topic(db: State<'_, Db>, key: String, filter: Filter) -> Result<TopicDetail> {
    topic_detail(&mut db.lock(), &key, &filter)
}
