//! The tutor's view of the learning log: focused tools over the same
//! queries the Progress pages use (analytics.rs), each returning compact
//! JSON with limits. They describe what they return and nothing more: the
//! tutor decides from the conversation whether and when to look.
//!
//! Also the grammar tagging tools: tag existing words and sentences, and
//! list the ones still untagged.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent::{tool, Tool};
use crate::analytics::{self, Filter};
use crate::db::Conn;
use crate::error::{Error, Result};
#[allow(unused_imports)]
use crate::ids::*;

const PERIODS: &[&str] = &["week", "month", "quarter", "year", "all"];
const COURSE: &str = "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$";
const DATE: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}$";

fn period_prop() -> Value {
    json!({ "type": "string", "enum": PERIODS, "description": "Up to today: the last 7, 30, 91 or 365 days, or everything (default month)" })
}

fn course_prop() -> Value {
    json!({ "type": "string", "pattern": COURSE, "description": "Only answers in this course's lessons" })
}

pub fn tools() -> Vec<Tool> {
    let classes: Vec<&str> = crate::mistakes::CLASSES.iter().map(|(c, _)| *c).collect();
    vec![
        tool(
            "get_learning_overview",
            "Looking at how it's going…",
            "Headline numbers from the learner's answer log over a period: answers, accuracy, days active, \
             minutes, mistakes by class, accuracy per day, the trend against the period before, the \
             strongest and weakest structures, sounds and exercise kinds, and how much is met and untagged.",
            json!({ "type": "object", "properties": { "period": period_prop(), "course_id": course_prop() } }),
        ),
        tool(
            "find_hard_items",
            "Looking at what's hard…",
            "The words, sentences, grammar structures or sounds the learner misses most, ranked (recent \
             misses weigh more), with answer counts, accuracy, mistake classes and the latest misses \
             (given vs expected).",
            json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": analytics::HARD_KINDS },
                    "period": period_prop(),
                    "course_id": course_prop(),
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Default 10" }
                },
                "required": ["kind"]
            }),
        ),
        tool(
            "get_item_history",
            "Looking at the history…",
            "Every answer and event for one word or sentence: when, which exercise, what was given and \
             expected, the mistake class, word-by-word results; its strength and grammar tags.",
            json!({
                "type": "object",
                "properties": { "item_id": { "type": "string", "pattern": "^(wd|sn)_[0-7][0-9a-hjkmnp-tv-z]{25}$" } },
                "required": ["item_id"]
            }),
        ),
        tool(
            "get_structure_stats",
            "Looking at the grammar…",
            "One grammar structure: its items, accuracy per week, mistake classes, and recent misses on \
             its items with given vs expected.",
            json!({
                "type": "object",
                "properties": { "tag": { "type": "string", "enum": crate::grammar::keys() }, "period": period_prop(), "course_id": course_prop() },
                "required": ["tag"]
            }),
        ),
        tool(
            "get_mistakes",
            "Looking at the mistakes…",
            "Concrete mistakes of one class (newest first): what was given and expected, the item, the \
             exercise, when; and the items it happens on most.",
            json!({
                "type": "object",
                "properties": {
                    "class": { "type": "string", "enum": classes },
                    "period": period_prop(),
                    "course_id": course_prop(),
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Default 15" }
                },
                "required": ["class"]
            }),
        ),
        tool(
            "compare_periods",
            "Comparing…",
            "Two periods side by side: accuracy and activity in each, what improved and what slipped \
             (words, sentences, structures, sounds), and mistakes by class in each. Dates are the \
             learner's days, inclusive; by default the last 14 days against the 14 before.",
            json!({
                "type": "object",
                "properties": {
                    "a_from": { "type": "string", "pattern": DATE, "description": "Earlier period, first day" },
                    "a_to": { "type": "string", "pattern": DATE },
                    "b_from": { "type": "string", "pattern": DATE, "description": "Later period, first day" },
                    "b_to": { "type": "string", "pattern": DATE },
                    "course_id": course_prop()
                }
            }),
        ),
        tool(
            "get_activity",
            "Looking at your practice…",
            "How the learner practises over a period: days active, minutes, drills started, finished and \
             left early (where), games, conversations, answers by hour and weekday, time to answer, audio \
             replays, translations shown, lessons practised most, and other events by kind.",
            json!({ "type": "object", "properties": { "period": period_prop(), "course_id": course_prop() } }),
        ),
        tool(
            "get_mastery_log",
            "Looking at the mastery points…",
            "One lesson's mastery ledger: its points now, whether it is mastered, the points it had \
             before the ledger began, and every change, newest first — when, the change and the points \
             before and after, the grade, a drill's score, days since the last go, extra practice, \
             drill or conversation, and the reason (with the rules each reason stands for).",
            json!({
                "type": "object",
                "properties": {
                    "lesson_id": { "type": "string", "pattern": "^ls_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Default 40" }
                },
                "required": ["lesson_id"]
            }),
        ),
        tool(
            "tag_grammar",
            "Tagging the grammar…",
            "Give existing words and sentences their grammar tags (replacing any they had). Ids from \
             get_course or list_untagged.",
            json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "maxItems": 200,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "pattern": "^(wd|sn)_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                                "grammar": { "type": "array", "items": { "type": "string", "enum": crate::grammar::keys() } }
                            },
                            "required": ["id", "grammar"]
                        }
                    }
                },
                "required": ["items"]
            }),
        ),
        tool(
            "list_untagged",
            "Looking for untagged items…",
            "Items in open courses still missing something, with their ids, and how many in all: grammar \
             tags (words and sentences), tokens (sentences not yet annotated word by word) or topics \
             (lessons, words and sentences).",
            json!({
                "type": "object",
                "properties": {
                    "missing": { "type": "string", "enum": ["grammar", "tokens", "topics"], "description": "Default grammar" },
                    "course_id": course_prop(),
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Default 100" }
                }
            }),
        ),
        tool(
            "tag_topics",
            "Tagging the topics…",
            "Give existing lessons, words and sentences their topics (replacing any they had). Ids from \
             get_course, list_lessons or list_untagged.",
            json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "maxItems": 200,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "pattern": "^(wd|sn|ls)_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                                "topics": crate::tutor::topics_prop()
                            },
                            "required": ["id", "topics"]
                        }
                    }
                },
                "required": ["items"]
            }),
        ),
        tool(
            "annotate_sentences",
            "Annotating the sentences…",
            "Annotate existing sentences word by word (replacing any annotation they had): one token \
             per word, in order. Ids from get_course or list_untagged.",
            json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "maxItems": 60,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "pattern": "^sn_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                                "tokens": crate::grammar::token_schema()
                            },
                            "required": ["id", "tokens"]
                        }
                    }
                },
                "required": ["items"]
            }),
        ),
        tool(
            "get_raw_log",
            "Reading the log…",
            "The learner's answer and event log, newest first, a page at a time: each answer with its \
             date, exercise, direction, prompt as shown, options in order, given and expected, right or \
             wrong, mistake class, course, lesson, source, time to answer, tries, plays, translations \
             shown, the words it involved (dictionary=form@role:result) and a conversation's verdict; each \
             event with its kind and details. `next` continues the listing.",
            json!({
                "type": "object",
                "properties": {
                    "period": period_prop(),
                    "from": { "type": "string", "pattern": DATE, "description": "First day (instead of period)" },
                    "to": { "type": "string", "pattern": DATE },
                    "course_id": course_prop(),
                    "lesson_id": { "type": "string", "pattern": "^ls_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                    "item_id": { "type": "string", "pattern": ITEM, "description": "A word, sentence or line: its answers and every answer it appeared in" },
                    "role": { "type": "string", "enum": crate::wordlinks::ROLES.iter().map(|(k, _)| *k).collect::<Vec<_>>(), "description": "With item_id: only where it had this role" },
                    "search": { "type": "string", "description": "Text in an item's Polish or English, or in a prompt, answer or expected answer" },
                    "exercise": { "type": "string" },
                    "mistake": { "type": "string", "enum": classes },
                    "topic": { "type": "string", "enum": crate::topics::keys() },
                    "grammar": { "type": "string", "enum": crate::grammar::keys() },
                    "correct": { "type": "boolean" },
                    "source": { "type": "string", "enum": crate::learnlog::SOURCES.iter().map(|(k, _)| *k).collect::<Vec<_>>() },
                    "kind": { "type": "string", "enum": ["answers", "events"] },
                    "next": { "type": "string", "description": "From the previous page" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Default 40" }
                }
            }),
        ),
        tool(
            "get_context_stats",
            "Looking at the contexts…",
            "Answers grouped one way, with counts and accuracy per group: for one word or sentence (every \
             time it appeared, on its own or inside other items), one grammar structure, one topic, or \
             everything. Groups: course practised from, role (on its own, in a sentence, dialogue, story, \
             song, or as a wrong option), case and reason (as annotated in the sentence), form, topic, \
             exercise, or source.",
            json!({
                "type": "object",
                "properties": {
                    "item_id": { "type": "string", "pattern": ITEM },
                    "tag": { "type": "string", "enum": crate::grammar::keys() },
                    "topic": { "type": "string", "enum": crate::topics::keys() },
                    "group_by": { "type": "string", "enum": crate::contexts::GROUPS.iter().map(|(k, _)| *k).collect::<Vec<_>>() },
                    "period": period_prop(),
                    "course_id": course_prop()
                },
                "required": ["group_by"]
            }),
        ),
    ]
}

const ITEM: &str = "^(wd|sn|dl)_[0-7][0-9a-hjkmnp-tv-z]{25}$";

#[derive(Debug, Deserialize)]
struct PeriodArgs {
    #[serde(default)]
    period: String,
    #[serde(default)]
    course_id: Option<CourseId>,
}

#[derive(Debug, Deserialize)]
struct HardArgs {
    kind: String,
    #[serde(default)]
    period: String,
    #[serde(default)]
    course_id: Option<CourseId>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct MasteryArgs {
    lesson_id: LessonId,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ItemArgs {
    item_id: String,
}

#[derive(Debug, Deserialize)]
struct StructureArgs {
    tag: String,
    #[serde(default)]
    period: String,
    #[serde(default)]
    course_id: Option<CourseId>,
}

#[derive(Debug, Deserialize)]
struct MistakeArgs {
    class: String,
    #[serde(default)]
    period: String,
    #[serde(default)]
    course_id: Option<CourseId>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct CompareArgs {
    a_from: Option<String>,
    a_to: Option<String>,
    b_from: Option<String>,
    b_to: Option<String>,
    #[serde(default)]
    course_id: Option<CourseId>,
}

#[derive(Debug, Deserialize)]
struct TagArgs {
    items: Vec<crate::grammar::TagItem>,
}

#[derive(Debug, Deserialize)]
struct UntaggedArgs {
    #[serde(default)]
    missing: Option<String>,
    #[serde(default)]
    course_id: Option<CourseId>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TopicArgs {
    items: Vec<crate::topics::TopicItem>,
}

#[derive(Debug, Deserialize)]
struct AnnotateArgs {
    items: Vec<crate::grammar::AnnotateItem>,
}

#[derive(Debug, Default, Deserialize)]
struct RawArgs {
    #[serde(default)]
    period: String,
    from: Option<String>,
    to: Option<String>,
    course_id: Option<CourseId>,
    lesson_id: Option<LessonId>,
    item_id: Option<String>,
    role: Option<String>,
    search: Option<String>,
    exercise: Option<String>,
    mistake: Option<String>,
    topic: Option<String>,
    grammar: Option<String>,
    correct: Option<bool>,
    source: Option<String>,
    kind: Option<String>,
    next: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ContextArgs {
    #[serde(default)]
    item_id: Option<String>,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    group_by: String,
    #[serde(default)]
    period: String,
    #[serde(default)]
    course_id: Option<CourseId>,
}

/// A raw-log row as the model reads it: what happened, compactly.
fn raw_row(r: &crate::rawlog::RawRow) -> Value {
    let mut v = json!({ "id": r.id, "date": date(r.day), "course": r.course, "lesson": r.lesson, "source": r.source });
    if !r.item.is_empty() {
        v["item"] = json!(r.item);
        v["item_id"] = json!(r.item_id);
    }
    match &r.answer {
        Some(a) => {
            v["exercise"] = json!(a.exercise);
            v["direction"] = json!(a.direction);
            if !a.prompt.is_empty() {
                v["prompt"] = json!(a.prompt);
            }
            if !a.options.is_empty() {
                v["options"] = json!(a.options);
            }
            v["given"] = json!(a.given);
            v["expected"] = json!(a.expected);
            v["correct"] = json!(a.correct);
            if !a.mistake.is_empty() {
                v["mistake"] = json!(a.mistake);
                v["detail"] = json!(a.detail);
            }
            if let Some(ms) = a.ms_answer {
                v["seconds"] = json!(crate::convert::round(crate::convert::real(ms) / 1000.0));
            }
            for (key, n) in [("tries", a.tries), ("plays", a.plays), ("reveals", a.reveals)] {
                if n > 0 {
                    v[key] = json!(n);
                }
            }
            if let Some(p) = a.position {
                v["card"] = json!(format!("{p}/{}", a.shown.as_ref().and_then(|s| s.of_cards).map(|n| n.to_string()).unwrap_or_default()));
            }
            let words: Vec<String> = a
                .links
                .iter()
                .filter(|l| l.item_kind == "word")
                .map(|l| {
                    let result = match l.correct {
                        Some(true) => ":right",
                        Some(false) => ":wrong",
                        None => "",
                    };
                    let why = if l.reason.is_empty() { String::new() } else { format!(" ({})", l.reason) };
                    format!("{}={}@{}{result}{why}", l.pl, l.form, l.role)
                })
                .collect();
            if !words.is_empty() {
                v["words"] = json!(words);
            }
            if let Some(verdict) = &a.verdict {
                v["verdict"] = json!({ "result": verdict.result, "xp": verdict.xp, "checkin_id": a.checkin_id });
            }
        }
        None => {
            v["event"] = json!(r.event);
            v["details"] = json!(r.event_details);
        }
    }
    v
}

/// Days since 1970 of a date written 2026-11-19.
pub fn day_of(date: &str) -> Result<i64> {
    let bad = || Error::msg(format!("Not a date: {date} (write it 2026-11-19)."));
    let mut parts = date.trim().split('-').map(|p| p.parse::<i64>().map_err(|_| bad()));
    let (y, m, d) = (parts.next().ok_or_else(bad)??, parts.next().ok_or_else(bad)??, parts.next().ok_or_else(bad)??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(bad());
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Ok(era * 146_097 + doe - 719_468)
}

fn date(day: i64) -> String {
    let (y, m, d) = crate::commands::civil_date(day);
    format!("{y}-{m:02}-{d:02}")
}

/// A filter's days as dates, for the model.
fn span(f: &Filter) -> Value {
    json!({ "from": f.from_day.map(date), "to": f.to_day.map(date), "course_id": f.course_id })
}

fn parse<T: serde::de::DeserializeOwned>(arguments: &str) -> Result<T> {
    serde_json::from_str(arguments).map_err(|e| Error::msg(format!("The tool arguments were not valid: {e}.")))
}

/// Answers kept short for the model: the fields that say what happened.
fn slim(a: &analytics::AttemptView) -> Value {
    let mut v = json!({
        "date": date(a.day),
        "exercise": a.exercise,
        "item": a.item,
        "item_id": a.item_id,
        "correct": a.correct,
        "given": a.given,
        "expected": a.expected,
    });
    if !a.mistake.is_empty() {
        v["mistake"] = json!(a.mistake);
        if !a.detail.is_empty() {
            v["detail"] = json!(a.detail);
        }
    }
    if !a.contrast.is_empty() {
        v["sound"] = json!(a.contrast);
    }
    v
}

fn pct(x: f64) -> i64 {
    crate::convert::round(x * 100.0)
}

/// Runs one of these tools; None when the name is not one of them.
pub fn run(conn: &mut Conn, name: &str, arguments: &str) -> Option<Result<Value>> {
    let result = match name {
        "get_learning_overview" => (|| {
            let args: PeriodArgs = parse(arguments)?;
            let f = Filter::period(conn, &args.period, args.course_id)?;
            let o = analytics::overview(conn, &f)?;
            Ok(json!({
                "period": span(&f),
                "answers": o.attempts,
                "accuracy_pct": pct(o.accuracy),
                "accuracy_before_pct": o.accuracy_before.map(pct),
                "trend": o.trend,
                "different_items": o.items,
                "days_active": o.days_active,
                "minutes": o.minutes,
                "mistakes_by_class": o.mistakes.iter().map(|c| json!({ "class": c.class, "count": c.count })).collect::<Vec<_>>(),
                "right_with_polish_letters_forgiven": o.letters_forgiven,
                "per_day": o.daily.iter().map(|d| json!([date(d.day), d.attempts, d.correct])).collect::<Vec<_>>(),
                "strongest": o.strongest.iter().map(|a| json!({ "kind": a.kind, "key": a.key, "name": a.name, "answers": a.attempts, "accuracy_pct": pct(a.accuracy) })).collect::<Vec<_>>(),
                "weakest": o.weakest.iter().map(|a| json!({ "kind": a.kind, "key": a.key, "name": a.name, "answers": a.attempts, "accuracy_pct": pct(a.accuracy) })).collect::<Vec<_>>(),
                "words_met": o.words_met,
                "structures_met": o.structures_met,
                "sounds_practised": o.sounds_met,
                "untagged_items": o.untagged,
            }))
        })(),
        "find_hard_items" => (|| {
            let args: HardArgs = parse(arguments)?;
            let f = Filter::period(conn, &args.period, args.course_id)?;
            let items = analytics::hard_items(conn, &args.kind, &f, args.limit.unwrap_or(10).min(50))?;
            Ok(json!({
                "period": span(&f),
                "items": items.iter().map(|h| json!({
                    "key": h.key,
                    "title": h.title,
                    "meaning": h.subtitle,
                    "answers": h.attempts,
                    "missed": h.wrong,
                    "accuracy_pct": pct(h.accuracy),
                    "mistakes": h.mistakes.iter().map(|c| json!({ "class": c.class, "count": c.count })).collect::<Vec<_>>(),
                    "latest_misses": h.last.iter().map(slim).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            }))
        })(),
        "get_item_history" => (|| {
            let args: ItemArgs = parse(arguments)?;
            let h = analytics::item_history(conn, &args.item_id)?;
            Ok(json!({
                "item_id": h.item_id,
                "pl": h.pl,
                "en": h.en,
                "lesson_id": h.lesson_id,
                "strength": h.strength,
                "answers": h.attempts,
                "missed": h.wrong,
                "grammar": h.grammar.iter().map(|t| t.key.clone()).collect::<Vec<_>>(),
                "mistakes": h.mistakes.iter().map(|c| json!({ "class": c.class, "count": c.count })).collect::<Vec<_>>(),
                "history": h.history.iter().take(60).map(|a| {
                    let mut v = slim(a);
                    if !a.words.is_empty() {
                        v["words"] = json!(a.words.iter().map(|w| format!("{}:{}:{}", w.s, w.e, w.g)).collect::<Vec<_>>());
                    }
                    v
                }).collect::<Vec<_>>(),
                "events": h.events.iter().take(40).map(|e| json!({ "kind": e.kind, "at": e.at })).collect::<Vec<_>>(),
            }))
        })(),
        "get_structure_stats" => (|| {
            let args: StructureArgs = parse(arguments)?;
            let f = Filter::period(conn, &args.period, args.course_id)?;
            let s = analytics::structure(conn, &args.tag, &f)?;
            Ok(json!({
                "period": span(&f),
                "tag": s.row.tag.key,
                "name": s.row.tag.name,
                "items": s.row.items,
                "answers": s.row.attempts,
                "accuracy_pct": s.row.accuracy.map(pct),
                "per_week": s.row.weeks.iter().map(|w| json!([date(w.day), w.attempts, w.correct])).collect::<Vec<_>>(),
                "mistakes": s.classes.iter().map(|c| json!({ "class": c.class, "count": c.count })).collect::<Vec<_>>(),
                "items_most_missed": s.examples.iter().take(12).map(|e| json!({ "id": e.item_id, "pl": e.pl, "en": e.en, "answers": e.attempts, "missed": e.wrong })).collect::<Vec<_>>(),
                "latest_misses": s.mistakes.iter().take(12).map(slim).collect::<Vec<_>>(),
            }))
        })(),
        "get_mistakes" => (|| {
            let args: MistakeArgs = parse(arguments)?;
            let f = Filter::period(conn, &args.period, args.course_id)?;
            let m = analytics::mistakes_of(conn, &args.class, &f, args.limit.unwrap_or(15).min(50))?;
            Ok(json!({
                "period": span(&f),
                "class": m.class,
                "count": m.count,
                "share_of_mistakes_pct": pct(m.share),
                "items": m.items.iter().take(10).map(|i| json!({ "item_id": i.item_id, "title": i.title, "count": i.count })).collect::<Vec<_>>(),
                "examples": m.examples.iter().map(slim).collect::<Vec<_>>(),
            }))
        })(),
        "compare_periods" => (|| {
            let args: CompareArgs = parse(arguments)?;
            let today = crate::db::today(conn);
            let day = |d: &Option<String>, fallback: i64| -> Result<i64> { d.as_deref().map_or(Ok(fallback), day_of) };
            let b = Filter { from_day: Some(day(&args.b_from, today - 13)?), to_day: Some(day(&args.b_to, today)?), course_id: args.course_id };
            let a = Filter { from_day: Some(day(&args.a_from, today - 27)?), to_day: Some(day(&args.a_to, today - 14)?), course_id: args.course_id };
            let c = analytics::compare(conn, &a, &b)?;
            let change = |x: &analytics::Change| json!({ "kind": x.kind, "key": x.key, "title": x.title, "a": [x.attempts_a, pct(x.accuracy_a)], "b": [x.attempts_b, pct(x.accuracy_b)] });
            Ok(json!({
                "a": { "period": span(&a), "answers": c.a.attempts, "accuracy_pct": pct(c.a.accuracy), "days_active": c.a.days_active },
                "b": { "period": span(&b), "answers": c.b.attempts, "accuracy_pct": pct(c.b.accuracy), "days_active": c.b.days_active },
                "improved": c.improved.iter().map(change).collect::<Vec<_>>(),
                "slipped": c.slipped.iter().map(change).collect::<Vec<_>>(),
                "mistakes_by_class": c.mistakes.iter().map(|m| json!({ "class": m.class, "a": m.a, "b": m.b })).collect::<Vec<_>>(),
                "note": "improved/slipped entries: [answers, accuracy_pct] in each period",
            }))
        })(),
        "get_activity" => (|| {
            let args: PeriodArgs = parse(arguments)?;
            let f = Filter::period(conn, &args.period, args.course_id)?;
            let a = analytics::activity(conn, &f)?;
            Ok(json!({
                "period": span(&f),
                "days_active": a.days_active,
                "minutes": a.minutes,
                "answers": a.attempts,
                "drills": { "started": a.sessions, "finished": a.finished, "left_early": a.left_early },
                "games": a.games,
                "checkins": a.checkins,
                "answers_by_hour": a.by_hour,
                "answers_by_weekday_mon_first": a.by_weekday,
                "median_seconds_to_answer": a.median_ms.map(|ms| crate::convert::round(crate::convert::real(ms) / 1000.0)),
                "audio_plays": a.audio_plays,
                "translations_shown": a.reveals,
                "left_early": a.exits.iter().take(8).map(|e| json!({ "lesson_id": e.lesson_id, "title": e.title, "at_card": e.at_card, "of_cards": e.of_cards })).collect::<Vec<_>>(),
                "lessons_most_practised": a.lessons.iter().take(8).map(|l| json!({ "lesson_id": l.lesson_id, "title": l.title, "answers": l.attempts, "accuracy_pct": pct(l.accuracy) })).collect::<Vec<_>>(),
                "events": a.events.iter().map(|e| json!({ "kind": e.class, "count": e.count })).collect::<Vec<_>>(),
            }))
        })(),
        "tag_grammar" => (|| {
            let args: TagArgs = parse(arguments)?;
            let (words, sentences) = crate::grammar::tag_items(conn, &args.items)?;
            Ok(json!({ "tagged_words": words, "tagged_sentences": sentences, "still_untagged": crate::grammar::untagged_count(conn)? }))
        })(),
        "list_untagged" => (|| {
            let args: UntaggedArgs = parse(arguments)?;
            let limit = args.limit.unwrap_or(100);
            let (total, items) = match args.missing.as_deref().unwrap_or("grammar") {
                "grammar" => (crate::grammar::untagged_count(conn)?, crate::grammar::untagged(conn, args.course_id, limit)?),
                "tokens" => (crate::grammar::unannotated_count(conn)?, crate::grammar::unannotated(conn, args.course_id, limit)?),
                "topics" => (crate::topics::untagged_count(conn)?, crate::topics::untagged(conn, args.course_id, limit)?),
                other => return Err(Error::msg(format!("Unknown 'missing': {other} (grammar, tokens or topics)."))),
            };
            Ok(json!({
                "total": total,
                "items": items.iter().map(|(id, pl, en, course)| json!({ "id": id, "pl": pl, "en": en, "course": course })).collect::<Vec<_>>(),
            }))
        })(),
        "tag_topics" => (|| {
            let args: TopicArgs = parse(arguments)?;
            let (words, sentences, lessons) = crate::topics::tag_items(conn, &args.items)?;
            Ok(json!({ "tagged_words": words, "tagged_sentences": sentences, "tagged_lessons": lessons, "still_untagged": crate::topics::untagged_count(conn)? }))
        })(),
        "annotate_sentences" => (|| {
            let args: AnnotateArgs = parse(arguments)?;
            let n = crate::grammar::annotate(conn, &args.items)?;
            Ok(json!({ "annotated": n, "still_unannotated": crate::grammar::unannotated_count(conn)? }))
        })(),
        "get_raw_log" => (|| {
            let args: RawArgs = parse(arguments)?;
            let span = match (&args.from, &args.to) {
                (None, None) => Filter::period(conn, &args.period, None)?,
                (from, to) => Filter {
                    from_day: from.as_deref().map(day_of).transpose()?,
                    to_day: to.as_deref().map(day_of).transpose()?,
                    course_id: None,
                },
            };
            let filter = crate::rawlog::RawFilter {
                from_day: span.from_day,
                to_day: span.to_day,
                course_id: args.course_id,
                lesson_id: args.lesson_id,
                item_id: args.item_id,
                role: args.role,
                search: args.search,
                exercise: args.exercise,
                mistake: args.mistake,
                topic: args.topic,
                grammar: args.grammar,
                correct: args.correct,
                source: args.source,
                kind: args.kind,
                context: None,
            };
            let before = match args.next.as_deref().filter(|s| !s.is_empty()) {
                Some(next) => {
                    let (at, id) = next.split_once(':').ok_or_else(|| Error::msg("`next` is the value the previous page gave."))?;
                    Some(crate::rawlog::Cursor { at: at.parse().map_err(|_| Error::msg("`next` is the value the previous page gave."))?, id: id.into() })
                }
                None => None,
            };
            let page = crate::rawlog::page(conn, &filter, before.as_ref(), args.limit.unwrap_or(40).clamp(1, 100))?;
            Ok(json!({
                "rows": page.rows.iter().map(raw_row).collect::<Vec<_>>(),
                "next": page.next.map(|c| format!("{}:{}", c.at, c.id)),
            }))
        })(),
        "get_context_stats" => (|| {
            let args: ContextArgs = parse(arguments)?;
            let f = Filter::period(conn, &args.period, args.course_id)?;
            let subject = crate::contexts::Subject { item_id: args.item_id, tag: args.tag, topic: args.topic };
            let s = crate::contexts::stats(conn, &subject, &args.group_by, &f)?;
            Ok(json!({
                "period": span(&f),
                "subject": s.subject,
                "group_by": s.group_by,
                "answers": s.attempts,
                "groups": s.groups.iter().map(|g| json!({ "key": g.key, "label": g.label, "answers": g.attempts, "correct": g.correct, "accuracy_pct": pct(g.accuracy) })).collect::<Vec<_>>(),
            }))
        })(),
        "get_mastery_log" => (|| {
            let args: MasteryArgs = parse(arguments)?;
            let log = crate::masterylog::for_lesson(conn, args.lesson_id)?;
            let reasons: std::collections::BTreeSet<&str> = log.entries.iter().map(|e| e.reason.as_str()).collect();
            Ok(json!({
                "lesson_id": log.lesson_id,
                "points": log.mastery,
                "mastered": log.mastered,
                "points_before_the_ledger": log.before_log.unwrap_or(0),
                "entries": log.entries.iter().take(args.limit.unwrap_or(40).min(100)).map(|e| json!({
                    "date": date(e.day),
                    "change": e.delta,
                    "before": e.before,
                    "after": e.after,
                    "grade": e.grade,
                    "score": e.total.map(|t| json!({ "correct": e.correct, "almost": e.almost, "total": t })),
                    "days_since_last_go": e.days_since,
                    "extra_practice": e.extra,
                    "via": e.via,
                    "reason": e.reason,
                    "text": e.text,
                })).collect::<Vec<_>>(),
                "reasons": crate::masterylog::REASONS.iter().filter(|(k, _)| reasons.contains(k)).map(|(k, v)| json!({ "reason": k, "rule": v })).collect::<Vec<_>>(),
            }))
        })(),
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_as_days() {
        assert_eq!(day_of("1970-01-01").unwrap(), 0);
        assert_eq!(day_of("2026-11-19").unwrap(), 20_776);
        assert_eq!(date(day_of("2024-02-29").unwrap()), "2024-02-29");
        assert!(day_of("19 Nov").is_err());
    }

    #[test]
    fn every_tool_says_what_it_returns() {
        for t in tools() {
            let description = t.definition["function"]["description"].as_str().unwrap();
            // Neutral: what it returns, never when to use it.
            assert!(!description.to_lowercase().contains("use this when"), "{}", t.name);
        }
    }
}
