//! Courses: the folders blocks are listed in.
//!
//! Three levels: a **course** lists **blocks**; a drill block owns the
//! **words**, sentences or dialogue lines it practises. A course reaches its
//! words only through its blocks. A block may be listed in several courses;
//! taking it out of the last one deletes it, with everything it owns.
//!
//! Giving a course words, sentences or a dialogue cuts them into drills: the
//! dialogue, words in eights, sentences in fives. The starter courses a fresh
//! install comes with and the ones the tutor makes are the same thing.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db;
use crate::text::clean;
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
pub struct NewCourse {
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

/// What a course holds after a change, reported back to the tutor.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CourseSummary {
    pub topic_id: i64,
    pub title: String,
    pub words: i64,
    pub sentences: i64,
    pub dialogue_lines: i64,
    /// Items that were offered but already present, so not added again.
    pub skipped_duplicates: i64,
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

pub fn require_course(conn: &Connection, topic_id: i64) -> Result<String> {
    conn.query_row("SELECT title_en FROM topics WHERE id = ?1", [topic_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::msg(format!("There is no course with id {topic_id}.")))
}

/// The position after the last course.
fn next_ord(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COALESCE(MAX(ord), 0) + 1 FROM topics", [], |r| r.get(0))?)
}

fn insert_topic(conn: &Connection, slug: Option<&str>, title: &str, title_pl: &str, summary: &str) -> Result<i64> {
    let ord = next_ord(conn)?;
    let now = db::now_secs();
    let slug = slug.map(str::to_string).unwrap_or_else(|| format!("course-{now}-{ord}"));
    conn.execute(
        "INSERT INTO topics (slug, ord, kind, title_pl, title_en, summary_en, created_at)
         VALUES (?1, ?2, 'course', ?3, ?4, ?5, ?6)",
        rusqlite::params![slug, ord, title_pl, title, clean(summary), now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// What one request may add: a model asked for "some words" should not be
/// able to fill a course with a thousand. (The starter content is not capped.)
fn capped<T>(items: &[T], max: usize) -> &[T] {
    &items[..items.len().min(max)]
}

/// Words, sentences and dialogue lines on their way into a block.
#[derive(Debug, Clone, Default)]
pub struct Items {
    pub words: Vec<NewWord>,
    pub sentences: Vec<NewSentence>,
    pub dialogue: Vec<NewLine>,
}

pub fn create(conn: &Connection, input: NewCourse, origin: &str) -> Result<CourseSummary> {
    let title = clean(&input.title);
    if title.is_empty() {
        return Err(Error::msg("A course needs a title."));
    }
    if input.words.is_empty() && input.sentences.is_empty() {
        return Err(Error::msg("A course needs at least one word or sentence."));
    }
    let title_pl = match clean(&input.title_pl) {
        empty if empty.is_empty() => title.clone(),
        polish => polish,
    };
    let tx = conn.unchecked_transaction()?;
    let topic_id = insert_topic(&tx, None, &title, &title_pl, &input.summary)?;
    let skipped = cut_into_drills(
        &tx,
        topic_id,
        capped(&input.words, MAX_WORDS),
        capped(&input.sentences, MAX_SENTENCES),
        capped(&input.dialogue, MAX_DIALOGUE_LINES),
        origin,
    )?;
    tx.commit()?;
    summary(conn, topic_id, skipped)
}

/// The course with this slug, made (empty) if it does not exist yet. For the
/// courses blocks are filed in when nobody chose one.
pub fn ensure(conn: &Connection, slug: &str, title: &str, title_pl: &str, summary: &str) -> Result<i64> {
    if let Some(id) = conn
        .query_row("SELECT id FROM topics WHERE slug = ?1", [slug], |r| r.get(0))
        .optional()?
    {
        return Ok(id);
    }
    insert_topic(conn, Some(slug), title, title_pl, summary)
}

/// Where a lesson goes when no course was chosen.
pub fn lessons_course(conn: &Connection) -> Result<i64> {
    ensure(conn, "lessons", "Lessons", "Lekcje", "Lessons on grammar, verbs and culture.")
}

/// Where listening blocks for songs go.
pub fn listening_course(conn: &Connection) -> Result<i64> {
    ensure(conn, "listening", "Listening", "Słuchanie", "Listen to your songs and answer questions about them.")
}

/// Where the starter lessons are. Dated like the other starter courses, so
/// it sorts with them.
pub fn grammar_course(conn: &Connection) -> Result<i64> {
    let id = ensure(
        conn,
        "grammar-essentials",
        "Grammar essentials",
        "Podstawy gramatyki",
        "Być and mieć, verb patterns, gender, the cases, and when to say ty or Pan.",
    )?;
    conn.execute("UPDATE topics SET created_at = 0 WHERE id = ?1", [id])?;
    Ok(id)
}

/// A course with nothing in it yet, for a drill the tutor fills itself.
pub fn create_empty(conn: &Connection, title: &str, summary: &str) -> Result<i64> {
    let title = clean(title);
    if title.is_empty() {
        return Err(Error::msg("A course needs a title."));
    }
    insert_topic(conn, None, &title, &title, summary)
}

// ---------------------------------------------------------------------------
// Membership
// ---------------------------------------------------------------------------

/// Lists a block in a course, at the end. Listing it again changes nothing.
pub fn add_block(conn: &Connection, topic_id: i64, block_id: i64) -> Result<()> {
    require_course(conn, topic_id)?;
    conn.execute(
        "INSERT OR IGNORE INTO course_blocks (topic_id, block_id, ord)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(ord), 0) + 1 FROM course_blocks WHERE topic_id = ?1))",
        [topic_id, block_id],
    )?;
    Ok(())
}

/// Takes a block out of a course. Out of its last course, the block is
/// deleted with its words, sentences and history; returns true then.
pub fn remove_block(conn: &Connection, topic_id: i64, block_id: i64) -> Result<bool> {
    let removed = conn.execute(
        "DELETE FROM course_blocks WHERE topic_id = ?1 AND block_id = ?2",
        [topic_id, block_id],
    )?;
    if removed == 0 {
        return Err(Error::msg("That block is not in that course."));
    }
    delete_if_orphaned(conn, block_id)
}

fn delete_if_orphaned(conn: &Connection, block_id: i64) -> Result<bool> {
    let listed: i64 =
        conn.query_row("SELECT COUNT(*) FROM course_blocks WHERE block_id = ?1", [block_id], |r| r.get(0))?;
    if listed == 0 {
        conn.execute("DELETE FROM blocks WHERE id = ?1", [block_id])?;
        return Ok(true);
    }
    Ok(false)
}

/// Moves a block from one course to another (its words go with it).
pub fn move_block(conn: &Connection, block_id: i64, from: i64, to: i64) -> Result<()> {
    if from == to {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    add_block(&tx, to, block_id)?;
    remove_block(&tx, from, block_id)?;
    tx.commit()?;
    Ok(())
}

/// Puts a course's blocks in this order. Blocks left out keep their relative
/// order after the ones given.
pub fn order_blocks(conn: &Connection, topic_id: i64, block_ids: &[i64]) -> Result<()> {
    let current: Vec<i64> = crate::blocks::for_topic(conn, topic_id)?.iter().map(|b| b.id).collect();
    let mut order: Vec<i64> = Vec::new();
    for id in block_ids.iter().chain(current.iter()) {
        if current.contains(id) && !order.contains(id) {
            order.push(*id);
        }
    }
    for (ord, id) in order.iter().enumerate() {
        conn.execute(
            "UPDATE course_blocks SET ord = ?3 WHERE topic_id = ?1 AND block_id = ?2",
            rusqlite::params![topic_id, id, ord as i64 + 1],
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

/// The Polish of every word or sentence a course's blocks own, lowercased.
fn course_texts(conn: &Connection, topic_id: i64, table: &str) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT x.pl FROM {table} x JOIN course_blocks c ON c.block_id = x.block_id WHERE c.topic_id = ?1"
    ))?;
    let rows = stmt
        .query_map([topic_id], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .map(|pl| pl.to_lowercase())
        .collect();
    Ok(rows)
}

/// Gives a course more to practise: new words and sentences are cut into new
/// drills (eights and fives), dialogue lines join its conversation (or start
/// one). Anything the course already teaches is skipped and counted.
pub fn cut_into_drills(
    conn: &Connection,
    topic_id: i64,
    words: &[NewWord],
    sentences: &[NewSentence],
    dialogue: &[NewLine],
    origin: &str,
) -> Result<i64> {
    let title_pl: String =
        conn.query_row("SELECT title_pl FROM topics WHERE id = ?1", [topic_id], |r| r.get(0))?;
    let mut skipped = 0;

    let lines: Vec<NewLine> = dialogue
        .iter()
        .filter(|l| !clean(&l.pl).is_empty() && !clean(&l.en).is_empty())
        .cloned()
        .collect();
    if !lines.is_empty() {
        let existing: Option<i64> = conn
            .query_row(
                "SELECT b.id FROM blocks b JOIN course_blocks c ON c.block_id = b.id
                 WHERE c.topic_id = ?1 AND b.format = 'dialogue' ORDER BY c.ord LIMIT 1",
                [topic_id],
                |r| r.get(0),
            )
            .optional()?;
        let items = Items { dialogue: lines, ..Default::default() };
        match existing {
            Some(block) => {
                insert_items(conn, block, &items)?;
            }
            None => {
                crate::blocks::create_drill(
                    conn,
                    topic_id,
                    &format!("Conversation: {title_pl}"),
                    "Read a short dialogue, then fill in its missing lines.",
                    "dialogue",
                    "low",
                    &items,
                    None,
                    origin,
                )?;
            }
        }
    }

    // Numbering continues after the drills of that kind the course has.
    let drills = |format: &str| -> Result<i64> {
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM blocks b JOIN course_blocks c ON c.block_id = b.id
             WHERE c.topic_id = ?1 AND b.format = ?2",
            rusqlite::params![topic_id, format],
            |r| r.get(0),
        )?)
    };

    let mut known = course_texts(conn, topic_id, "words")?;
    let fresh: Vec<NewWord> = words
        .iter()
        .filter(|w| !clean(&w.pl).is_empty() && !clean(&w.en).is_empty())
        .filter(|w| {
            let new = known.insert(clean(&w.pl).to_lowercase());
            skipped += i64::from(!new);
            new
        })
        .cloned()
        .collect();
    let first = drills("words")?;
    for (n, chunk) in fresh.chunks(crate::blocks::WORDS_PER_BLOCK).enumerate() {
        let preview: Vec<String> = chunk.iter().take(3).map(|w| clean(&w.pl)).collect();
        crate::blocks::create_drill(
            conn,
            topic_id,
            &format!("Words {}", first + n as i64 + 1),
            &format!("{}, …", preview.join(", ")),
            "words",
            "low",
            &Items { words: chunk.to_vec(), ..Default::default() },
            None,
            origin,
        )?;
    }

    let mut known = course_texts(conn, topic_id, "sentences")?;
    let fresh: Vec<NewSentence> = sentences
        .iter()
        .filter(|s| !clean(&s.pl).is_empty() && !clean(&s.en).is_empty())
        .filter(|s| {
            let new = known.insert(clean(&s.pl).to_lowercase());
            skipped += i64::from(!new);
            new
        })
        .cloned()
        .collect();
    let first = drills("sentences")?;
    for (n, chunk) in fresh.chunks(crate::blocks::SENTENCES_PER_BLOCK).enumerate() {
        crate::blocks::create_drill(
            conn,
            topic_id,
            &format!("Sentences {}", first + n as i64 + 1),
            &clean(&chunk[0].pl),
            "sentences",
            "medium",
            &Items { sentences: chunk.to_vec(), ..Default::default() },
            None,
            origin,
        )?;
    }
    Ok(skipped)
}

/// Adds words, sentences or dialogue lines to an existing course.
pub fn extend(
    conn: &Connection,
    topic_id: i64,
    words: &[NewWord],
    sentences: &[NewSentence],
    dialogue: &[NewLine],
    origin: &str,
) -> Result<CourseSummary> {
    require_course(conn, topic_id)?;
    let tx = conn.unchecked_transaction()?;
    let skipped = cut_into_drills(
        &tx,
        topic_id,
        capped(words, MAX_WORDS),
        capped(sentences, MAX_SENTENCES),
        capped(dialogue, MAX_DIALOGUE_LINES),
        origin,
    )?;
    tx.commit()?;
    summary(conn, topic_id, skipped)
}

/// Puts items into one block, after what it has. Items it already holds (by
/// their Polish) are skipped; returns how many.
pub fn insert_items(conn: &Connection, block_id: i64, items: &Items) -> Result<i64> {
    let next = |table: &str| -> Result<i64> {
        Ok(conn.query_row(
            &format!("SELECT COALESCE(MAX(ord) + 1, 0) FROM {table} WHERE block_id = ?1"),
            [block_id],
            |r| r.get(0),
        )?)
    };
    let mut skipped = 0;

    let mut ord = next("words")?;
    for word in items.words.iter().take(MAX_WORDS) {
        let (pl, en) = (clean(&word.pl), clean(&word.en));
        if pl.is_empty() || en.is_empty() {
            continue;
        }
        let added = conn.execute(
            "INSERT INTO words (block_id, ord, pl, en, pos) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(block_id, pl) DO NOTHING",
            rusqlite::params![block_id, ord, pl, en, normalize_pos(&word.pos)],
        )?;
        if added == 0 {
            skipped += 1;
        } else {
            ord += 1;
        }
    }

    let mut ord = next("sentences")?;
    for sentence in items.sentences.iter().take(MAX_SENTENCES) {
        let (pl, en) = (clean(&sentence.pl), clean(&sentence.en));
        if pl.is_empty() || en.is_empty() {
            continue;
        }
        let added = conn.execute(
            "INSERT INTO sentences (block_id, ord, pl, en) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(block_id, pl) DO NOTHING",
            rusqlite::params![block_id, ord, pl, en],
        )?;
        if added == 0 {
            skipped += 1;
        } else {
            ord += 1;
        }
    }

    let mut ord = next("conversation_lines")?;
    for line in items.dialogue.iter().take(MAX_DIALOGUE_LINES) {
        let (speaker, pl, en) = (clean(&line.speaker), clean(&line.pl), clean(&line.en));
        if pl.is_empty() || en.is_empty() {
            continue;
        }
        let gender = if line.gender.trim().eq_ignore_ascii_case("m") { "m" } else { "f" };
        conn.execute(
            "INSERT INTO conversation_lines (block_id, ord, speaker, gender, pl, en)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                block_id,
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

/// Removes words or sentences (matched on their Polish text) from a course's
/// blocks, along with their review history. A drill left with nothing to
/// practise goes too.
pub fn remove_items(
    conn: &Connection,
    topic_id: i64,
    words: &[String],
    sentences: &[String],
) -> Result<CourseSummary> {
    require_course(conn, topic_id)?;
    let tx = conn.unchecked_transaction()?;
    for (table, items) in [("words", words), ("sentences", sentences)] {
        for pl in items {
            tx.execute(
                &format!(
                    "DELETE FROM {table} WHERE pl = ?2 AND block_id IN
                         (SELECT block_id FROM course_blocks WHERE topic_id = ?1)"
                ),
                rusqlite::params![topic_id, clean(pl)],
            )?;
        }
    }
    tx.execute(
        "DELETE FROM blocks WHERE kind = 'drill'
           AND id IN (SELECT block_id FROM course_blocks WHERE topic_id = ?1)
           AND NOT EXISTS (SELECT 1 FROM words w WHERE w.block_id = blocks.id)
           AND NOT EXISTS (SELECT 1 FROM sentences s WHERE s.block_id = blocks.id)
           AND NOT EXISTS (SELECT 1 FROM conversation_lines l WHERE l.block_id = blocks.id)",
        [topic_id],
    )?;
    tx.commit()?;
    summary(conn, topic_id, 0)
}

pub fn rename(conn: &Connection, topic_id: i64, title: Option<&str>, title_pl: Option<&str>, summary_text: Option<&str>) -> Result<CourseSummary> {
    require_course(conn, topic_id)?;
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

/// Deletes a course. Its blocks go with it — and their words and history —
/// unless another course also lists them. Returns its title.
pub fn delete(conn: &Connection, topic_id: i64) -> Result<String> {
    let title = require_course(conn, topic_id)?;
    let tx = conn.unchecked_transaction()?;
    let blocks: Vec<i64> = crate::blocks::for_topic(&tx, topic_id)?.iter().map(|b| b.id).collect();
    tx.execute("DELETE FROM topics WHERE id = ?1", [topic_id])?;
    for block in blocks {
        delete_if_orphaned(&tx, block)?;
    }
    tx.commit()?;
    Ok(title)
}

fn summary(conn: &Connection, topic_id: i64, skipped: i64) -> Result<CourseSummary> {
    let one = |table: &str| -> Result<i64> {
        Ok(conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM {table} x JOIN course_blocks c ON c.block_id = x.block_id
                 WHERE c.topic_id = ?1"
            ),
            [topic_id],
            |r| r.get(0),
        )?)
    };
    Ok(CourseSummary {
        topic_id,
        title: conn.query_row("SELECT title_en FROM topics WHERE id = ?1", [topic_id], |r| r.get(0))?,
        words: one("words")?,
        sentences: one("sentences")?,
        dialogue_lines: one("conversation_lines")?,
        skipped_duplicates: skipped,
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

pub mod commands {
    use tauri::State;

    use crate::db::Db;
    use crate::error::Result;

    /// Deletes a course with all its blocks, words and practice history.
    #[tauri::command]
    pub async fn course_delete(db: State<'_, Db>, topic_id: i64) -> Result<()> {
        super::delete(&db.lock(), topic_id).map(|_| ())
    }

    /// Renames a course or changes its summary.
    #[tauri::command]
    pub async fn course_rename(
        db: State<'_, Db>,
        topic_id: i64,
        title: String,
        title_pl: String,
        summary: String,
    ) -> Result<()> {
        super::rename(&db.lock(), topic_id, Some(&title), Some(&title_pl), Some(&summary)).map(|_| ())
    }

    /// Lists a block in another course too.
    #[tauri::command]
    pub async fn block_add_to_course(db: State<'_, Db>, block_id: i64, topic_id: i64) -> Result<()> {
        super::add_block(&db.lock(), topic_id, block_id)
    }

    /// Takes a block out of a course; true when that was its last course and
    /// the block was deleted.
    #[tauri::command]
    pub async fn block_remove_from_course(db: State<'_, Db>, block_id: i64, topic_id: i64) -> Result<bool> {
        super::remove_block(&db.lock(), topic_id, block_id)
    }
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
}
