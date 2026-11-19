//! Builds the exercises of a drill lesson: a handful of words, sentences or a
//! dialogue, practised in the lesson's format.

use std::collections::HashSet;

use diesel::prelude::*;
use rand::seq::SliceRandom;

use crate::db::{self, Conn};
use crate::schema::{conversation_lines, course_lessons, course_lines, course_sentences, course_words, reviews, sentences, courses, words};
use crate::error::Result;
use crate::models::{DialogueLine, Exercise, Gap, Pair, SessionPlan, Course};
#[allow(unused_imports)]
use crate::ids::*;

const DIALOGUE_GAPS: usize = 3;
/// Cards in the memory game: this many Polish/English pairs.
pub(crate) const MEMORY_PAIRS: usize = 6;
/// Sentences in a fill-the-gaps exercise.
pub(crate) const GAP_SENTENCES: usize = 4;
/// Once an item has survived this many correct reps we stop showing multiple
/// choice and make the learner produce it from memory.
const TYPING_THRESHOLD: i64 = 3;

pub struct WordRow {
    pub id: WordId,
    pub pl: String,
    pub meaning: String,
    pub pos: String,
    pub reps: i64,
    /// Its picture, when it has one.
    pub image: Option<FileId>,
}

pub struct SentenceRow {
    pub id: SentenceId,
    pub pl: String,
    pub meaning: String,
    /// Its words in another form than their dictionary one (tiles.rs).
    pub endings: Vec<crate::tiles::Ending>,
}

pub fn load_course(conn: &mut Conn, course_id: CourseId) -> Result<Course> {
    courses::table
        .find(course_id)
        .select(Course::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| crate::error::Error::msg(format!("There is no course with id {course_id}.")))
}

/// A course's dialogue: the lines of its conversation drills, in order.
pub fn dialogue_lines(conn: &mut Conn, course_id: CourseId) -> Result<Vec<DialogueLine>> {
    Ok(course_lines::table
        .filter(course_lines::course_id.eq(course_id))
        .order(course_lines::ord)
        .select((course_lines::ord, course_lines::speaker, course_lines::gender, course_lines::pl, course_lines::en))
        .load(conn)?)
}

/// The dialogue one lesson owns.
pub fn lesson_lines(conn: &mut Conn, lesson_id: LessonId) -> Result<Vec<DialogueLine>> {
    Ok(conversation_lines::table
        .filter(conversation_lines::lesson_id.eq(lesson_id))
        .order((conversation_lines::ord, conversation_lines::id))
        .select(DialogueLine::as_select())
        .load(conn)?)
}

/// The kinds of item a learner has met (has a review row for), by id.
fn met(conn: &mut Conn, kind: &str, ids: &[String]) -> Result<std::collections::HashMap<String, i64>> {
    Ok(reviews::table
        .filter(reviews::kind.eq(kind))
        .filter(reviews::item_id.eq_any(ids))
        .select((reviews::item_id, reviews::reps))
        .load::<(String, i64)>(conn)?
        .into_iter()
        .collect())
}

/// The courses a lesson is listed in.
fn courses_of(conn: &mut Conn, lesson_id: LessonId) -> Result<Vec<CourseId>> {
    Ok(course_lessons::table
        .filter(course_lessons::lesson_id.eq(lesson_id))
        .select(course_lessons::course_id)
        .load(conn)?)
}

pub(crate) fn word_intro(w: &WordRow) -> Exercise {
    Exercise {
        key: format!("w{}-intro", w.id),
        kind: "intro".into(),
        item_kind: "word".into(),
        item_id: w.id.to_string(),
        is_new: true,
        prompt: w.pl.clone(),
        hint: pretty_pos(&w.pos),
        answer: w.meaning.clone(),
        options: vec![],
        tokens: vec![],
        gaps: vec![],
        pairs: vec![],
        image: None,
        option_images: vec![],
        context: vec![],
        endings: vec![],
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
    }
}

pub(crate) fn typing(w: &WordRow) -> Exercise {
    Exercise {
        key: format!("w{}-type", w.id),
        kind: "type".into(),
        item_kind: "word".into(),
        item_id: w.id.to_string(),
        is_new: false,
        prompt: w.meaning.clone(),
        hint: pretty_pos(&w.pos),
        answer: w.pl.clone(),
        options: vec![],
        tokens: vec![],
        gaps: vec![],
        pairs: vec![],
        image: None,
        option_images: vec![],
        context: vec![],
        endings: vec![],
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
    }
}

pub(crate) fn sentence_intro(s: &SentenceRow) -> Exercise {
    Exercise {
        key: format!("s{}-intro", s.id),
        kind: "sentenceIntro".into(),
        item_kind: "sentence".into(),
        item_id: s.id.to_string(),
        is_new: true,
        prompt: s.pl.clone(),
        hint: String::new(),
        answer: s.meaning.clone(),
        options: vec![],
        tokens: vec![],
        gaps: vec![],
        pairs: vec![],
        image: None,
        option_images: vec![],
        context: vec![],
        endings: vec![],
        pl: s.pl.clone(),
        meaning: s.meaning.clone(),
    }
}

/// The practice step for a word the learner has met: harder the better they
/// know it — recognise it, then pick it, then type it from memory.
pub(crate) fn practise_word(w: &WordRow, pool: &Distractors, rng: &mut impl rand::Rng) -> Exercise {
    if w.reps >= TYPING_THRESHOLD {
        typing(w)
    } else if w.reps >= 1 {
        produce(w, pool, rng)
    } else {
        recognise(w, pool, rng)
    }
}

pub(crate) fn lesson_words(conn: &mut Conn, lesson_id: LessonId) -> Result<Vec<(WordRow, bool)>> {
    let rows: Vec<(WordId, String, String, String, Option<FileId>)> = words::table
        .filter(words::lesson_id.eq(lesson_id))
        .order((words::ord, words::id))
        .select((words::id, words::pl, words::en, words::pos, words::image_file_id))
        .load(conn)?;
    let reps = met(conn, "word", &rows.iter().map(|r| r.0.to_string()).collect::<Vec<_>>())?;
    Ok(rows
        .into_iter()
        .map(|(id, pl, meaning, pos, image)| {
            let seen = reps.get(&id.to_string()).copied();
            (WordRow { id, pl, meaning, pos, reps: seen.unwrap_or(0), image }, seen.is_none())
        })
        .collect())
}

pub(crate) fn lesson_sentences(conn: &mut Conn, lesson_id: LessonId) -> Result<Vec<(SentenceRow, bool)>> {
    let rows: Vec<(SentenceId, String, String, String)> = sentences::table
        .filter(sentences::lesson_id.eq(lesson_id))
        .order((sentences::ord, sentences::id))
        .select((sentences::id, sentences::pl, sentences::en, sentences::endings))
        .load(conn)?;
    let seen = met(conn, "sentence", &rows.iter().map(|r| r.0.to_string()).collect::<Vec<_>>())?;
    Ok(rows
        .into_iter()
        .map(|(id, pl, meaning, endings)| {
            let new = !seen.contains_key(&id.to_string());
            (SentenceRow { id, pl, meaning, endings: crate::tiles::parse(&endings) }, new)
        })
        .collect())
}

/// The exercises of one drill lesson. Small on purpose: a low lesson is a
/// minute or two of taps, never the whole course at once.
/// What a drill has to work with: the lesson, its course, its words and
/// sentences (each with whether it is new), and decoys for choices. Each
/// lesson type builds its exercises from this (see lesson_types).
pub struct Drill<'a> {
    pub lesson: &'a crate::lessons::Lesson,
    pub course: &'a Course,
    pub words: Vec<(WordRow, bool)>,
    pub sentences: Vec<(SentenceRow, bool)>,
    pub pool: Distractors,
}

/// A drill's exercises, and the dialogue it shows (conversation drills).
#[derive(Default)]
pub struct Built {
    pub exercises: Vec<Exercise>,
    pub dialogue: Vec<DialogueLine>,
}

/// The exercises of one drill lesson, built by its type. Small on purpose:
/// a low lesson is a minute or two of taps, never the whole course at once.
pub fn build_lesson(conn: &mut Conn, lesson_id: LessonId) -> Result<SessionPlan> {
    let lesson = crate::lessons::load(conn, lesson_id)?;
    let lesson_type = crate::lesson_types::get(&lesson.lesson_type)?;
    let crate::lesson_types::Run::Exercises(build) = lesson_type.run else {
        return Err(crate::error::Error::msg("This lesson is a conversation with the examiner, not a drill."));
    };
    let course_id = lesson
        .course_id
        .ok_or_else(|| crate::error::Error::msg("This drill has no words to practise."))?;
    let course = load_course(conn, course_id)?;
    let today = db::today(conn);
    let drill = Drill {
        lesson: &lesson,
        course: &course,
        words: lesson_words(conn, lesson_id)?,
        sentences: lesson_sentences(conn, lesson_id)?,
        pool: distractor_pool(conn, lesson_id)?,
    };
    let built = build(conn, &drill)?;
    let new_words = drill.words.iter().filter(|(_, n)| *n).count();
    let new_sentences = drill.sentences.iter().filter(|(_, n)| *n).count();
    let (review_words, review_sentences) = (drill.words.len() - new_words, drill.sentences.len() - new_sentences);
    Ok(SessionPlan {
        day: today,
        course,
        dialogue: built.dialogue,
        new_words,
        review_words,
        new_sentences,
        review_sentences,
        exercises: built.exercises,
        already_completed: false,
        lesson_id,
        extra: crate::lessons::is_extra_practice(&lesson, today),
    })
}

/// A conversation drill's opening card: read the whole dialogue.
pub(crate) fn dialogue_read(lesson_id: LessonId, course: &Course) -> Exercise {
    Exercise {
        key: format!("d{lesson_id}-read"),
        kind: "dialogueRead".into(),
        item_kind: "dialogue".into(),
        item_id: lesson_id.to_string(),
        is_new: true,
        prompt: course.title.clone(),
        hint: course.summary.clone(),
        answer: String::new(),
        options: vec![],
        tokens: vec![],
        gaps: vec![],
        pairs: vec![],
        image: None,
        option_images: vec![],
        context: vec![],
        endings: vec![],
        pl: course.title_pl.clone(),
        meaning: course.summary.clone(),
    }
}

/// Picture lessons: a new word, seen, heard and read at once. Its English is
/// shown with it.
pub(crate) fn picture_intro(w: &WordRow) -> Exercise {
    Exercise { key: format!("w{}-picture-intro", w.id), kind: "pictureIntro".into(), image: w.image, ..word_intro(w) }
}

/// The practice step for a pictured word: see the picture and pick the word,
/// or see the word and pick the picture — in turn, as the word comes back.
/// With too few pictures to choose from, the picture is shown and the word
/// picked.
pub(crate) fn practise_picture(w: &WordRow, pool: &Distractors, images: &[(String, FileId)], rng: &mut impl rand::Rng) -> Exercise {
    let kind = if w.reps % 2 == 0 { "picturePick" } else { "wordPick" };
    let base = Exercise {
        key: format!("w{}-{kind}", w.id),
        kind: kind.into(),
        is_new: false,
        image: w.image,
        answer: w.pl.clone(),
        prompt: w.pl.clone(),
        ..word_intro(w)
    };
    match picture_options(w, images, rng) {
        Some((options, option_images)) if kind == "wordPick" => Exercise { options, option_images, ..base },
        _ => Exercise { kind: "picturePick".into(), key: format!("w{}-picturePick", w.id), options: options(&w.pl, &pool.polish, rng), ..base },
    }
}

/// Four pictures to choose from: the word's and three others', each of a
/// different word. None with fewer than four.
pub(crate) fn picture_options(w: &WordRow, images: &[(String, FileId)], rng: &mut impl rand::Rng) -> Option<(Vec<String>, Vec<Option<FileId>>)> {
    let mut decoys: Vec<&(String, FileId)> =
        images.iter().filter(|(pl, image)| pl != &w.pl && Some(*image) != w.image).collect();
    decoys.shuffle(rng);
    let mut chosen: Vec<(String, FileId)> = Vec::new();
    for (pl, image) in decoys {
        if chosen.iter().all(|(p, i)| p != pl && i != image) {
            chosen.push((pl.clone(), *image));
        }
        if chosen.len() == 3 {
            break;
        }
    }
    if chosen.len() < 3 {
        return None;
    }
    chosen.push((w.pl.clone(), w.image?));
    chosen.shuffle(rng);
    Some((chosen.iter().map(|(pl, _)| pl.clone()).collect(), chosen.iter().map(|(_, i)| Some(*i)).collect()))
}

/// The pictured words of every course the lesson is in: decoys for the
/// picture choices.
pub(crate) fn picture_pool(conn: &mut Conn, lesson_id: LessonId) -> Result<Vec<(String, FileId)>> {
    let courses = courses_of(conn, lesson_id)?;
    let ids: Vec<WordId> = course_words::table
        .filter(course_words::course_id.eq_any(courses))
        .select(course_words::id)
        .load(conn)?;
    let rows: Vec<(String, Option<FileId>)> = words::table
        .filter(words::id.eq_any(ids).or(words::lesson_id.eq(lesson_id)))
        .filter(words::image_file_id.is_not_null())
        .select((words::pl, words::image_file_id))
        .load(conn)?;
    Ok(rows.into_iter().filter_map(|(pl, image)| image.map(|i| (pl, i))).collect())
}

/// The memory round of a picture lesson: pictures to match with their words.
pub(crate) fn picture_memory(words: &[&WordRow], key: String, item_id: LessonId, rng: &mut impl rand::Rng) -> Option<Exercise> {
    let mut pool: Vec<&&WordRow> = words.iter().collect();
    pool.shuffle(rng);
    let mut pairs: Vec<Pair> = Vec::new();
    for w in pool {
        if pairs.iter().any(|p| p.pl == w.pl || p.image == w.image) {
            continue;
        }
        pairs.push(Pair { pl: w.pl.clone(), en: short_gloss(&w.meaning), image: w.image });
        if pairs.len() == MEMORY_PAIRS {
            break;
        }
    }
    if pairs.len() < 4 {
        return None;
    }
    Some(Exercise {
        key,
        kind: "memory".into(),
        item_kind: "set".into(),
        item_id: item_id.to_string(),
        is_new: false,
        prompt: String::new(),
        hint: String::new(),
        answer: String::new(),
        options: vec![],
        tokens: vec![],
        pl: String::new(),
        meaning: String::new(),
        gaps: vec![],
        pairs,
        image: None,
        option_images: vec![],
        context: vec![],
        endings: vec![],
    })
}

pub struct Distractors {
    pub meanings: Vec<String>,
    pub polish: Vec<String>,
}

/// Decoys come from the words of every course the lesson is in.
pub(crate) fn distractor_pool(conn: &mut Conn, lesson_id: LessonId) -> Result<Distractors> {
    let courses = courses_of(conn, lesson_id)?;
    let rows: Vec<(String, String)> = course_words::table
        .filter(course_words::course_id.eq_any(courses))
        .select((course_words::pl, course_words::en))
        .distinct()
        .load(conn)?;
    Ok(Distractors {
        polish: rows.iter().map(|(pl, _)| pl.clone()).collect(),
        meanings: rows.into_iter().map(|(_, m)| m).collect(),
    })
}

/// Builds a four-way choice: the answer plus three distinct decoys.
fn options(answer: &str, pool: &[String], rng: &mut impl rand::Rng) -> Vec<String> {
    let mut decoys: Vec<String> = pool
        .iter()
        .filter(|candidate| candidate.as_str() != answer)
        .cloned()
        .collect();
    decoys.shuffle(rng);
    decoys.truncate(3);

    let mut all = vec![answer.to_string()];
    all.extend(decoys);
    all.shuffle(rng);
    all
}

pub(crate) fn recognise(w: &WordRow, pool: &Distractors, rng: &mut impl rand::Rng) -> Exercise {
    Exercise {
        key: format!("w{}-recognise", w.id),
        kind: "recognise".into(),
        item_kind: "word".into(),
        item_id: w.id.to_string(),
        is_new: false,
        prompt: w.pl.clone(),
        hint: pretty_pos(&w.pos),
        answer: w.meaning.clone(),
        options: options(&w.meaning, &pool.meanings, rng),
        tokens: vec![],
        gaps: vec![],
        pairs: vec![],
        image: None,
        option_images: vec![],
        context: vec![],
        endings: vec![],
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
    }
}

pub(crate) fn produce(w: &WordRow, pool: &Distractors, rng: &mut impl rand::Rng) -> Exercise {
    Exercise {
        key: format!("w{}-produce", w.id),
        kind: "produce".into(),
        item_kind: "word".into(),
        item_id: w.id.to_string(),
        is_new: false,
        prompt: w.meaning.clone(),
        hint: pretty_pos(&w.pos),
        answer: w.pl.clone(),
        options: options(&w.pl, &pool.polish, rng),
        tokens: vec![],
        gaps: vec![],
        pairs: vec![],
        image: None,
        option_images: vec![],
        context: vec![],
        endings: vec![],
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
    }
}

/// Build the sentence from its shuffled words. Words that stand in another
/// form than their dictionary one (when the sentence has them: tiles.rs) are
/// laid out in their dictionary form, with the endings to pick from.
pub(crate) fn word_order(s: &SentenceRow) -> Exercise {
    let rng = &mut rand::rng();
    let mut tokens: Vec<String> = s.pl.split_whitespace().map(str::to_string).collect();
    tokens.shuffle(rng);
    let endings = crate::tiles::tile_endings(&tokens, &s.endings, rng);
    Exercise {
        endings,
        key: format!("s{}-order", s.id),
        kind: "wordOrder".into(),
        item_kind: "sentence".into(),
        item_id: s.id.to_string(),
        is_new: false,
        prompt: s.meaning.clone(),
        hint: String::new(),
        answer: s.pl.clone(),
        options: vec![],
        tokens,
        pl: s.pl.clone(),
        meaning: s.meaning.clone(),
        gaps: vec![],
        pairs: vec![],
        image: None,
        option_images: vec![],
        context: vec![],
    }
}

/// How many lines of the conversation a gap shows before it.
const DIALOGUE_CONTEXT: usize = 3;

/// "What comes next?" for a few lines of a dialogue. It asks what you can
/// tell from understanding the conversation, not from remembering the
/// script:
/// - gaps after a question come first: the right line answers it;
/// - the lines just before the gap are shown, so you know where you are;
/// - the wrong options are lines from far away in the dialogue — never the
///   ones around the gap, which could follow just as well.
pub(crate) fn dialogue_gaps(
    dialogue: &[DialogueLine],
    lesson_id: LessonId,
    rng: &mut impl rand::Rng,
) -> Vec<Exercise> {
    if dialogue.len() < 5 {
        return vec![];
    }
    let asks = |line: &DialogueLine| line.pl.trim_end().ends_with('?');
    let mut answers: Vec<usize> = (1..dialogue.len()).filter(|&i| asks(&dialogue[i - 1])).collect();
    let mut others: Vec<usize> = (1..dialogue.len()).filter(|&i| !asks(&dialogue[i - 1])).collect();
    answers.shuffle(rng);
    others.shuffle(rng);
    let mut indices: Vec<usize> = answers.into_iter().chain(others).take(DIALOGUE_GAPS).collect();
    indices.sort_unstable();

    indices
        .into_iter()
        .map(|i| {
            let line = &dialogue[i];
            let previous = &dialogue[i - 1];
            Exercise {
                key: format!("d{lesson_id}-gap{i}"),
                kind: "dialogueGap".into(),
                item_kind: "dialogue".into(),
                item_id: lesson_id.to_string(),
                is_new: false,
                prompt: previous.pl.clone(),
                hint: format!("{} — {}", previous.speaker, line.speaker),
                answer: line.pl.clone(),
                options: gap_options(dialogue, i, rng),
                tokens: vec![],
                gaps: vec![],
                pairs: vec![],
                image: None,
                option_images: vec![],
                context: dialogue[i.saturating_sub(DIALOGUE_CONTEXT)..i].to_vec(),
                endings: vec![],
                pl: line.pl.clone(),
                meaning: line.meaning.clone(),
            }
        })
        .collect()
}

/// The right line and three wrong ones for the gap at `at`: the farthest
/// lines from the gap first (they belong to another part of the
/// conversation), never the two on either side of it while others are left.
fn gap_options(dialogue: &[DialogueLine], at: usize, rng: &mut impl rand::Rng) -> Vec<String> {
    let answer = &dialogue[at].pl;
    let mut candidates: Vec<(usize, &str)> = dialogue
        .iter()
        .enumerate()
        .filter(|(j, line)| *j != at && line.pl != *answer)
        .map(|(j, line)| (j.abs_diff(at), line.pl.as_str()))
        .collect();
    candidates.shuffle(rng);
    // Far ones first; within the near ones (two lines either side), the
    // farther first too.
    candidates.sort_by_key(|(distance, _)| std::cmp::Reverse((*distance > 2, *distance)));
    let mut options: Vec<String> = vec![answer.clone()];
    for (_, text) in candidates {
        if options.len() == 4 {
            break;
        }
        if !options.iter().any(|o| o == text) {
            options.push(text.to_string());
        }
    }
    options.shuffle(rng);
    options
}

/// The first of a gloss's alternatives: "hi, bye" becomes "hi", so a card stays short.
fn short_gloss(meaning: &str) -> String {
    meaning.split([',', ';']).next().unwrap_or(meaning).trim().to_string()
}

/// Up to `count` Polish/English pairs from `words`, in their order, no two
/// alike on either side — two cards that read the same would make a
/// matching game unwinnable. `pictures`: a pictured word brings its picture
/// for its second card (in place of the English), no picture twice.
pub(crate) fn pick_pairs(words: &[&WordRow], count: usize, pictures: bool) -> Vec<Pair> {
    let mut pairs: Vec<Pair> = Vec::new();
    for w in words {
        let en = short_gloss(&w.meaning);
        let image = if pictures { w.image } else { None };
        if en.is_empty()
            || pairs.iter().any(|p| p.en.eq_ignore_ascii_case(&en) || p.pl == w.pl || (image.is_some() && p.image == image))
        {
            continue;
        }
        pairs.push(Pair { pl: w.pl.clone(), en, image });
        if pairs.len() == count {
            break;
        }
    }
    pairs
}

/// Face-down cards: find each Polish word's English partner.
pub(crate) fn memory_game(words: &[&WordRow], key: String, item_id: LessonId, rng: &mut impl rand::Rng) -> Option<Exercise> {
    let mut pool: Vec<&WordRow> = words.to_vec();
    pool.shuffle(rng);
    let pairs = pick_pairs(&pool, MEMORY_PAIRS, false);
    if pairs.len() < 4 {
        return None;
    }
    Some(Exercise {
        key,
        kind: "memory".into(),
        item_kind: "set".into(),
        item_id: item_id.to_string(),
        is_new: false,
        prompt: String::new(),
        hint: String::new(),
        answer: String::new(),
        options: vec![],
        tokens: vec![],
        pl: String::new(),
        meaning: String::new(),
        gaps: vec![],
        pairs,
        image: None,
        option_images: vec![],
        context: vec![],
        endings: vec![],
    })
}

/// Splits "Anna." into ("", "Anna", "."), so punctuation stays out of the gap.
pub(crate) fn split_punctuation(token: &str) -> (&str, &str, &str) {
    let start = token.find(|c: char| c.is_alphanumeric()).unwrap_or(token.len());
    let end = token
        .rfind(|c: char| c.is_alphanumeric())
        .map(|i| i + token[i..].chars().next().map_or(1, char::len_utf8))
        .unwrap_or(start);
    (&token[..start], &token[start..end.max(start)], &token[end.max(start)..])
}

/// Takes the most useful word out of a sentence: one of the course's own
/// words if it has one, else its longest word.
fn make_gap(pl: &str, meaning: &str, course_words: &std::collections::HashSet<String>) -> Option<Gap> {
    let tokens: Vec<&str> = pl.split_whitespace().collect();
    if tokens.len() < 3 {
        return None;
    }
    let candidates: Vec<(usize, &str)> = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| (i, split_punctuation(t).1))
        .filter(|(_, word)| word.chars().count() >= 3 && word.chars().all(char::is_alphabetic))
        .collect();
    let (index, _) = candidates
        .iter()
        .find(|(_, word)| course_words.contains(&word.to_lowercase()))
        .or_else(|| candidates.iter().max_by_key(|(_, word)| word.chars().count()))?;
    let (prefix, word, suffix) = split_punctuation(tokens[*index]);
    let mut before = tokens[..*index].join(" ");
    if !before.is_empty() {
        before.push(' ');
    }
    before.push_str(prefix);
    let mut after = suffix.to_string();
    if *index + 1 < tokens.len() {
        after.push(' ');
        after.push_str(&tokens[*index + 1..].join(" "));
    }
    Some(Gap { before, answer: word.to_string(), after, meaning: meaning.to_string() })
}

/// A few of the course's sentences with a word missing from each, and a word
/// bank of the missing words plus two that do not belong.
pub(crate) fn gap_fill(
    conn: &mut Conn,
    lesson_id: LessonId,
    session: &[&SentenceRow],
    // Only the given sentences, rather than topping up from the course.
    only_session: bool,
    rng: &mut impl rand::Rng,
) -> Result<Option<Exercise>> {
    let courses = courses_of(conn, lesson_id)?;
    let mut course_sentences: Vec<(String, String)> = course_sentences::table
        .filter(course_sentences::course_id.eq_any(&courses))
        .select((course_sentences::pl, course_sentences::en))
        .distinct()
        .load(conn)?;
    let course_words: HashSet<String> = course_words::table
        .filter(course_words::course_id.eq_any(&courses))
        .select(course_words::pl)
        .distinct()
        .load::<String>(conn)?
        .into_iter()
        .filter(|w| !w.contains(' '))
        .map(|w| w.to_lowercase())
        .collect();

    // Today's sentences first, then the rest of the course in random order.
    course_sentences.shuffle(rng);
    let mut ordered: Vec<(String, String)> =
        session.iter().map(|s| (s.pl.clone(), s.meaning.clone())).collect();
    for sentence in course_sentences.iter().filter(|_| !only_session) {
        if !ordered.iter().any(|(pl, _)| pl == &sentence.0) {
            ordered.push(sentence.clone());
        }
    }

    let mut gaps: Vec<Gap> = Vec::new();
    for (pl, en) in &ordered {
        if let Some(gap) = make_gap(pl, en, &course_words) {
            if gaps.iter().any(|g| g.answer.eq_ignore_ascii_case(&gap.answer)) {
                continue;
            }
            gaps.push(gap);
            if gaps.len() == GAP_SENTENCES {
                break;
            }
        }
    }
    if gaps.len() < 2 {
        return Ok(None);
    }

    let mut decoys: Vec<String> = course_sentences
        .iter()
        .flat_map(|(pl, _)| pl.split_whitespace().map(|t| split_punctuation(t).1.to_string()).collect::<Vec<_>>())
        .filter(|w| w.chars().count() >= 3 && w.chars().all(char::is_alphabetic))
        .filter(|w| !gaps.iter().any(|g| g.answer.eq_ignore_ascii_case(w)))
        .collect();
    decoys.sort();
    decoys.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    decoys.shuffle(rng);
    let mut bank: Vec<String> = gaps.iter().map(|g| g.answer.clone()).collect();
    bank.extend(decoys.into_iter().take(2));
    bank.shuffle(rng);

    Ok(Some(Exercise {
        key: format!("t{lesson_id}-gaps"),
        kind: "gapFill".into(),
        item_kind: "set".into(),
        item_id: lesson_id.to_string(),
        is_new: false,
        prompt: String::new(),
        hint: String::new(),
        answer: gaps.iter().map(|g| g.answer.as_str()).collect::<Vec<_>>().join(" | "),
        options: bank,
        tokens: vec![],
        pl: String::new(),
        meaning: String::new(),
        gaps,
        pairs: vec![],
        image: None,
        option_images: vec![],
        context: vec![],
        endings: vec![],
    }))
}

fn pretty_pos(code: &str) -> String {
    match code {
        "n-m" => "noun, masculine",
        "n-f" => "noun, feminine",
        "n-n" => "noun, neuter",
        "n-pl" => "noun, plural",
        "v" => "verb",
        "adj" => "adjective",
        "adv" => "adverb",
        "num" => "number",
        "pron" => "pronoun",
        "prep" => "preposition",
        "conj" => "conjunction",
        "part" => "particle",
        "interj" => "interjection",
        "phr" => "phrase",
        "n" => "noun",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punctuation_stays_outside_the_gap() {
        assert_eq!(split_punctuation("Anna."), ("", "Anna", "."));
        assert_eq!(split_punctuation("„Cześć,"), ("„", "Cześć", ","));
        assert_eq!(split_punctuation("—"), ("—", "", ""));
    }

    #[test]
    fn a_gap_prefers_a_course_word() {
        let words: std::collections::HashSet<String> = ["kawę".to_string()].into();
        let gap = make_gap("Poproszę kawę z mlekiem.", "A coffee with milk, please.", &words).unwrap();
        assert_eq!(gap.answer, "kawę");
        assert_eq!(gap.before, "Poproszę ");
        assert_eq!(gap.after, " z mlekiem.");
        // Without a course word, the longest word goes.
        let gap = make_gap("Skąd pan pochodzi?", "Where are you from?", &Default::default()).unwrap();
        assert_eq!(gap.answer, "pochodzi");
        assert_eq!(gap.after, "?");
        assert!(make_gap("Dzień dobry", "Hello", &Default::default()).is_none());
    }
}

#[cfg(test)]
mod dialogue_gap_tests {
    use super::*;
    use rand::SeedableRng;

    fn line(ord: i64, speaker: &str, pl: &str) -> DialogueLine {
        DialogueLine { ord, speaker: speaker.into(), gender: "f".into(), pl: pl.into(), meaning: String::new() }
    }

    #[test]
    fn gaps_answer_questions_show_context_and_keep_neighbours_out() {
        let dialogue = vec![
            line(0, "Ola", "Dzień dobry!"),
            line(1, "Jan", "Dzień dobry. Co podać?"),
            line(2, "Ola", "Poproszę kawę."),
            line(3, "Jan", "Z mlekiem?"),
            line(4, "Ola", "Tak, z mlekiem."),
            line(5, "Jan", "Proszę bardzo."),
            line(6, "Ola", "Ile płacę?"),
            line(7, "Jan", "Dziesięć złotych."),
            line(8, "Ola", "Dziękuję, do widzenia."),
        ];
        for seed in 0..20 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let gaps = dialogue_gaps(&dialogue, LessonId::derived("test", "dialogue"), &mut rng);
            assert_eq!(gaps.len(), DIALOGUE_GAPS);
            for gap in &gaps {
                assert!(gap.prompt.ends_with('?'), "after a question: {}", gap.prompt);
                assert_eq!(gap.context.last().map(|l| l.pl.as_str()), Some(gap.prompt.as_str()));
                assert!(gap.context.len() <= DIALOGUE_CONTEXT);
                let at = dialogue.iter().position(|l| l.pl == gap.answer).unwrap();
                assert_eq!(gap.options.len(), 4);
                for option in gap.options.iter().filter(|o| **o != gap.answer) {
                    let j = dialogue.iter().position(|l| l.pl == *option).unwrap();
                    assert!(j.abs_diff(at) > 2, "{option} is right next to the gap");
                }
            }
        }
    }
}
