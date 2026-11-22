//! How hard a drill's cards are. The lesson's CEFR level is the ceiling: it
//! allows some kinds of card and not others (a beginner never writes a whole
//! sentence from hearing it). Within the ceiling each word and sentence
//! climbs its own ladder of kinds, easiest first, a step for every day it
//! was answered with no miss and a step back for every day with one — its
//! familiarity. The names here are the spec's (`drill.*`, `xp.level-factor-*`).

use std::collections::HashMap;

use diesel::prelude::*;

use crate::db::Conn;
use crate::error::Result;
use crate::schema::{attempts, reviews};

/// A word's kinds of card, easiest first: pick its meaning, pick the word,
/// type it from its meaning, write it from hearing it, say it.
// Spec: DRILL-15
pub const WORD_CARDS: &[&str] = &["pick-meaning", "pick-word", "type-word", "hear-word", "say-word"];
/// A sentence's kinds of card, easiest first.
// Spec: DRILL-15
pub const SENTENCE_CARDS: &[&str] = &["tiles-set", "tiles-two", "bank-dictation", "tiles-all", "gap-typed", "dictation", "say-sentence"];

/// What each level adds to the kinds allowed below it (A1 first).
// Spec: DRILL-14
const CARDS_A1: &[&str] = &["pick-meaning", "pick-picture", "pick-word", "tiles-set", "gaps-from-bank"];
const CARDS_A2: &[&str] = &["type-word", "tiles-two", "bank-dictation"];
const CARDS_B1: &[&str] = &["hear-word", "say-word", "tiles-all", "gap-typed"];
const CARDS_B2: &[&str] = &["dictation", "say-sentence"];

/// How far up its ladder a known item starts at C1 and C2.
// Spec: DRILL-17
pub const C_HEAD_START: usize = 2;

/// The level a lesson's cards follow: its own, else its course's, else A1.
// Spec: DRILL-16
pub fn level_of(lesson_level: Option<&str>, course_level: Option<&str>) -> &'static str {
    lesson_level
        .and_then(crate::levels::normalize)
        .or_else(|| course_level.and_then(crate::levels::normalize))
        .unwrap_or("A1")
}

/// The level of a lesson by its id (A1 when it has none, or is not found).
pub fn level_of_lesson(conn: &mut Conn, lesson_id: crate::ids::LessonId) -> Result<&'static str> {
    use crate::schema::courses;
    let Ok(lesson) = crate::lessons::load(conn, lesson_id) else { return Ok("A1") };
    let course_level: Option<String> = match lesson.course_id {
        Some(id) => courses::table.find(id).select(courses::level).first::<Option<String>>(conn).optional()?.flatten(),
        None => None,
    };
    Ok(level_of(lesson.level.as_deref(), course_level.as_deref()))
}

/// Whether a level allows a kind of card: its own additions and those of the
/// levels below it; C1 and C2 allow every kind.
// Spec: DRILL-14
pub fn allows(level: &str, card: &str) -> bool {
    let rungs: &[&[&str]] = match level {
        "A1" => &[CARDS_A1],
        "A2" => &[CARDS_A1, CARDS_A2],
        "B1" => &[CARDS_A1, CARDS_A2, CARDS_B1],
        "B2" => &[CARDS_A1, CARDS_A2, CARDS_B1, CARDS_B2],
        _ => return true,
    };
    rungs.iter().any(|kinds| kinds.contains(&card))
}

/// An item's familiarity from its answers, one entry per day it was
/// answered (in day order): whether every answer that day was right. Up one
/// for a clean day, down one for a day with a miss, never below 0.
// Spec: DRILL-15
pub fn familiarity(days: &[bool]) -> usize {
    days.iter().fold(0usize, |f, clean| if *clean { f + 1 } else { f.saturating_sub(1) })
}

/// The card for an item: the kind at its familiarity's step on its ladder,
/// counting only the kinds the level allows (and `can` give here), stopping
/// at the hardest. A new item gets the easiest (after its introduction); at
/// C1 and C2 a known one starts a couple of steps up.
// Spec: DRILL-1, DRILL-15, DRILL-17, DRILL-18
pub fn pick(ladder: &[&'static str], level: &str, familiarity: usize, is_new: bool, can: impl Fn(&str) -> bool) -> &'static str {
    let allowed: Vec<&'static str> = ladder.iter().copied().filter(|k| allows(level, k) && can(k)).collect();
    let Some(first) = allowed.first().copied() else { return ladder[0] };
    if is_new {
        return first;
    }
    let head = if matches!(level, "C1" | "C2") { C_HEAD_START } else { 0 };
    allowed[(familiarity + head).min(allowed.len() - 1)]
}

/// The hardest kind on a ladder the level allows, up to `ceiling` (a
/// typing lesson at A1: picking the word, not typing it).
// Spec: TYP-1
pub fn at_most(ladder: &[&'static str], level: &str, ceiling: &str) -> &'static str {
    let top = ladder.iter().position(|k| *k == ceiling).unwrap_or(ladder.len() - 1);
    ladder[..=top].iter().rev().copied().find(|k| allows(level, k)).unwrap_or(ladder[0])
}

/// The familiarity of each of `ids` (words or sentences), from the learning
/// log's answers by day. An item answered before the log began counts at
/// least its run of right days in its review (nothing is backfilled).
pub fn familiarities(conn: &mut Conn, item_kind: &str, ids: &[String]) -> Result<HashMap<String, usize>> {
    let rows: Vec<(String, i64, i64)> = attempts::table
        .filter(attempts::item_kind.eq(item_kind))
        .filter(attempts::item_id.eq_any(ids))
        .select((attempts::item_id, attempts::day, attempts::correct))
        .order((attempts::item_id, attempts::day))
        .load(conn)?;
    let mut days: HashMap<String, Vec<(i64, bool)>> = HashMap::new();
    for (id, day, correct) in rows {
        let list = days.entry(id).or_default();
        match list.last_mut() {
            Some((d, clean)) if *d == day => *clean = *clean && correct == 1,
            _ => list.push((day, correct == 1)),
        }
    }
    let reps: HashMap<String, i64> = reviews::table
        .filter(reviews::kind.eq(item_kind))
        .filter(reviews::item_id.eq_any(ids))
        .select((reviews::item_id, reviews::reps))
        .load::<(String, i64)>(conn)?
        .into_iter()
        .collect();
    Ok(ids
        .iter()
        .map(|id| {
            let clean: Vec<bool> = days.get(id).map(|d| d.iter().map(|(_, c)| *c).collect()).unwrap_or_default();
            let from_reviews = crate::convert::index(reps.get(id).copied().unwrap_or(0));
            (id.clone(), familiarity(&clean).max(from_reviews))
        })
        .collect())
}

/// A level's XP factor, in quarters: A1 ×1, A2 ×1.25, B1 ×1.5, B2 ×1.75,
/// C1 and C2 ×2.
// Spec: PROG-2
pub fn xp_factor_quarters(level: &str) -> i64 {
    match level {
        "A1" => 4,
        "A2" => 5,
        "B1" => 6,
        "B2" => 7,
        _ => 8,
    }
}

/// XP times a level's factor, rounded (halves up).
// Spec: PROG-2
pub fn with_level(xp: i64, level: &str) -> i64 {
    (xp * xp_factor_quarters(level) + 2) / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    // Spec: DRILL-14
    #[test]
    fn a1_never_asks_endings_from_every_form_or_a_full_dictation() {
        for familiarity in 0..20 {
            for new in [true, false] {
                let card = pick(SENTENCE_CARDS, "A1", familiarity, new, |_| true);
                assert_eq!(card, "tiles-set", "familiarity {familiarity}");
                assert!(!["tiles-all", "dictation", "bank-dictation", "say-sentence"].contains(&card));
                let word = pick(WORD_CARDS, "A1", familiarity, new, |_| true);
                assert!(["pick-meaning", "pick-word"].contains(&word), "{word}");
            }
        }
        assert!(!allows("A1", "tiles-all") && !allows("A1", "dictation") && !allows("A1", "type-word"));
        assert!(allows("A2", "pick-meaning") && allows("A2", "type-word") && !allows("A2", "dictation"));
        assert!(allows("B2", "dictation") && allows("C1", "say-sentence") && allows("C2", "anything"));
    }

    // Spec: DRILL-1, DRILL-15
    #[test]
    fn cards_follow_familiarity_within_the_level() {
        // A new item: the easiest kind (its introduction comes before).
        assert_eq!(pick(WORD_CARDS, "B2", 9, true, |_| true), "pick-meaning");
        assert_eq!(pick(SENTENCE_CARDS, "B2", 9, true, |_| true), "tiles-set");
        // A known A2 word is typed once it has a couple of clean days.
        assert_eq!(pick(WORD_CARDS, "A2", 0, false, |_| true), "pick-meaning");
        assert_eq!(pick(WORD_CARDS, "A2", 1, false, |_| true), "pick-word");
        assert_eq!(pick(WORD_CARDS, "A2", 2, false, |_| true), "type-word");
        assert_eq!(pick(WORD_CARDS, "A2", 7, false, |_| true), "type-word", "A2 stops at typing");
        // Sentences climb: tiles, tiles with two endings, a word bank, every form…
        let b2: Vec<&str> = (0..8).map(|f| pick(SENTENCE_CARDS, "B2", f, false, |_| true)).collect();
        assert_eq!(b2, ["tiles-set", "tiles-two", "bank-dictation", "tiles-all", "gap-typed", "dictation", "say-sentence", "say-sentence"]);
        // …and a card that cannot be given (no speaking) steps down.
        assert_eq!(pick(SENTENCE_CARDS, "B2", 9, false, |k| k != "say-sentence"), "dictation");
        // C1 and C2 start a known item higher.
        assert_eq!(pick(SENTENCE_CARDS, "C1", 0, false, |_| true), "bank-dictation");
    }

    // Spec: DRILL-15
    #[test]
    fn a_miss_steps_familiarity_down_one() {
        assert_eq!(familiarity(&[]), 0);
        assert_eq!(familiarity(&[true, true, true]), 3);
        assert_eq!(familiarity(&[true, true, true, false]), 2);
        assert_eq!(familiarity(&[false, false, true]), 1);
    }

    // Spec: DRILL-16
    #[test]
    fn the_level_falls_back_to_the_course_then_a1() {
        assert_eq!(level_of(Some("B1"), Some("A1")), "B1");
        assert_eq!(level_of(None, Some("a2")), "A2");
        assert_eq!(level_of(None, None), "A1");
        assert_eq!(level_of(Some("nonsense"), None), "A1");
    }

    // Spec: TYP-1
    #[test]
    fn a_typing_lesson_types_from_a2() {
        assert_eq!(at_most(WORD_CARDS, "A1", "type-word"), "pick-word");
        assert_eq!(at_most(WORD_CARDS, "A2", "type-word"), "type-word");
        assert_eq!(at_most(WORD_CARDS, "C2", "type-word"), "type-word");
    }

    // Spec: PROG-2
    #[test]
    fn xp_grows_with_the_level() {
        assert_eq!(with_level(6, "A1"), 6);
        assert_eq!(with_level(6, "A2"), 8); // 7.5, rounded up
        assert_eq!(with_level(14, "B1"), 21);
        assert_eq!(with_level(14, "B2"), 25); // 24.5
        assert_eq!(with_level(14, "C1"), 28);
        assert_eq!(with_level(14, "C2"), 28);
    }
}
