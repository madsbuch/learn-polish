//! Programmes: practice topics made by the learner, through the tutor.
//!
//! A programme is stored as a topic of kind `programme`, with its own words,
//! sentences and (optionally) a short dialogue. That is all the session
//! builder, the scheduler, the library and the progress screen need, so a
//! programme on colours is practised exactly like a course topic.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db;
use crate::error::{Error, Result};

const MAX_WORDS: usize = 120;
const MAX_SENTENCES: usize = 60;
const MAX_DIALOGUE_LINES: usize = 40;

#[derive(Debug, Clone, Deserialize)]
pub struct NewWord {
    pub pl: String,
    pub en: String,
    #[serde(default)]
    pub pos: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewSentence {
    pub pl: String,
    pub en: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewLine {
    pub speaker: String,
    /// "f" or "m"; decides the voice. Anything else reads as "f".
    #[serde(default)]
    pub gender: String,
    pub pl: String,
    pub en: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewProgramme {
    /// English title, e.g. "Colours".
    pub title: String,
    #[serde(default)]
    pub title_pl: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub words: Vec<NewWord>,
    #[serde(default)]
    pub sentences: Vec<NewSentence>,
    #[serde(default)]
    pub dialogue: Vec<NewLine>,
}

/// What a programme holds after a change, reported back to the tutor.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProgrammeSummary {
    pub topic_id: i64,
    pub title: String,
    pub words: i64,
    pub sentences: i64,
    pub dialogue_lines: i64,
    /// Items that were offered but already present, so not added again.
    pub skipped_duplicates: i64,
}

/// Trims and collapses inner whitespace, so "  czerwony   kolor " is stored
/// the way it will be typed.
fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The part-of-speech codes the session understands. Models tend to spell
/// them out, so the common long forms are folded in.
fn normalize_pos(pos: &str) -> String {
    let pos = pos.trim().to_lowercase();
    let code = match pos.as_str() {
        "n-m" | "n-f" | "n-n" | "n-pl" | "v" | "adj" | "adv" | "num" | "pron" | "prep"
        | "conj" | "part" | "interj" | "phr" | "n" => pos.as_str(),
        "noun" => "n",
        "masculine noun" | "noun, masculine" | "noun-m" => "n-m",
        "feminine noun" | "noun, feminine" | "noun-f" => "n-f",
        "neuter noun" | "noun, neuter" | "noun-n" => "n-n",
        "plural noun" | "noun, plural" => "n-pl",
        "verb" => "v",
        "adjective" => "adj",
        "adverb" => "adv",
        "number" | "numeral" => "num",
        "pronoun" => "pron",
        "preposition" => "prep",
        "conjunction" => "conj",
        "particle" => "part",
        "interjection" => "interj",
        "phrase" | "expression" => "phr",
        "" => "",
        _ => "phr",
    };
    code.to_string()
}

fn require_programme(conn: &Connection, topic_id: i64) -> Result<String> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT kind, title_en FROM topics WHERE id = ?1",
            [topic_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        None => Err(Error::msg(format!("There is no topic with id {topic_id}."))),
        Some((kind, _)) if kind != "programme" => Err(Error::msg(
            "That is one of the built-in course topics, which cannot be changed. \
             Create a programme instead.",
        )),
        Some((_, title)) => Ok(title),
    }
}

pub fn create(conn: &Connection, input: NewProgramme) -> Result<ProgrammeSummary> {
    let title = clean(&input.title);
    if title.is_empty() {
        return Err(Error::msg("A programme needs a title."));
    }
    if input.words.is_empty() && input.sentences.is_empty() {
        return Err(Error::msg("A programme needs at least one word or sentence."));
    }

    let tx = conn.unchecked_transaction()?;
    let ord: i64 = tx.query_row(
        "SELECT COALESCE(MAX(ord), 0) + 1 FROM topics WHERE kind = 'programme'",
        [],
        |r| r.get(0),
    )?;
    let now = db::now_secs();
    let title_pl = match clean(&input.title_pl) {
        empty if empty.is_empty() => title.clone(),
        polish => polish,
    };
    tx.execute(
        "INSERT INTO topics (slug, ord, kind, title_pl, title_en, summary_en, created_at)
         VALUES (?1, ?2, 'programme', ?3, ?4, ?5, ?6)",
        rusqlite::params![
            format!("programme-{now}-{ord}"),
            ord,
            title_pl,
            title,
            clean(&input.summary),
            now
        ],
    )?;
    let topic_id = tx.last_insert_rowid();
    let skipped = insert_items(&tx, topic_id, &input.words, &input.sentences, &input.dialogue)?;
    tx.commit()?;
    summary(conn, topic_id, skipped)
}

/// Adds words, sentences or dialogue lines to an existing programme.
pub fn extend(
    conn: &Connection,
    topic_id: i64,
    words: &[NewWord],
    sentences: &[NewSentence],
    dialogue: &[NewLine],
) -> Result<ProgrammeSummary> {
    require_programme(conn, topic_id)?;
    let tx = conn.unchecked_transaction()?;
    let skipped = insert_items(&tx, topic_id, words, sentences, dialogue)?;
    tx.commit()?;
    summary(conn, topic_id, skipped)
}

/// Removes words or sentences (matched on their Polish text) from a programme,
/// along with their review history.
pub fn remove_items(
    conn: &Connection,
    topic_id: i64,
    words: &[String],
    sentences: &[String],
) -> Result<ProgrammeSummary> {
    require_programme(conn, topic_id)?;
    let tx = conn.unchecked_transaction()?;
    for (table, kind, items) in [("words", "word", words), ("sentences", "sentence", sentences)] {
        for pl in items {
            let id: Option<i64> = tx
                .query_row(
                    &format!("SELECT id FROM {table} WHERE topic_id = ?1 AND pl = ?2"),
                    rusqlite::params![topic_id, clean(pl)],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = id {
                tx.execute(
                    "DELETE FROM reviews WHERE kind = ?1 AND item_id = ?2",
                    rusqlite::params![kind, id],
                )?;
                tx.execute(&format!("DELETE FROM {table} WHERE id = ?1"), [id])?;
            }
        }
    }
    tx.commit()?;
    summary(conn, topic_id, 0)
}

pub fn rename(conn: &Connection, topic_id: i64, title: Option<&str>, title_pl: Option<&str>, summary_text: Option<&str>) -> Result<ProgrammeSummary> {
    require_programme(conn, topic_id)?;
    if let Some(title) = title.map(clean).filter(|t| !t.is_empty()) {
        conn.execute("UPDATE topics SET title_en = ?2 WHERE id = ?1", rusqlite::params![topic_id, title])?;
    }
    if let Some(title_pl) = title_pl.map(clean).filter(|t| !t.is_empty()) {
        conn.execute("UPDATE topics SET title_pl = ?2 WHERE id = ?1", rusqlite::params![topic_id, title_pl])?;
    }
    if let Some(text) = summary_text {
        conn.execute("UPDATE topics SET summary_en = ?2 WHERE id = ?1", rusqlite::params![topic_id, clean(text)])?;
    }
    summary(conn, topic_id, 0)
}

/// Deletes a programme with everything in it, including review history.
/// Returns its title.
pub fn delete(conn: &Connection, topic_id: i64) -> Result<String> {
    let title = require_programme(conn, topic_id)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM reviews WHERE kind = 'word'
         AND item_id IN (SELECT id FROM words WHERE topic_id = ?1)",
        [topic_id],
    )?;
    tx.execute(
        "DELETE FROM reviews WHERE kind = 'sentence'
         AND item_id IN (SELECT id FROM sentences WHERE topic_id = ?1)",
        [topic_id],
    )?;
    tx.execute("DELETE FROM words WHERE topic_id = ?1", [topic_id])?;
    tx.execute("DELETE FROM sentences WHERE topic_id = ?1", [topic_id])?;
    tx.execute("DELETE FROM conversation_lines WHERE topic_id = ?1", [topic_id])?;
    tx.execute("DELETE FROM topics WHERE id = ?1", [topic_id])?;
    if db::get_setting_i64(&tx, "focus_topic_id", 0) == topic_id {
        tx.execute("DELETE FROM settings WHERE key = 'focus_topic_id'", [])?;
    }
    tx.commit()?;
    Ok(title)
}

/// Points the daily check-in at a topic, or back at the course with `None`.
pub fn set_focus(conn: &Connection, topic_id: Option<i64>) -> Result<Option<String>> {
    match topic_id {
        None => {
            conn.execute("DELETE FROM settings WHERE key = 'focus_topic_id'", [])?;
            Ok(None)
        }
        Some(id) => {
            let title: String = conn
                .query_row("SELECT title_en FROM topics WHERE id = ?1", [id], |r| r.get(0))
                .optional()?
                .ok_or_else(|| Error::msg(format!("There is no topic with id {id}.")))?;
            db::set_setting(conn, "focus_topic_id", &id.to_string())?;
            Ok(Some(title))
        }
    }
}

fn insert_items(
    conn: &Connection,
    topic_id: i64,
    words: &[NewWord],
    sentences: &[NewSentence],
    dialogue: &[NewLine],
) -> Result<i64> {
    let count = |table: &str| -> Result<i64> {
        Ok(conn.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE topic_id = ?1"),
            [topic_id],
            |r| r.get(0),
        )?)
    };
    let mut skipped = 0;

    let mut ord = count("words")?;
    for word in words.iter().take(MAX_WORDS) {
        let (pl, en) = (clean(&word.pl), clean(&word.en));
        if pl.is_empty() || en.is_empty() {
            continue;
        }
        let added = conn.execute(
            "INSERT INTO words (topic_id, ord, pl, en, pos) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(topic_id, pl) DO NOTHING",
            rusqlite::params![topic_id, ord, pl, en, normalize_pos(&word.pos)],
        )?;
        if added == 0 {
            skipped += 1;
        } else {
            ord += 1;
        }
    }

    let mut ord = count("sentences")?;
    for sentence in sentences.iter().take(MAX_SENTENCES) {
        let (pl, en) = (clean(&sentence.pl), clean(&sentence.en));
        if pl.is_empty() || en.is_empty() {
            continue;
        }
        let added = conn.execute(
            "INSERT INTO sentences (topic_id, ord, pl, en) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(topic_id, pl) DO NOTHING",
            rusqlite::params![topic_id, ord, pl, en],
        )?;
        if added == 0 {
            skipped += 1;
        } else {
            ord += 1;
        }
    }

    let mut ord = count("conversation_lines")?;
    for line in dialogue.iter().take(MAX_DIALOGUE_LINES) {
        let (speaker, pl, en) = (clean(&line.speaker), clean(&line.pl), clean(&line.en));
        if pl.is_empty() || en.is_empty() {
            continue;
        }
        let gender = if line.gender.trim().eq_ignore_ascii_case("m") { "m" } else { "f" };
        conn.execute(
            "INSERT INTO conversation_lines (topic_id, ord, speaker, gender, pl, en)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                topic_id,
                ord,
                if speaker.is_empty() { "?".to_string() } else { speaker },
                gender,
                pl,
                en
            ],
        )?;
        ord += 1;
    }
    Ok(skipped)
}

fn summary(conn: &Connection, topic_id: i64, skipped: i64) -> Result<ProgrammeSummary> {
    let one = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [topic_id], |r| r.get(0))?) };
    Ok(ProgrammeSummary {
        topic_id,
        title: conn.query_row("SELECT title_en FROM topics WHERE id = ?1", [topic_id], |r| r.get(0))?,
        words: one("SELECT COUNT(*) FROM words WHERE topic_id = ?1")?,
        sentences: one("SELECT COUNT(*) FROM sentences WHERE topic_id = ?1")?,
        dialogue_lines: one("SELECT COUNT(*) FROM conversation_lines WHERE topic_id = ?1")?,
        skipped_duplicates: skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_of_speech_are_folded_to_codes() {
        assert_eq!(normalize_pos("Adjective"), "adj");
        assert_eq!(normalize_pos("n-f"), "n-f");
        assert_eq!(normalize_pos("noun, masculine"), "n-m");
        assert_eq!(normalize_pos("something odd"), "phr");
        assert_eq!(normalize_pos(""), "");
    }

    #[test]
    fn whitespace_is_collapsed() {
        assert_eq!(clean("  jasno   niebieski "), "jasno niebieski");
    }
}
