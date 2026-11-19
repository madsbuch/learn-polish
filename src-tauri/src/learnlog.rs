//! The learning log: every graded answer (`attempts`) and everything else
//! the learner does while learning (`learning_events`), kept fine-grained so
//! the Progress pages and the tutor can say what the learner finds hard
//! (analytics.rs).
//!
//! An answer's row is written where it is graded, in the same transaction as
//! its grade (practice.rs, games.rs, examiner.rs), with its mistake class
//! (mistakes.rs) and the screen's signals: how long it took, tries, audio
//! played, translations shown. Events come from the screens in batches, one
//! command and one short transaction a batch (`learning_log`), so logging
//! never holds the database long. Both tables are append-only and per device
//! (docs/SYNC.md: nothing to conflict).

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
#[allow(unused_imports)]
use crate::ids::*;
use crate::mistakes::{Mistake, WordResult};
use crate::schema::{attempts, learning_events};

/// The version of the details kept with each row; bumped when their shape
/// changes, so a reader knows what it has.
pub const DETAILS_VERSION: u32 = 1;

/// The longest given or expected text kept: enough for a sentence.
const TEXT_KEPT: usize = 160;

fn short(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= TEXT_KEPT {
        text.to_string()
    } else {
        let mut cut: String = text.chars().take(TEXT_KEPT - 1).collect();
        cut.push('…');
        cut
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// How an answer was given, as the screen saw it. All optional: an answer
/// from somewhere that doesn't measure these logs zeros.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Signals {
    /// From the card showing to the first tap or key.
    #[serde(default)]
    pub ms_first: Option<i64>,
    /// From the card showing to the answer being checked.
    #[serde(default)]
    pub ms_answer: Option<i64>,
    /// Answers changed before checking (another option picked, retyped).
    #[serde(default)]
    pub tries: i64,
    #[serde(default)]
    pub hints: i64,
    /// The audio played, and how many of those were slow.
    #[serde(default)]
    pub plays: i64,
    #[serde(default)]
    pub slow_plays: i64,
    /// The English (or the translation) shown.
    #[serde(default)]
    pub reveals: i64,
    /// The first pick was changed before checking.
    #[serde(default)]
    pub changed: bool,
    /// The option picked first, when it was changed.
    #[serde(default)]
    pub first_pick: Option<String>,
}

/// What an attempt keeps beyond its columns (versioned by `v`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AttemptDetails {
    pub v: u32,
    /// Word by word (a sentence, tiles, a dictation, speech).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<WordResult>,
    /// The options shown (choices).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_pick: Option<String>,
    /// Grammar tag keys the examiner gave a check-in mistake.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grammar: Vec<String>,
    /// A check-in mistake's note.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

pub fn read_details(json: &str) -> AttemptDetails {
    serde_json::from_str(json).unwrap_or_default()
}

/// One graded answer, to be logged.
pub struct NewAttempt<'a> {
    pub item_kind: &'a str,
    pub item_id: &'a str,
    pub lesson_id: Option<LessonId>,
    pub exercise: &'a str,
    pub run: &'a str,
    pub correct: bool,
    pub given: &'a str,
    pub expected: &'a str,
    pub mistake: &'a Mistake,
    pub signals: &'a Signals,
    pub options: &'a [String],
    pub grammar: Vec<String>,
    pub note: String,
}

/// Writes one answer to the log. Called inside the grade's transaction.
pub fn write_attempt(conn: &mut Conn, a: &NewAttempt) -> Result<AttemptId> {
    let id = AttemptId::new();
    let details = AttemptDetails {
        v: DETAILS_VERSION,
        words: a.mistake.words.iter().take(40).map(|w| WordResult { e: short(&w.e), g: short(&w.g), s: w.s }).collect(),
        options: a.options.iter().take(8).map(|o| short(o)).collect(),
        first_pick: a.signals.first_pick.as_deref().map(short),
        grammar: a.grammar.clone(),
        note: short(&a.note),
    };
    let s = a.signals;
    diesel::insert_into(attempts::table)
        .values((
            attempts::id.eq(id),
            attempts::at.eq(now_ms()),
            attempts::day.eq(db::today(conn)),
            attempts::device.eq(crate::sync::device_id(conn)?),
            attempts::item_kind.eq(a.item_kind),
            attempts::item_id.eq(a.item_id),
            attempts::lesson_id.eq(a.lesson_id),
            attempts::exercise.eq(a.exercise),
            attempts::run.eq(short(a.run)),
            attempts::correct.eq(i64::from(a.correct)),
            attempts::given.eq(short(a.given)),
            attempts::expected.eq(short(a.expected)),
            attempts::mistake.eq(a.mistake.class),
            attempts::detail.eq(short(&a.mistake.detail)),
            attempts::contrast.eq(&a.mistake.contrast),
            attempts::ms_first.eq(s.ms_first.map(|v| v.clamp(0, 3_600_000))),
            attempts::ms_answer.eq(s.ms_answer.map(|v| v.clamp(0, 3_600_000))),
            attempts::tries.eq(s.tries.clamp(0, 999)),
            attempts::hints.eq(s.hints.clamp(0, 999)),
            attempts::plays.eq(s.plays.clamp(0, 999)),
            attempts::slow_plays.eq(s.slow_plays.clamp(0, 999)),
            attempts::reveals.eq(s.reveals.clamp(0, 999)),
            attempts::changed.eq(i64::from(s.changed)),
            attempts::details.eq(serde_json::to_string(&details)?),
        ))
        .execute(conn)?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Every kind of event, and what it means.
pub const EVENT_KINDS: &[(&str, &str)] = &[
    ("lesson_opened", "a lesson's page opened"),
    ("lesson_started", "a drill started (details: of_cards)"),
    ("lesson_left", "a drill left before the end (at_card, of_cards, answered, correct)"),
    ("lesson_finished", "a drill finished (answered, correct)"),
    ("item_viewed", "a card read: a new word or sentence, a dialogue, a scene, a story page"),
    ("spot_explored", "a thing tapped in a scene's picture"),
    ("meaning_tapped", "a word's meaning opened in lyrics or a story (word)"),
    ("talk_opened", "\"Talk about it\" opened on a word, sentence or line"),
    ("audio_played", "Polish played outside an answer (slow)"),
    ("card_skipped", "a card left out (reason: speaking_off, mic_refused, no_mic)"),
    ("game_started", "a quick game started (game)"),
    ("game_finished", "a quick game finished (game, ms, answered)"),
    ("checkin_started", "a check-in started"),
    ("report_asked", "the learning report asked for"),
];

pub fn is_event_kind(kind: &str) -> bool {
    EVENT_KINDS.iter().any(|(k, _)| *k == kind)
}

fn v1() -> u32 {
    DETAILS_VERSION
}

/// What an event keeps beyond its columns (versioned by `v`; strict: a
/// field not listed here is refused, so the shape can't drift).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventDetails {
    #[serde(default = "v1")]
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_card: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of_cards: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correct: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The Polish it was about (a tapped word, a word played).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slow: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<i64>,
    /// Where it happened: "session", "lyrics", "story", "lesson page", …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The exercise kind of the card it happened on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exercise: Option<String>,
}

/// An event from the screens.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventIn {
    pub kind: String,
    #[serde(default)]
    pub item_kind: String,
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub lesson_id: Option<LessonId>,
    #[serde(default)]
    pub run: String,
    /// When it happened, by the screen's clock (ms): a batch is sent a
    /// little later. Now, when not given.
    #[serde(default)]
    pub at: Option<i64>,
    #[serde(default)]
    pub details: EventDetails,
}

/// The most events one batch takes.
pub const MAX_BATCH: usize = 200;

fn check_event(event: &EventIn) -> Result<()> {
    if !is_event_kind(&event.kind) {
        return Err(Error::msg(format!("Unknown learning event: {}", event.kind)));
    }
    if !event.item_id.is_empty() && crate::ids::kind_of(&event.item_id).is_none() {
        return Err(Error::msg(format!("An event's item must be a TypeID: {}", event.item_id)));
    }
    Ok(())
}

fn clean_details(mut d: EventDetails) -> EventDetails {
    d.v = DETAILS_VERSION;
    for text in [&mut d.reason, &mut d.word, &mut d.game, &mut d.source, &mut d.exercise].into_iter().flatten() {
        *text = short(text);
    }
    d
}

/// Writes one event (inside a caller's transaction).
pub fn write_event(conn: &mut Conn, event: EventIn) -> Result<()> {
    check_event(&event)?;
    let now = now_ms();
    // A screen's clock a little behind or ahead is fine; a wild one is not.
    let at = event.at.filter(|at| (now - 86_400_000..=now + 60_000).contains(at)).unwrap_or(now);
    let offset = db::get_setting_i64(conn, "tz_offset_minutes", 0);
    let day = (at / 1000 + offset * 60).div_euclid(86_400);
    diesel::insert_into(learning_events::table)
        .values((
            learning_events::id.eq(LearningEventId::new()),
            learning_events::at.eq(at),
            learning_events::day.eq(day),
            learning_events::device.eq(crate::sync::device_id(conn)?),
            learning_events::kind.eq(&event.kind),
            learning_events::item_kind.eq(short(&event.item_kind)),
            learning_events::item_id.eq(event.item_id.trim()),
            learning_events::lesson_id.eq(event.lesson_id),
            learning_events::run.eq(short(&event.run)),
            learning_events::details.eq(serde_json::to_string(&clean_details(event.details))?),
        ))
        .execute(conn)?;
    Ok(())
}

/// A batch of events, all or nothing, in one short transaction. An unknown
/// kind fails the batch with its name (a screen bug, caught in testing).
pub fn log_events(conn: &mut Conn, events: Vec<EventIn>) -> Result<usize> {
    if events.len() > MAX_BATCH {
        return Err(Error::msg(format!("At most {MAX_BATCH} events at a time.")));
    }
    for event in &events {
        check_event(event)?;
    }
    let count = events.len();
    conn.transaction(|tx| {
        for event in events {
            write_event(tx, event)?;
        }
        Ok::<_, Error>(())
    })?;
    Ok(count)
}

#[tauri::command]
pub async fn learning_log(db: State<'_, Db>, events: Vec<EventIn>) -> Result<()> {
    log_events(&mut db.lock(), events).map(|_| ())
}

/// An event written from Rust (a card read, a check-in started).
pub fn note(conn: &mut Conn, kind: &str, item_kind: &str, item_id: &str, lesson_id: Option<LessonId>, run: &str, details: EventDetails) -> Result<()> {
    write_event(
        conn,
        EventIn { kind: kind.into(), item_kind: item_kind.into(), item_id: item_id.into(), lesson_id, run: run.into(), at: None, details },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_details_are_strict_and_versioned() {
        let ok: EventIn = serde_json::from_str(r#"{"kind":"lesson_left","details":{"atCard":3,"ofCards":12}}"#).unwrap();
        assert_eq!(ok.details.v, 1);
        assert_eq!(ok.details.at_card, Some(3));
        assert!(serde_json::from_str::<EventIn>(r#"{"kind":"lesson_left","details":{"mood":"bad"}}"#).is_err());
        assert!(check_event(&EventIn { kind: "nap".into(), ..EventIn::default() }).is_err());
        assert!(check_event(&EventIn { kind: "talk_opened".into(), item_id: "42".into(), ..EventIn::default() }).is_err());
        let written = serde_json::to_string(&EventDetails { v: 1, slow: Some(true), ..EventDetails::default() }).unwrap();
        assert_eq!(written, r#"{"v":1,"slow":true}"#, "compact: only what is set");
    }

    #[test]
    fn long_text_is_cut_short() {
        let long = "słowo ".repeat(80);
        let kept = short(&long);
        assert_eq!(kept.chars().count(), TEXT_KEPT);
        assert!(kept.ends_with('…'));
    }
}
