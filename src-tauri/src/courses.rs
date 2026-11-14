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

use std::collections::HashSet;

use diesel::dsl::max;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::db::{self, Conn};
use crate::schema::{blocks, conversation_lines, course_blocks, course_lines, course_sentences, course_words, sentences, songs, topics, words};
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

pub fn require_course(conn: &mut Conn, topic_id: i64) -> Result<String> {
    topics::table
        .find(topic_id)
        .select(topics::title_en)
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg(format!("There is no course with id {topic_id}.")))
}

/// The position after the last course.
fn next_ord(conn: &mut Conn) -> Result<i64> {
    Ok(topics::table.select(max(topics::ord)).first::<Option<i64>>(conn)?.unwrap_or(0) + 1)
}

fn insert_topic(conn: &mut Conn, slug: Option<&str>, title: &str, title_pl: &str, summary: &str) -> Result<i64> {
    let ord = next_ord(conn)?;
    let now = db::now_secs();
    let slug = slug.map(str::to_string).unwrap_or_else(|| format!("course-{now}-{ord}"));
    Ok(diesel::insert_into(topics::table)
        .values((
            topics::slug.eq(slug),
            topics::ord.eq(ord),
            topics::kind.eq("course"),
            topics::title_pl.eq(title_pl),
            topics::title_en.eq(title),
            topics::summary_en.eq(clean(summary)),
            topics::created_at.eq(now),
        ))
        .returning(topics::id)
        .get_result(conn)?)
}

/// What a request may add: a model asked for "some words" should not be
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

pub fn create(conn: &mut Conn, input: NewCourse, origin: &str) -> Result<CourseSummary> {
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
    let (topic_id, skipped) = conn.transaction::<_, Error, _>(|conn| {
        let topic_id = insert_topic(conn, None, &title, &title_pl, &input.summary)?;
        let skipped = cut_into_drills(
            conn,
            topic_id,
            capped(&input.words, MAX_WORDS),
            capped(&input.sentences, MAX_SENTENCES),
            capped(&input.dialogue, MAX_DIALOGUE_LINES),
            origin,
        )?;
        Ok((topic_id, skipped))
    })?;
    summary(conn, topic_id, skipped)
}

/// The course with this slug, made (empty) if it does not exist yet. For the
/// courses blocks are filed in when nobody chose one.
pub fn ensure(conn: &mut Conn, slug: &str, title: &str, title_pl: &str, summary: &str) -> Result<i64> {
    if let Some(id) = topics::table.filter(topics::slug.eq(slug)).select(topics::id).first(conn).optional()? {
        return Ok(id);
    }
    insert_topic(conn, Some(slug), title, title_pl, summary)
}

/// Where a lesson goes when no course was chosen.
pub fn lessons_course(conn: &mut Conn) -> Result<i64> {
    ensure(conn, "lessons", "Lessons", "Lekcje", "Lessons on grammar, verbs and culture.")
}

/// Where listening blocks for songs go.
pub fn listening_course(conn: &mut Conn) -> Result<i64> {
    ensure(conn, "listening", "Listening", "Słuchanie", "Listen to your songs and answer questions about them.")
}

/// Where writing blocks go when no course is given.
pub fn writing_course(conn: &mut Conn) -> Result<i64> {
    ensure(conn, "writing", "Writing", "Pisanie", "Write your way through everyday situations, in Polish only.")
}

/// Where the starter lessons are. Dated like the other starter courses, so
/// it sorts with them.
pub fn grammar_course(conn: &mut Conn) -> Result<i64> {
    let id = ensure(
        conn,
        "grammar-essentials",
        "Grammar essentials",
        "Podstawy gramatyki",
        "Być and mieć, verb patterns, gender, the cases, and when to say ty or Pan.",
    )?;
    diesel::update(topics::table.find(id)).set(topics::created_at.eq(0)).execute(conn)?;
    Ok(id)
}

/// A course with nothing in it yet, for a drill the tutor fills itself.
pub fn create_empty(conn: &mut Conn, title: &str, summary: &str) -> Result<i64> {
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
pub fn add_block(conn: &mut Conn, topic_id: i64, block_id: i64) -> Result<()> {
    require_course(conn, topic_id)?;
    let ord = course_blocks::table
        .filter(course_blocks::topic_id.eq(topic_id))
        .select(max(course_blocks::ord))
        .first::<Option<i64>>(conn)?
        .unwrap_or(0)
        + 1;
    diesel::insert_or_ignore_into(course_blocks::table)
        .values((
            course_blocks::topic_id.eq(topic_id),
            course_blocks::block_id.eq(block_id),
            course_blocks::ord.eq(ord),
        ))
        .execute(conn)?;
    // A listening block filed in a real course gives a song without one its
    // course, so the song is known to belong there from now on.
    let is_listening = topics::table.find(topic_id).select(topics::slug).first::<String>(conn)? == "listening";
    let song: Option<i64> = blocks::table.find(block_id).select(blocks::song_id).first(conn).optional()?.flatten();
    if let (Some(song), false) = (song, is_listening) {
        diesel::update(songs::table.find(song).filter(songs::topic_id.is_null()))
            .set(songs::topic_id.eq(topic_id))
            .execute(conn)?;
    }
    Ok(())
}

/// Takes a block out of a course. Out of its last course, the block is
/// deleted with its words, sentences and history; returns true then.
pub fn remove_block(conn: &mut Conn, topic_id: i64, block_id: i64) -> Result<bool> {
    let removed = diesel::delete(course_blocks::table.find((topic_id, block_id))).execute(conn)?;
    if removed == 0 {
        return Err(Error::msg("That block is not in that course."));
    }
    delete_if_orphaned(conn, block_id)
}

fn delete_if_orphaned(conn: &mut Conn, block_id: i64) -> Result<bool> {
    let listed: i64 = course_blocks::table.filter(course_blocks::block_id.eq(block_id)).count().get_result(conn)?;
    if listed == 0 {
        diesel::delete(blocks::table.find(block_id)).execute(conn)?;
        return Ok(true);
    }
    Ok(false)
}

/// Moves a block from one course to another (its words go with it).
pub fn move_block(conn: &mut Conn, block_id: i64, from: i64, to: i64) -> Result<()> {
    if from == to {
        return Ok(());
    }
    conn.transaction::<_, Error, _>(|conn| {
        add_block(conn, to, block_id)?;
        remove_block(conn, from, block_id)?;
        Ok(())
    })
}

/// Puts a course's blocks in this order. Blocks left out keep their relative
/// order after the ones given.
pub fn order_blocks(conn: &mut Conn, topic_id: i64, block_ids: &[i64]) -> Result<()> {
    let current: Vec<i64> = crate::blocks::for_topic(conn, topic_id)?.iter().map(|b| b.id).collect();
    let mut order: Vec<i64> = Vec::new();
    for id in block_ids.iter().chain(current.iter()) {
        if current.contains(id) && !order.contains(id) {
            order.push(*id);
        }
    }
    for (ord, id) in order.iter().enumerate() {
        diesel::update(course_blocks::table.find((topic_id, id)))
            .set(course_blocks::ord.eq(ord as i64 + 1))
            .execute(conn)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

/// The Polish of every word a course's blocks own, lowercased.
fn course_words_known(conn: &mut Conn, topic_id: i64) -> Result<HashSet<String>> {
    Ok(course_words::table
        .filter(course_words::topic_id.eq(topic_id))
        .select(course_words::pl)
        .load::<String>(conn)?
        .into_iter()
        .map(|pl| pl.to_lowercase())
        .collect())
}

/// The Polish of every sentence a course's blocks own, lowercased.
fn course_sentences_known(conn: &mut Conn, topic_id: i64) -> Result<HashSet<String>> {
    Ok(course_sentences::table
        .filter(course_sentences::topic_id.eq(topic_id))
        .select(course_sentences::pl)
        .load::<String>(conn)?
        .into_iter()
        .map(|pl| pl.to_lowercase())
        .collect())
}

/// Gives a course more to practise: new words and sentences are cut into new
/// drills (eights and fives), dialogue lines join its conversation (or start
/// one). Anything the course already teaches is skipped and counted.
pub fn cut_into_drills(
    conn: &mut Conn,
    topic_id: i64,
    words: &[NewWord],
    sentences: &[NewSentence],
    dialogue: &[NewLine],
    origin: &str,
) -> Result<i64> {
    let title_pl: String = topics::table.find(topic_id).select(topics::title_pl).first(conn)?;
    let mut skipped = 0;
    let drills_of = |conn: &mut Conn, format: &str| -> Result<Vec<i64>> {
        Ok(course_blocks::table
            .inner_join(blocks::table)
            .filter(course_blocks::topic_id.eq(topic_id))
            .filter(blocks::format.eq(format))
            .order(course_blocks::ord)
            .select(blocks::id)
            .load(conn)?)
    };

    let lines: Vec<NewLine> = dialogue
        .iter()
        .filter(|l| !clean(&l.pl).is_empty() && !clean(&l.en).is_empty())
        .cloned()
        .collect();
    if !lines.is_empty() {
        let items = Items { dialogue: lines, ..Default::default() };
        match drills_of(conn, "dialogue")?.first() {
            Some(&block) => {
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
    let mut known = course_words_known(conn, topic_id)?;
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
    let first = drills_of(conn, "words")?.len() as i64;
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

    let mut known = course_sentences_known(conn, topic_id)?;
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
    let first = drills_of(conn, "sentences")?.len() as i64;
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
    conn: &mut Conn,
    topic_id: i64,
    words: &[NewWord],
    sentences: &[NewSentence],
    dialogue: &[NewLine],
    origin: &str,
) -> Result<CourseSummary> {
    require_course(conn, topic_id)?;
    let skipped = conn.transaction::<_, Error, _>(|conn| {
        cut_into_drills(
            conn,
            topic_id,
            capped(words, MAX_WORDS),
            capped(sentences, MAX_SENTENCES),
            capped(dialogue, MAX_DIALOGUE_LINES),
            origin,
        )
    })?;
    summary(conn, topic_id, skipped)
}

/// Puts items into one block, after what it has. Items it already holds (by
/// their Polish) are skipped; returns how many.
pub fn insert_items(conn: &mut Conn, block_id: i64, items: &Items) -> Result<i64> {
    let mut skipped = 0;

    let mut ord = words::table
        .filter(words::block_id.eq(block_id))
        .select(max(words::ord))
        .first::<Option<i64>>(conn)?
        .map_or(0, |o| o + 1);
    for word in items.words.iter().take(MAX_WORDS) {
        let (pl, en) = (clean(&word.pl), clean(&word.en));
        if pl.is_empty() || en.is_empty() {
            continue;
        }
        let added = diesel::insert_or_ignore_into(words::table)
            .values((
                words::block_id.eq(block_id),
                words::ord.eq(ord),
                words::pl.eq(pl),
                words::en.eq(en),
                words::pos.eq(normalize_pos(&word.pos)),
            ))
            .execute(conn)?;
        if added == 0 {
            skipped += 1;
        } else {
            ord += 1;
        }
    }

    let mut ord = sentences::table
        .filter(sentences::block_id.eq(block_id))
        .select(max(sentences::ord))
        .first::<Option<i64>>(conn)?
        .map_or(0, |o| o + 1);
    for sentence in items.sentences.iter().take(MAX_SENTENCES) {
        let (pl, en) = (clean(&sentence.pl), clean(&sentence.en));
        if pl.is_empty() || en.is_empty() {
            continue;
        }
        let added = diesel::insert_or_ignore_into(sentences::table)
            .values((
                sentences::block_id.eq(block_id),
                sentences::ord.eq(ord),
                sentences::pl.eq(pl),
                sentences::en.eq(en),
            ))
            .execute(conn)?;
        if added == 0 {
            skipped += 1;
        } else {
            ord += 1;
        }
    }

    let mut ord = conversation_lines::table
        .filter(conversation_lines::block_id.eq(block_id))
        .select(max(conversation_lines::ord))
        .first::<Option<i64>>(conn)?
        .map_or(0, |o| o + 1);
    for line in items.dialogue.iter().take(MAX_DIALOGUE_LINES) {
        let (speaker, pl, en) = (clean(&line.speaker), clean(&line.pl), clean(&line.en));
        if pl.is_empty() || en.is_empty() {
            continue;
        }
        let gender = if line.gender.trim().eq_ignore_ascii_case("m") { "m" } else { "f" };
        diesel::insert_into(conversation_lines::table)
            .values((
                conversation_lines::block_id.eq(block_id),
                conversation_lines::ord.eq(ord),
                conversation_lines::speaker.eq(if speaker.is_empty() { "?".to_string() } else { speaker }),
                conversation_lines::gender.eq(gender),
                conversation_lines::pl.eq(pl),
                conversation_lines::en.eq(en),
            ))
            .execute(conn)?;
        ord += 1;
    }
    Ok(skipped)
}

/// Removes words or sentences (matched on their Polish text) from a course's
/// blocks, along with their review history. A drill left with nothing to
/// practise goes too.
pub fn remove_items(
    conn: &mut Conn,
    topic_id: i64,
    remove_words: &[String],
    remove_sentences: &[String],
) -> Result<CourseSummary> {
    require_course(conn, topic_id)?;
    conn.transaction::<_, Error, _>(|conn| {
        let listed: Vec<i64> =
            course_blocks::table.filter(course_blocks::topic_id.eq(topic_id)).select(course_blocks::block_id).load(conn)?;
        let words_out: Vec<String> = remove_words.iter().map(|pl| clean(pl)).collect();
        let sentences_out: Vec<String> = remove_sentences.iter().map(|pl| clean(pl)).collect();
        diesel::delete(words::table.filter(words::block_id.eq_any(&listed)).filter(words::pl.eq_any(&words_out)))
            .execute(conn)?;
        diesel::delete(
            sentences::table.filter(sentences::block_id.eq_any(&listed)).filter(sentences::pl.eq_any(&sentences_out)),
        )
        .execute(conn)?;
        // Drills left with nothing to practise.
        let owners: HashSet<i64> = words::table
            .select(words::block_id)
            .filter(words::block_id.eq_any(&listed))
            .load::<i64>(conn)?
            .into_iter()
            .chain(sentences::table.select(sentences::block_id).filter(sentences::block_id.eq_any(&listed)).load::<i64>(conn)?)
            .chain(
                conversation_lines::table
                    .select(conversation_lines::block_id)
                    .filter(conversation_lines::block_id.eq_any(&listed))
                    .load::<i64>(conn)?,
            )
            .collect();
        let empty: Vec<i64> = blocks::table
            .filter(blocks::id.eq_any(&listed))
            .filter(blocks::kind.eq("drill"))
            .select(blocks::id)
            .load::<i64>(conn)?
            .into_iter()
            .filter(|id| !owners.contains(id))
            .collect();
        diesel::delete(blocks::table.filter(blocks::id.eq_any(empty))).execute(conn)?;
        Ok(())
    })?;
    summary(conn, topic_id, 0)
}

pub fn rename(conn: &mut Conn, topic_id: i64, title: Option<&str>, title_pl: Option<&str>, summary_text: Option<&str>) -> Result<CourseSummary> {
    require_course(conn, topic_id)?;
    let course = topics::table.find(topic_id);
    if let Some(title) = title.map(clean).filter(|t| !t.is_empty()) {
        diesel::update(course).set(topics::title_en.eq(title)).execute(conn)?;
    }
    if let Some(title_pl) = title_pl.map(clean).filter(|t| !t.is_empty()) {
        diesel::update(course).set(topics::title_pl.eq(title_pl)).execute(conn)?;
    }
    if let Some(text) = summary_text {
        diesel::update(course).set(topics::summary_en.eq(clean(text))).execute(conn)?;
    }
    summary(conn, topic_id, 0)
}

/// Deletes a course. Its blocks go with it — and their words and history —
/// unless another course also lists them. Returns its title.
pub fn delete(conn: &mut Conn, topic_id: i64) -> Result<String> {
    let title = require_course(conn, topic_id)?;
    conn.transaction::<_, Error, _>(|conn| {
        let blocks: Vec<i64> =
            course_blocks::table.filter(course_blocks::topic_id.eq(topic_id)).select(course_blocks::block_id).load(conn)?;
        diesel::delete(topics::table.find(topic_id)).execute(conn)?;
        for block in blocks {
            delete_if_orphaned(conn, block)?;
        }
        Ok(())
    })?;
    Ok(title)
}

fn summary(conn: &mut Conn, topic_id: i64, skipped: i64) -> Result<CourseSummary> {
    Ok(CourseSummary {
        topic_id,
        title: require_course(conn, topic_id)?,
        words: course_words::table.filter(course_words::topic_id.eq(topic_id)).count().get_result(conn)?,
        sentences: course_sentences::table.filter(course_sentences::topic_id.eq(topic_id)).count().get_result(conn)?,
        dialogue_lines: course_lines::table.filter(course_lines::topic_id.eq(topic_id)).count().get_result(conn)?,
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
        super::delete(&mut db.lock(), topic_id).map(|_| ())
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
        super::rename(&mut db.lock(), topic_id, Some(&title), Some(&title_pl), Some(&summary)).map(|_| ())
    }

    /// Lists a block in another course too.
    #[tauri::command]
    pub async fn block_add_to_course(db: State<'_, Db>, block_id: i64, topic_id: i64) -> Result<()> {
        super::add_block(&mut db.lock(), topic_id, block_id)
    }

    /// Takes a block out of a course; true when that was its last course and
    /// the block was deleted.
    #[tauri::command]
    pub async fn block_remove_from_course(db: State<'_, Db>, block_id: i64, topic_id: i64) -> Result<bool> {
        super::remove_block(&mut db.lock(), topic_id, block_id)
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
