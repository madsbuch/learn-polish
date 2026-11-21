//! Courses: the folders lessons are listed in.
//!
//! Three levels: a **course** lists **lessons**; a drill lesson owns the
//! **words**, sentences or dialogue lines it practises. A course reaches its
//! words only through its lessons. A lesson is in exactly one course (a
//! unique index on `course_lessons(lesson_id)`; Other lessons counts as one);
//! taking it out of its course deletes it, with everything it owns.
//!
//! Giving a course words, sentences or a dialogue cuts them into drills: the
//! dialogue, words in eights, sentences in fives. Courses are created by the
//! learner or their tutor; a fresh install has one small demo course.

use std::collections::HashSet;

use diesel::dsl::max;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::db::{self, Conn};
use crate::schema::{lessons, conversation_lines, course_lessons, course_lines, course_sentences, course_words, sentences, songs, courses, words};
use crate::text::clean;
use crate::error::{Error, Result};
#[allow(unused_imports)]
use crate::ids::*;

const MAX_WORDS: usize = 120;
const MAX_SENTENCES: usize = 60;
const MAX_DIALOGUE_LINES: usize = 40;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewWord {
    pub pl: String,
    pub en: String,
    #[serde(default)]
    pub pos: String,
    /// A real word an English ear hears it as, for a sound-pair card
    /// (sounds.rs). Optional: most words have none.
    #[serde(default)]
    pub near: Option<NearWord>,
    /// The grammar it shows (grammar.rs keys). Optional; checked.
    #[serde(default)]
    pub grammar: Vec<String>,
    /// What it is about (topics.rs keys). Optional; checked.
    #[serde(default)]
    pub topics: Vec<String>,
}

/// A word's near neighbour by sound: "prosię" (piglet) for "proszę".
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NearWord {
    pub pl: String,
    #[serde(default)]
    pub en: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewSentence {
    pub pl: String,
    pub en: String,
    /// Its words that stand in another form than their dictionary one, so a
    /// word-order card can ask for the ending (tiles.rs). Optional: without
    /// them the sentence is built by plain ordering.
    #[serde(default)]
    pub endings: Vec<crate::tiles::Ending>,
    /// The grammar it shows (grammar.rs keys). Optional; checked.
    #[serde(default)]
    pub grammar: Vec<String>,
    /// What it is about (topics.rs keys). Optional; checked.
    #[serde(default)]
    pub topics: Vec<String>,
    /// The sentence word by word (grammar.rs: one token per word, every
    /// value from the vocabulary). Optional; checked against the sentence.
    #[serde(default)]
    pub tokens: Vec<crate::grammar::TokenIn>,
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
    pub course_id: CourseId,
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

pub fn require_course(conn: &mut Conn, course_id: CourseId) -> Result<String> {
    courses::table
        .find(course_id)
        .select(courses::title_en)
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg(format!("There is no course with id {course_id}.")))
}

/// Like [`require_course`], for changes: a course closed as mastered is
/// finished, so nothing is added to it — revisiting its theme is a new course.
pub fn require_open_course(conn: &mut Conn, course_id: CourseId) -> Result<String> {
    let title = require_course(conn, course_id)?;
    let closed: Option<i64> = courses::table.find(course_id).select(courses::closed_at).first(conn)?;
    if closed.is_some() {
        return Err(Error::msg(format!(
            "“{title}” is mastered and closed, so it is not changed any more. To revisit its theme, make a new course (a level up)."
        )));
    }
    Ok(title)
}

/// The one folder for lessons that belong to no course (see [`other_course`]).
// Spec: CRS-8, PRIN-10
pub const OTHER: &str = "other";
/// Built-in courses that never end: just the Other lessons folder.
pub const OPEN_ENDED: [&str; 1] = [OTHER];
/// The built-in folders there were before Other lessons, merged into it.
pub const OLD_FOLDERS: [&str; 3] = ["lessons", "listening", "writing"];

/// Lessons in closed courses: retired, so check-ins no longer bring them back.
pub fn retired_lessons(conn: &mut Conn) -> Result<std::collections::HashSet<LessonId>> {
    Ok(course_lessons::table
        .inner_join(courses::table)
        .filter(courses::closed_at.is_not_null())
        .select(course_lessons::lesson_id)
        .load::<LessonId>(conn)?
        .into_iter()
        .collect())
}

/// A course just closed, for its celebration.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CourseClosed {
    pub title: String,
    pub lessons: i64,
    pub words: i64,
    /// Days from the course's making to its closing (at least 1).
    pub days: i64,
    /// XP its lessons earned.
    pub xp: i64,
}

/// Closes a mastered course — the learner's own celebration, and final
/// (PRINCIPLES.md: closed is closed). It moves to the Mastered section as a
/// trophy, stays readable, and its lessons leave play. Only a mastered course, and not the Other lessons
/// folder, can be closed. To revisit its theme, the tutor makes a new course.
// Spec: CRS-12, PRIN-7
pub fn close(conn: &mut Conn, course_id: CourseId) -> Result<CourseClosed> {
    let title = require_open_course(conn, course_id)?;
    let current = status(conn, course_id)?;
    if current.open_ended {
        return Err(Error::msg(format!("“{title}” keeps growing, so it is never closed.")));
    }
    if current.status != "mastered" {
        return Err(Error::msg(format!("“{title}” is not mastered yet, so it cannot be closed.")));
    }
    let now = db::now_secs();
    diesel::update(courses::table.find(course_id)).set(courses::closed_at.eq(now)).execute(conn)?;
    let ids: Vec<LessonId> =
        course_lessons::table.filter(course_lessons::course_id.eq(course_id)).select(course_lessons::lesson_id).load(conn)?;
    let words: i64 = words::table.filter(words::lesson_id.eq_any(&ids)).count().get_result(conn)?;
    let earned: Vec<(i64, i64)> = crate::schema::xp_log::table
        .filter(crate::schema::xp_log::lesson_id.eq_any(&ids))
        .select((crate::schema::xp_log::day, crate::schema::xp_log::xp))
        .load(conn)?;
    let xp = earned.iter().map(|e| e.1).sum();
    // From the first day its lessons were practised (or, before the XP log,
    // the day it was made) to today.
    let today = db::today(conn);
    let made: i64 = courses::table.find(course_id).select(courses::created_at).first(conn)?;
    let first = earned.iter().map(|e| e.0).min().unwrap_or(if made > 0 { made.div_euclid(86_400) } else { today });
    Ok(CourseClosed { title, lessons: crate::convert::count(ids.len()), words, days: (today - first + 1).max(1), xp })
}

/// The position after the last course.
fn next_ord(conn: &mut Conn) -> Result<i64> {
    Ok(courses::table.select(max(courses::ord)).first::<Option<i64>>(conn)?.unwrap_or(0) + 1)
}

fn insert_course(conn: &mut Conn, slug: Option<&str>, title: &str, title_pl: &str, summary: &str) -> Result<CourseId> {
    let ord = next_ord(conn)?;
    let now = db::now_secs();
    // A built-in course (a fixed slug) has the same id on every device.
    let id = slug.map_or_else(CourseId::new, |slug| CourseId::derived("course", slug));
    // A learner's course is named by its id: unique on every device, so two
    // devices making a course at the same moment never clash (a clash on a
    // unique column would delete one of them everywhere when they sync).
    let slug = slug.map_or_else(|| id.to_string(), str::to_string);
    Ok(diesel::insert_into(courses::table)
        .values((
            courses::id.eq(id),
            courses::slug.eq(slug),
            courses::ord.eq(ord),
            courses::kind.eq("course"),
            courses::title_pl.eq(title_pl),
            courses::title_en.eq(title),
            courses::summary_en.eq(clean(summary)),
            courses::created_at.eq(now),
        ))
        .returning(courses::id)
        .get_result(conn)?)
}

/// What a request may add: a model asked for "some words" should not be
/// able to fill a course with a thousand. (The starter content is not capped.)
fn capped<T>(items: &[T], max: usize) -> &[T] {
    &items[..items.len().min(max)]
}

/// Words, sentences and dialogue lines on their way into a lesson.
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
    let (course_id, skipped) = conn.transaction::<_, Error, _>(|conn| {
        let course_id = insert_course(conn, None, &title, &title_pl, &input.summary)?;
        let skipped = cut_into_drills(
            conn,
            course_id,
            capped(&input.words, MAX_WORDS),
            capped(&input.sentences, MAX_SENTENCES),
            capped(&input.dialogue, MAX_DIALOGUE_LINES),
            origin,
        )?;
        Ok((course_id, skipped))
    })?;
    summary(conn, course_id, skipped)
}

/// The course with this slug, made (empty) if it does not exist yet. For the
/// courses lessons are filed in when nobody chose one.
pub fn ensure(conn: &mut Conn, slug: &str, title: &str, title_pl: &str, summary: &str) -> Result<CourseId> {
    if let Some(id) = courses::table.filter(courses::slug.eq(slug)).select(courses::id).first(conn).optional()? {
        return Ok(id);
    }
    insert_course(conn, Some(slug), title, title_pl, summary)
}

/// Where a lesson goes that belongs to no course: a song to listen to, a
/// situation to write, a question answered on its own. A folder, not a
/// course — it never ends, and the course list shows it last.
pub fn other_course(conn: &mut Conn) -> Result<CourseId> {
    ensure(conn, OTHER, OTHER_TITLES.0, OTHER_TITLES.1, "One-off lessons that belong to no course.")
}

/// The Other lessons folder's English and Polish titles.
pub const OTHER_TITLES: (&str, &str) = ("Other lessons", "Inne lekcje");

/// The open-ended course of older databases ("Grammar & culture"), for
/// upgrades from before courses (db.rs).
pub const GRAMMAR_CULTURE: (&str, &str) = ("Grammar & culture", "Gramatyka i kultura");

/// Moves the lessons of the old built-in folders (Grammar & culture,
/// Listening, Writing) into Other lessons, keeping their order, and removes
/// the empty folders. Lessons, their history and mastery are untouched.
pub fn merge_old_folders(conn: &mut Conn) -> Result<()> {
    let old: Vec<CourseId> =
        courses::table.filter(courses::slug.eq_any(OLD_FOLDERS)).order(courses::id).select(courses::id).load(conn)?;
    if old.is_empty() {
        return Ok(());
    }
    conn.transaction::<_, Error, _>(|conn| {
        let other = other_course(conn)?;
        let mut ord: i64 = course_lessons::table
            .filter(course_lessons::course_id.eq(other))
            .select(max(course_lessons::ord))
            .first::<Option<i64>>(conn)?
            .unwrap_or(0);
        for folder in &old {
            let lessons: Vec<LessonId> = course_lessons::table
                .filter(course_lessons::course_id.eq(folder))
                .order(course_lessons::ord)
                .select(course_lessons::lesson_id)
                .load(conn)?;
            for lesson in lessons {
                ord += 1;
                // Off the folder first: a lesson is in one course only.
                diesel::delete(course_lessons::table.find((folder, lesson))).execute(conn)?;
                diesel::insert_or_ignore_into(course_lessons::table)
                    .values((
                        course_lessons::course_id.eq(other),
                        course_lessons::lesson_id.eq(lesson),
                        course_lessons::ord.eq(ord),
                    ))
                    .execute(conn)?;
            }
            diesel::delete(course_lessons::table.filter(course_lessons::course_id.eq(folder))).execute(conn)?;
            diesel::delete(courses::table.find(folder)).execute(conn)?;
        }
        Ok(())
    })
}

/// Where the starter lessons are. Dated like the other starter courses, so
/// it sorts with them.
pub fn grammar_course(conn: &mut Conn) -> Result<CourseId> {
    let id = ensure(
        conn,
        "grammar-essentials",
        "Grammar essentials",
        "Podstawy gramatyki",
        "Być and mieć, verb patterns, gender, the cases, and when to say ty or Pan.",
    )?;
    diesel::update(courses::table.find(id)).set(courses::created_at.eq(0)).execute(conn)?;
    Ok(id)
}

/// A course with nothing in it yet (tests).
#[cfg(test)]
pub fn create_empty(conn: &mut Conn, title: &str, summary: &str) -> Result<CourseId> {
    let title = clean(title);
    if title.is_empty() {
        return Err(Error::msg("A course needs a title."));
    }
    insert_course(conn, None, &title, &title, summary)
}

// ---------------------------------------------------------------------------
// Membership
// ---------------------------------------------------------------------------

/// Lists a lesson in a course, at the end: a lesson just made, or one in no
/// course yet. Listing it again changes nothing. A lesson is in one course
/// only, so one already in another course is refused.
pub fn add_lesson(conn: &mut Conn, course_id: CourseId, lesson_id: LessonId) -> Result<()> {
    require_open_course(conn, course_id)?;
    let listed: Option<CourseId> =
        course_lessons::table.filter(course_lessons::lesson_id.eq(lesson_id)).select(course_lessons::course_id).first(conn).optional()?;
    match listed {
        Some(listed) if listed == course_id => return Ok(()),
        Some(listed) => {
            let lesson: String = lessons::table.find(lesson_id).select(lessons::title).first(conn)?;
            let course = require_course(conn, listed)?;
            return Err(Error::msg(format!(
                "A lesson is in one course only: “{lesson}” is in “{course}”. Make a new lesson in the other course instead."
            )));
        }
        None => {}
    }
    let ord = course_lessons::table
        .filter(course_lessons::course_id.eq(course_id))
        .select(max(course_lessons::ord))
        .first::<Option<i64>>(conn)?
        .unwrap_or(0)
        + 1;
    diesel::insert_or_ignore_into(course_lessons::table)
        .values((
            course_lessons::course_id.eq(course_id),
            course_lessons::lesson_id.eq(lesson_id),
            course_lessons::ord.eq(ord),
        ))
        .execute(conn)?;
    // A listening lesson filed in a real course gives a song without one its
    // course, so the song is known to belong there from now on.
    let open_ended = OPEN_ENDED.contains(&courses::table.find(course_id).select(courses::slug).first::<String>(conn)?.as_str());
    let song: Option<SongId> = lessons::table.find(lesson_id).select(lessons::song_id).first(conn).optional()?.flatten();
    if let (Some(song), false) = (song, open_ended) {
        diesel::update(songs::table.find(song).filter(songs::course_id.is_null()))
            .set(songs::course_id.eq(course_id))
            .execute(conn)?;
    }
    Ok(())
}

/// Takes a lesson out of its course: a lesson is in one course only, so this
/// deletes it, with its words, sentences and history.
pub fn remove_lesson(conn: &mut Conn, course_id: CourseId, lesson_id: LessonId) -> Result<()> {
    require_open_course(conn, course_id)?;
    let listed: i64 = course_lessons::table.find((course_id, lesson_id)).count().get_result(conn)?;
    if listed == 0 {
        return Err(Error::msg("That lesson is not in that course."));
    }
    diesel::delete(lessons::table.find(lesson_id)).execute(conn)?;
    Ok(())
}

fn delete_if_orphaned(conn: &mut Conn, lesson_id: LessonId) -> Result<bool> {
    let listed: i64 = course_lessons::table.filter(course_lessons::lesson_id.eq(lesson_id)).count().get_result(conn)?;
    if listed == 0 {
        diesel::delete(lessons::table.find(lesson_id)).execute(conn)?;
        return Ok(true);
    }
    Ok(false)
}

/// Puts a course's lessons in this order. Lessons left out keep their relative
/// order after the ones given.
pub fn order_lessons(conn: &mut Conn, course_id: CourseId, lesson_ids: &[LessonId]) -> Result<()> {
    let current: Vec<LessonId> = crate::lessons::for_course(conn, course_id)?.iter().map(|b| b.id).collect();
    let mut order: Vec<LessonId> = Vec::new();
    for id in lesson_ids.iter().chain(current.iter()) {
        if current.contains(id) && !order.contains(id) {
            order.push(*id);
        }
    }
    for (ord, id) in order.iter().enumerate() {
        diesel::update(course_lessons::table.find((course_id, id)))
            .set(course_lessons::ord.eq(crate::convert::count(ord) + 1))
            .execute(conn)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

/// The Polish of every word a course's lessons own, lowercased.
fn course_words_known(conn: &mut Conn, course_id: CourseId) -> Result<HashSet<String>> {
    Ok(course_words::table
        .filter(course_words::course_id.eq(course_id))
        .select(course_words::pl)
        .load::<String>(conn)?
        .into_iter()
        .map(|pl| pl.to_lowercase())
        .collect())
}

/// The Polish of every sentence a course's lessons own, lowercased.
fn course_sentences_known(conn: &mut Conn, course_id: CourseId) -> Result<HashSet<String>> {
    Ok(course_sentences::table
        .filter(course_sentences::course_id.eq(course_id))
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
    course_id: CourseId,
    words: &[NewWord],
    sentences: &[NewSentence],
    dialogue: &[NewLine],
    origin: &str,
) -> Result<i64> {
    let title_pl: String = courses::table.find(course_id).select(courses::title_pl).first(conn)?;
    let mut skipped = 0;
    let drills_of = |conn: &mut Conn, format: &str| -> Result<Vec<LessonId>> {
        Ok(course_lessons::table
            .inner_join(lessons::table)
            .filter(course_lessons::course_id.eq(course_id))
            .filter(lessons::lesson_type.eq(format))
            .order(course_lessons::ord)
            .select(lessons::id)
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
            Some(&lesson) => {
                insert_items(conn, lesson, &items)?;
            }
            None => {
                crate::lessons::create_drill(
                    conn,
                    course_id,
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
    let mut known = course_words_known(conn, course_id)?;
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
    let first = crate::convert::count(drills_of(conn, "words")?.len() + drills_of(conn, "typing")?.len());
    for (n, chunk) in fresh.chunks(crate::lessons::WORDS_PER_LESSON).enumerate() {
        let preview: Vec<String> = chunk.iter().take(3).map(|w| clean(&w.pl)).collect();
        // For variety, every third batch is typed from memory instead of
        // recognised (both introduce their new words first).
        let number = first + crate::convert::count(n) + 1;
        let (title, format) = if number % 3 == 0 {
            (format!("Words {number} · typing"), "typing")
        } else {
            (format!("Words {number}"), "words")
        };
        crate::lessons::create_drill(
            conn,
            course_id,
            &title,
            &format!("{}, …", preview.join(", ")),
            format,
            "low",
            &Items { words: chunk.to_vec(), ..Default::default() },
            None,
            origin,
        )?;
    }

    let mut known = course_sentences_known(conn, course_id)?;
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
    let first = crate::convert::count(drills_of(conn, "sentences")?.len());
    for (n, chunk) in fresh.chunks(crate::lessons::SENTENCES_PER_LESSON).enumerate() {
        crate::lessons::create_drill(
            conn,
            course_id,
            &format!("Sentences {}", first + crate::convert::count(n) + 1),
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
    course_id: CourseId,
    words: &[NewWord],
    sentences: &[NewSentence],
    dialogue: &[NewLine],
    origin: &str,
) -> Result<CourseSummary> {
    require_open_course(conn, course_id)?;
    let skipped = conn.transaction::<_, Error, _>(|conn| {
        cut_into_drills(
            conn,
            course_id,
            capped(words, MAX_WORDS),
            capped(sentences, MAX_SENTENCES),
            capped(dialogue, MAX_DIALOGUE_LINES),
            origin,
        )
    })?;
    summary(conn, course_id, skipped)
}

/// Puts items into one lesson, after what it has. Items it already holds (by
/// their Polish) are skipped; returns how many.
pub fn insert_items(conn: &mut Conn, lesson_id: LessonId, items: &Items) -> Result<i64> {
    let mut skipped = 0;

    let mut ord = words::table
        .filter(words::lesson_id.eq(lesson_id))
        .select(max(words::ord))
        .first::<Option<i64>>(conn)?
        .map_or(0, |o| o + 1);
    for word in items.words.iter().take(MAX_WORDS) {
        let (pl, en) = (clean(&word.pl), clean(&word.en));
        if pl.is_empty() || en.is_empty() {
            continue;
        }
        // A neighbour by sound is kept only with its meaning: the card says
        // what each word of the pair is.
        let near = word
            .near
            .as_ref()
            .and_then(|n| Some((crate::sounds::clean_near(&pl, &n.pl)?, clean(&n.en))))
            .filter(|(_, en)| !en.is_empty())
            .unwrap_or_default();
        let tags = crate::grammar::validate(&word.grammar)?;
        let topics = crate::topics::validate(&word.topics)?;
        let id = WordId::new();
        let added = diesel::insert_or_ignore_into(words::table)
            .values((
                words::id.eq(id),
                words::lesson_id.eq(lesson_id),
                words::ord.eq(ord),
                words::pl.eq(pl),
                words::en.eq(en),
                words::pos.eq(normalize_pos(&word.pos)),
                words::near_pl.eq(near.0),
                words::near_en.eq(near.1),
            ))
            .execute(conn)?;
        if added == 0 {
            skipped += 1;
        } else {
            crate::grammar::set_word(conn, id, &tags)?;
            crate::topics::set_word(conn, id, &topics)?;
            ord += 1;
        }
    }

    let mut ord = sentences::table
        .filter(sentences::lesson_id.eq(lesson_id))
        .select(max(sentences::ord))
        .first::<Option<i64>>(conn)?
        .map_or(0, |o| o + 1);
    for sentence in items.sentences.iter().take(MAX_SENTENCES) {
        let (pl, en) = (clean(&sentence.pl), clean(&sentence.en));
        if pl.is_empty() || en.is_empty() {
            continue;
        }
        let endings = crate::tiles::clean_endings(&pl, &sentence.endings);
        let tags = crate::grammar::validate(&sentence.grammar)?;
        let topics = crate::topics::validate(&sentence.topics)?;
        let tokens = if sentence.tokens.is_empty() { vec![] } else { crate::grammar::validate_tokens(conn, &pl, &sentence.tokens)? };
        let id = SentenceId::new();
        let added = diesel::insert_or_ignore_into(sentences::table)
            .values((
                sentences::id.eq(id),
                sentences::lesson_id.eq(lesson_id),
                sentences::ord.eq(ord),
                sentences::pl.eq(pl),
                sentences::en.eq(en),
                sentences::endings.eq(serde_json::to_string(&endings)?),
            ))
            .execute(conn)?;
        if added == 0 {
            skipped += 1;
        } else {
            crate::grammar::set_sentence(conn, id, &tags)?;
            crate::topics::set_sentence(conn, id, &topics)?;
            crate::grammar::set_tokens(conn, id, &tokens)?;
            ord += 1;
        }
    }

    let mut ord = conversation_lines::table
        .filter(conversation_lines::lesson_id.eq(lesson_id))
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
                conversation_lines::id.eq(LineId::new()),
                conversation_lines::lesson_id.eq(lesson_id),
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
/// lessons, along with their review history. A drill left with nothing to
/// practise goes too.
pub fn remove_items(
    conn: &mut Conn,
    course_id: CourseId,
    remove_words: &[String],
    remove_sentences: &[String],
) -> Result<CourseSummary> {
    require_open_course(conn, course_id)?;
    conn.transaction::<_, Error, _>(|conn| {
        let listed: Vec<LessonId> =
            course_lessons::table.filter(course_lessons::course_id.eq(course_id)).select(course_lessons::lesson_id).load(conn)?;
        let words_out: Vec<String> = remove_words.iter().map(|pl| clean(pl)).collect();
        let sentences_out: Vec<String> = remove_sentences.iter().map(|pl| clean(pl)).collect();
        diesel::delete(words::table.filter(words::lesson_id.eq_any(&listed)).filter(words::pl.eq_any(&words_out)))
            .execute(conn)?;
        diesel::delete(
            sentences::table.filter(sentences::lesson_id.eq_any(&listed)).filter(sentences::pl.eq_any(&sentences_out)),
        )
        .execute(conn)?;
        // Drills left with nothing to practise.
        let owners: HashSet<LessonId> = words::table
            .select(words::lesson_id)
            .filter(words::lesson_id.eq_any(&listed))
            .load::<LessonId>(conn)?
            .into_iter()
            .chain(sentences::table.select(sentences::lesson_id).filter(sentences::lesson_id.eq_any(&listed)).load::<LessonId>(conn)?)
            .chain(
                conversation_lines::table
                    .select(conversation_lines::lesson_id)
                    .filter(conversation_lines::lesson_id.eq_any(&listed))
                    .load::<LessonId>(conn)?,
            )
            .collect();
        let empty: Vec<LessonId> = lessons::table
            .filter(lessons::id.eq_any(&listed))
            .filter(lessons::lesson_type.eq_any(crate::lesson_types::REGISTRY.iter().filter(|t| t.is_drill()).map(|t| t.id)))
            .select(lessons::id)
            .load::<LessonId>(conn)?
            .into_iter()
            .filter(|id| !owners.contains(id))
            .collect();
        diesel::delete(lessons::table.filter(lessons::id.eq_any(empty))).execute(conn)?;
        Ok(())
    })?;
    summary(conn, course_id, 0)
}

pub fn rename(conn: &mut Conn, course_id: CourseId, title: Option<&str>, title_pl: Option<&str>, summary_text: Option<&str>) -> Result<CourseSummary> {
    require_open_course(conn, course_id)?;
    let course = courses::table.find(course_id);
    if let Some(title) = title.map(clean).filter(|t| !t.is_empty()) {
        diesel::update(course).set(courses::title_en.eq(title)).execute(conn)?;
    }
    if let Some(title_pl) = title_pl.map(clean).filter(|t| !t.is_empty()) {
        diesel::update(course).set(courses::title_pl.eq(title_pl)).execute(conn)?;
    }
    if let Some(text) = summary_text {
        diesel::update(course).set(courses::summary_en.eq(clean(text))).execute(conn)?;
    }
    summary(conn, course_id, 0)
}

/// Orders a course's lessons as a path: the conversation first, to hear the
/// theme; then words, lessons and sentences interleaved, so each lesson
/// comes when its words are known; listening after; writing — using it all
/// in a situation — last.
pub fn arrange_path(conn: &mut Conn, course_id: CourseId) -> Result<()> {
    let rows: Vec<(LessonId, String)> = course_lessons::table
        .inner_join(lessons::table)
        .filter(course_lessons::course_id.eq(course_id))
        .order((course_lessons::ord, course_lessons::lesson_id))
        .select((lessons::id, lessons::lesson_type))
        .load(conn)?;
    // Each type says where it goes (lesson_types::PathGroup).
    use crate::lesson_types::PathGroup;
    let pick = |group: PathGroup| -> Vec<LessonId> {
        rows.iter()
            .filter(|(_, t)| crate::lesson_types::get(t).map_or(PathGroup::Study, |t| t.path) == group)
            .map(|(id, _)| *id)
            .collect()
    };
    let dialogues = pick(PathGroup::Dialogue);
    let words = pick(PathGroup::Words);
    let sentences = pick(PathGroup::Sentences);
    let lessons = pick(PathGroup::Study);
    let listening = pick(PathGroup::Listening);
    let writing = pick(PathGroup::Writing);

    let mut path = dialogues;
    let mut lessons = lessons.into_iter();
    let rounds = words.len().max(sentences.len());
    for round in 0..rounds {
        path.extend(words.get(round));
        // The first lesson once the first words are in; the next after the
        // first sentences; then one per round.
        if round == 0 {
            path.extend(lessons.next());
        }
        path.extend(sentences.get(round));
        path.extend(lessons.next());
    }
    path.extend(lessons);
    path.extend(listening);
    path.extend(writing);
    // Anything not sorted above keeps its place at the end.
    let placed: HashSet<LessonId> = path.iter().copied().collect();
    path.extend(rows.iter().map(|(id, ..)| *id).filter(|id| !placed.contains(id)));
    order_lessons(conn, course_id, &path)
}

/// Deletes a course. Its lessons go with it, and their words and history.
/// Returns its title.
pub fn delete(conn: &mut Conn, course_id: CourseId) -> Result<String> {
    let title = require_course(conn, course_id)?;
    conn.transaction::<_, Error, _>(|conn| {
        let lessons: Vec<LessonId> =
            course_lessons::table.filter(course_lessons::course_id.eq(course_id)).select(course_lessons::lesson_id).load(conn)?;
        let image: Option<FileId> = courses::table.find(course_id).select(courses::image_file_id).first(conn)?;
        // A listening lesson's episode that was never recorded (the demo's,
        // before a key) is nothing without the lesson: it goes too.
        let scripts: Vec<SongId> =
            lessons::table.filter(lessons::id.eq_any(&lessons)).filter(lessons::song_id.is_not_null()).select(lessons::song_id.assume_not_null()).load(conn)?;
        diesel::delete(songs::table.filter(songs::id.eq_any(&scripts)).filter(songs::status.eq("draft"))).execute(conn)?;
        diesel::delete(courses::table.find(course_id)).execute(conn)?;
        crate::files::delete(conn, &image.into_iter().collect::<Vec<_>>())?;
        for lesson in lessons {
            delete_if_orphaned(conn, lesson)?;
        }
        Ok(())
    })?;
    Ok(title)
}

fn summary(conn: &mut Conn, course_id: CourseId, skipped: i64) -> Result<CourseSummary> {
    Ok(CourseSummary {
        course_id,
        title: require_course(conn, course_id)?,
        words: course_words::table.filter(course_words::course_id.eq(course_id)).count().get_result(conn)?,
        sentences: course_sentences::table.filter(course_sentences::course_id.eq(course_id)).count().get_result(conn)?,
        dialogue_lines: course_lines::table.filter(course_lines::course_id.eq(course_id)).count().get_result(conn)?,
        skipped_duplicates: skipped,
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

pub mod commands {
    #[allow(unused_imports)]
    use crate::ids::*;
    use tauri::State;

    use crate::db::Db;
    use crate::error::Result;

    /// Deletes a course with all its lessons, words and practice history.
    #[tauri::command]
    pub async fn course_delete(db: State<'_, Db>, course_id: CourseId) -> Result<()> {
        super::delete(&mut db.lock(), course_id).map(|_| ())
    }

    /// Renames a course or changes its summary.
    #[tauri::command]
    pub async fn course_rename(
        db: State<'_, Db>,
        course_id: CourseId,
        title: String,
        title_pl: String,
        summary: String,
    ) -> Result<()> {
        super::rename(&mut db.lock(), course_id, Some(&title), Some(&title_pl), Some(&summary)).map(|_| ())
    }

    /// Closes a mastered course (see [`super::close`]).
    #[tauri::command]
    pub async fn course_close(db: State<'_, Db>, course_id: CourseId) -> Result<super::CourseClosed> {
        super::close(&mut db.lock(), course_id)
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

// ---------------------------------------------------------------------------
// Finishing a course
// ---------------------------------------------------------------------------

/// Where a course stands: active, completed (every core lesson passed once)
/// or mastered (every lesson — challenges too — mastered: full mastery
/// points, writing done without help; see [`crate::lessons::is_mastered`]).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CourseStatus {
    /// "active" | "completed" | "mastered"
    pub status: String,
    /// Core lessons (not challenges), and how many have been passed.
    pub core_lessons: i64,
    pub passed_core: i64,
    pub challenges: i64,
    /// Lessons, challenges too, already mastered.
    pub mastered_lessons: i64,
    /// Mastery points earned across its lessons, out of [`Self::mastery_max`]:
    /// moves with every pass, so the way to mastering it shows.
    pub mastery_points: i64,
    pub mastery_max: i64,
    /// About how many minutes of practice are left to master it: each lesson's
    /// minutes, times the passes it still needs when spaced out
    /// (lessons::passes_left, from its own schedule).
    pub minutes_to_master: i64,
    /// The first core lesson, in the course's order, not passed yet.
    pub next_lesson: Option<String>,
    /// A built-in course that never ends (see [`OPEN_ENDED`]): always active.
    pub open_ended: bool,
    /// When it was closed as mastered, if it was.
    pub closed_at: Option<i64>,
}

pub fn status(conn: &mut Conn, course_id: CourseId) -> Result<CourseStatus> {
    let list: Vec<(i64, i64, i64, String, Option<i64>, String, String)> = course_lessons::table
        .inner_join(lessons::table)
        .filter(course_lessons::course_id.eq(course_id))
        .order((course_lessons::ord, lessons::id))
        .select((
            lessons::challenge,
            lessons::passed,
            lessons::mastery,
            lessons::lesson_type,
            lessons::mastered_at,
            lessons::title,
            lessons::complexity,
        ))
        .load(conn)?;
    // Passes left at the spaced pace, each lesson by its own schedule
    // (lessons::passes_left), times its minutes.
    let ids: Vec<LessonId> = course_lessons::table
        .filter(course_lessons::course_id.eq(course_id))
        .select(course_lessons::lesson_id)
        .load(conn)?;
    let minutes_to_master = crate::lessons::load_many(conn, &ids)?
        .iter()
        .filter(|l| !l.mastered)
        .map(|l| l.passes_left.spaced.max(1) * crate::lesson_types::minutes(&l.lesson_type, &l.complexity))
        .sum();
    // Its core lessons: all but challenges — or, for a course of challenges
    // only, those, so it can be completed too.
    let mut core: Vec<_> = list.iter().filter(|l| l.0 == 0).collect();
    if core.is_empty() {
        core = list.iter().collect();
    }
    let passed_core = crate::convert::count(core.iter().filter(|l| l.1 == 1).count());
    let completed = !core.is_empty() && passed_core == crate::convert::count(core.len());
    let mastered_lessons = list
        .iter()
        .filter(|l| l.4.is_some())
        .count();
    let mastered = completed && mastered_lessons == list.len();
    let mastery_points = list.iter().map(|l| l.2.clamp(0, crate::lessons::MASTERY_FULL)).sum();
    let (slug, closed_at): (String, Option<i64>) =
        courses::table.find(course_id).select((courses::slug, courses::closed_at)).first(conn)?;
    let open_ended = OPEN_ENDED.contains(&slug.as_str());
    Ok(CourseStatus {
        status: if open_ended {
            "active"
        } else if mastered {
            "mastered"
        } else if completed {
            "completed"
        } else {
            "active"
        }
        .into(),
        open_ended,
        closed_at,
        core_lessons: crate::convert::count(core.len()),
        passed_core,
        challenges: crate::convert::count(list.len() - core.len()),
        mastered_lessons: crate::convert::count(mastered_lessons),
        mastery_points,
        mastery_max: crate::convert::count(list.len()) * crate::lessons::MASTERY_FULL,
        minutes_to_master,
        next_lesson: core.iter().find(|l| l.1 == 0).map(|l| l.5.clone()),
    })
}

/// A course just completed or mastered, to celebrate.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub course_id: CourseId,
    pub title: String,
    pub mastered: bool,
}

/// Courses that have reached completed or mastered since last asked: each
/// is noted, so it is celebrated once.
pub fn milestones(conn: &mut Conn) -> Result<Vec<Milestone>> {
    let open: Vec<(CourseId, String, Option<i64>, Option<i64>)> = courses::table
        .filter(courses::completed_at.is_null().or(courses::mastered_at.is_null()))
        .select((courses::id, courses::title_en, courses::completed_at, courses::mastered_at))
        .load(conn)?;
    let mut reached = Vec::new();
    for (id, title, completed_at, mastered_at) in open {
        let status = status(conn, id)?.status;
        let now = db::now_secs();
        if status == "mastered" && mastered_at.is_none() {
            // Mastered: ready to close. Closing is the learner's own
            // celebration ("Close as mastered", see [`close`]); mastery is
            // for good, so it stays ready until they do.
            diesel::update(courses::table.find(id))
                .set((courses::mastered_at.eq(now), courses::completed_at.eq(completed_at.unwrap_or(now))))
                .execute(conn)?;
            reached.push(Milestone { course_id: id, title, mastered: true });
        } else if status != "active" && completed_at.is_none() {
            diesel::update(courses::table.find(id)).set(courses::completed_at.eq(now)).execute(conn)?;
            reached.push(Milestone { course_id: id, title, mastered: false });
        }
    }
    Ok(reached)
}
