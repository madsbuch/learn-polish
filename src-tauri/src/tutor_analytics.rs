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
             left early (where), games, check-ins, answers by hour and weekday, time to answer, audio \
             replays, translations shown, lessons practised most, and other events by kind.",
            json!({ "type": "object", "properties": { "period": period_prop(), "course_id": course_prop() } }),
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
            "Words and sentences in open courses that have no grammar tag yet, with their ids, and how many in all.",
            json!({
                "type": "object",
                "properties": { "course_id": course_prop(), "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Default 100" } }
            }),
        ),
    ]
}

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
    course_id: Option<CourseId>,
    #[serde(default)]
    limit: Option<i64>,
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
            let items = crate::grammar::untagged(conn, args.course_id, args.limit.unwrap_or(100))?;
            Ok(json!({
                "total": crate::grammar::untagged_count(conn)?,
                "items": items.iter().map(|(id, pl, en, course)| json!({ "id": id, "pl": pl, "en": en, "course": course })).collect::<Vec<_>>(),
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
