//! Builds the exercises of a drill block: a handful of words, sentences or a
//! dialogue, practised in the block's format.

use std::collections::HashSet;

use diesel::prelude::*;
use rand::seq::SliceRandom;

use crate::db::{self, Conn};
use crate::schema::{conversation_lines, course_blocks, course_lines, course_sentences, course_words, reviews, sentences, topics, words};
use crate::error::Result;
use crate::models::{DialogueLine, Exercise, Gap, Pair, SessionPlan, Topic};

const DIALOGUE_GAPS: usize = 3;
/// Cards in the memory game: this many Polish/English pairs.
const MEMORY_PAIRS: usize = 6;
/// Sentences in a fill-the-gaps exercise.
const GAP_SENTENCES: usize = 4;
/// Once an item has survived this many correct reps we stop showing multiple
/// choice and make the learner produce it from memory.
const TYPING_THRESHOLD: i64 = 3;

struct WordRow {
    id: i64,
    pl: String,
    meaning: String,
    pos: String,
    reps: i64,
}

struct SentenceRow {
    id: i64,
    pl: String,
    meaning: String,
}

pub fn load_topic(conn: &mut Conn, topic_id: i64) -> Result<Topic> {
    topics::table
        .find(topic_id)
        .select(Topic::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| crate::error::Error::msg(format!("There is no topic with id {topic_id}.")))
}

/// A course's dialogue: the lines of its conversation drills, in order.
pub fn dialogue_lines(conn: &mut Conn, topic_id: i64) -> Result<Vec<DialogueLine>> {
    Ok(course_lines::table
        .filter(course_lines::topic_id.eq(topic_id))
        .order(course_lines::ord)
        .select((course_lines::ord, course_lines::speaker, course_lines::gender, course_lines::pl, course_lines::en))
        .load(conn)?)
}

/// The dialogue one block owns.
pub fn block_lines(conn: &mut Conn, block_id: i64) -> Result<Vec<DialogueLine>> {
    Ok(conversation_lines::table
        .filter(conversation_lines::block_id.eq(block_id))
        .order((conversation_lines::ord, conversation_lines::id))
        .select(DialogueLine::as_select())
        .load(conn)?)
}

/// The kinds of item a learner has met (has a review row for), by id.
fn met(conn: &mut Conn, kind: &str, ids: &[i64]) -> Result<std::collections::HashMap<i64, i64>> {
    Ok(reviews::table
        .filter(reviews::kind.eq(kind))
        .filter(reviews::item_id.eq_any(ids))
        .select((reviews::item_id, reviews::reps))
        .load::<(i64, i64)>(conn)?
        .into_iter()
        .collect())
}

/// The courses a block is listed in.
fn courses_of(conn: &mut Conn, block_id: i64) -> Result<Vec<i64>> {
    Ok(course_blocks::table
        .filter(course_blocks::block_id.eq(block_id))
        .select(course_blocks::topic_id)
        .load(conn)?)
}

fn word_intro(w: &WordRow) -> Exercise {
    Exercise {
        key: format!("w{}-intro", w.id),
        kind: "intro".into(),
        item_kind: "word".into(),
        item_id: w.id,
        is_new: true,
        prompt: w.pl.clone(),
        hint: pretty_pos(&w.pos),
        answer: w.meaning.clone(),
        options: vec![],
        tokens: vec![],
        gaps: vec![],
        pairs: vec![],
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
    }
}

fn typing(w: &WordRow) -> Exercise {
    Exercise {
        key: format!("w{}-type", w.id),
        kind: "type".into(),
        item_kind: "word".into(),
        item_id: w.id,
        is_new: false,
        prompt: w.meaning.clone(),
        hint: pretty_pos(&w.pos),
        answer: w.pl.clone(),
        options: vec![],
        tokens: vec![],
        gaps: vec![],
        pairs: vec![],
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
    }
}

fn sentence_intro(s: &SentenceRow) -> Exercise {
    Exercise {
        key: format!("s{}-intro", s.id),
        kind: "sentenceIntro".into(),
        item_kind: "sentence".into(),
        item_id: s.id,
        is_new: true,
        prompt: s.pl.clone(),
        hint: String::new(),
        answer: s.meaning.clone(),
        options: vec![],
        tokens: vec![],
        gaps: vec![],
        pairs: vec![],
        pl: s.pl.clone(),
        meaning: s.meaning.clone(),
    }
}

/// The practice step for a word the learner has met: harder the better they
/// know it — recognise it, then pick it, then type it from memory.
fn practise_word(w: &WordRow, pool: &Distractors, rng: &mut impl rand::Rng) -> Exercise {
    if w.reps >= TYPING_THRESHOLD {
        typing(w)
    } else if w.reps >= 1 {
        produce(w, pool, rng)
    } else {
        recognise(w, pool, rng)
    }
}

fn block_words(conn: &mut Conn, block_id: i64) -> Result<Vec<(WordRow, bool)>> {
    let rows: Vec<(i64, String, String, String)> = words::table
        .filter(words::block_id.eq(block_id))
        .order((words::ord, words::id))
        .select((words::id, words::pl, words::en, words::pos))
        .load(conn)?;
    let reps = met(conn, "word", &rows.iter().map(|r| r.0).collect::<Vec<_>>())?;
    Ok(rows
        .into_iter()
        .map(|(id, pl, meaning, pos)| {
            let seen = reps.get(&id).copied();
            (WordRow { id, pl, meaning, pos, reps: seen.unwrap_or(0) }, seen.is_none())
        })
        .collect())
}

fn block_sentences(conn: &mut Conn, block_id: i64) -> Result<Vec<(SentenceRow, bool)>> {
    let rows: Vec<(i64, String, String)> = sentences::table
        .filter(sentences::block_id.eq(block_id))
        .order((sentences::ord, sentences::id))
        .select((sentences::id, sentences::pl, sentences::en))
        .load(conn)?;
    let seen = met(conn, "sentence", &rows.iter().map(|r| r.0).collect::<Vec<_>>())?;
    Ok(rows
        .into_iter()
        .map(|(id, pl, meaning)| {
            let new = !seen.contains_key(&id);
            (SentenceRow { id, pl, meaning }, new)
        })
        .collect())
}

/// The exercises of one drill block. Small on purpose: a low block is a
/// minute or two of taps, never the whole topic at once.
pub fn build_block(conn: &mut Conn, block_id: i64) -> Result<SessionPlan> {
    let block = crate::blocks::load(conn, block_id)?;
    if block.kind != "drill" {
        return Err(crate::error::Error::msg("Study blocks are checked by the examiner, not drilled."));
    }
    let topic_id = block
        .topic_id
        .ok_or_else(|| crate::error::Error::msg("This drill has no words to practise."))?;
    let topic = load_topic(conn, topic_id)?;
    let today = db::today(conn);
    let pool = distractor_pool(conn, block_id)?;
    let rng = &mut rand::rng();

    let words = block_words(conn, block_id)?;
    let sentences = block_sentences(conn, block_id)?;
    let mut exercises: Vec<Exercise> = Vec::new();
    let mut dialogue = Vec::new();

    match block.format.as_str() {
        "words" | "typing" => {
            // New words are introduced first, in order; known ones are
            // practised in a shuffled order.
            let mut known = Vec::new();
            for (w, is_new) in &words {
                if *is_new {
                    exercises.push(word_intro(w));
                    exercises.push(if block.format == "typing" { typing(w) } else { recognise(w, &pool, rng) });
                } else {
                    known.push(if block.format == "typing" { typing(w) } else { practise_word(w, &pool, rng) });
                }
            }
            known.shuffle(rng);
            exercises.extend(known);
            if block.format == "words" {
                let all: Vec<&WordRow> = words.iter().map(|(w, _)| w).collect();
                exercises.extend(memory_game(&all, format!("b{block_id}-memory"), block_id, rng));
            }
        }
        "memory" => {
            let all: Vec<&WordRow> = words.iter().map(|(w, _)| w).collect();
            for (n, chunk) in all.chunks(MEMORY_PAIRS).enumerate() {
                exercises.extend(memory_game(chunk, format!("b{block_id}-memory-{n}"), block_id, rng));
            }
        }
        "sentences" | "gaps" => {
            if block.format == "sentences" {
                for (s, is_new) in &sentences {
                    if *is_new {
                        exercises.push(sentence_intro(s));
                    }
                    exercises.push(word_order(s));
                }
            }
            let rows: Vec<&SentenceRow> = sentences.iter().map(|(s, _)| s).collect();
            for chunk in rows.chunks(GAP_SENTENCES) {
                exercises.extend(gap_fill(conn, block_id, chunk, true, rng)?);
            }
        }
        "dialogue" => {
            dialogue = block_lines(conn, block_id)?;
            if !dialogue.is_empty() {
                exercises.push(Exercise {
                    key: format!("d{block_id}-read"),
                    kind: "dialogueRead".into(),
                    item_kind: "dialogue".into(),
                    item_id: block_id,
                    is_new: true,
                    prompt: topic.title.clone(),
                    hint: topic.summary.clone(),
                    answer: String::new(),
                    options: vec![],
                    tokens: vec![],
                    gaps: vec![],
                    pairs: vec![],
                    pl: topic.title_pl.clone(),
                    meaning: topic.summary.clone(),
                });
            }
            exercises.extend(dialogue_gaps(&dialogue, block_id, rng));
        }
        other => return Err(crate::error::Error::msg(format!("Unknown drill format '{other}'."))),
    }

    let new_words = words.iter().filter(|(_, n)| *n).count();
    let new_sentences = sentences.iter().filter(|(_, n)| *n).count();
    Ok(SessionPlan {
        day: today,
        topic,
        dialogue,
        new_words,
        review_words: words.len() - new_words,
        new_sentences,
        review_sentences: sentences.len() - new_sentences,
        exercises,
        already_completed: false,
        block_id,
        extra: crate::blocks::is_extra_practice(&block, today),
    })
}

struct Distractors {
    meanings: Vec<String>,
    polish: Vec<String>,
}

/// Decoys come from the words of every course the block is in.
fn distractor_pool(conn: &mut Conn, block_id: i64) -> Result<Distractors> {
    let courses = courses_of(conn, block_id)?;
    let rows: Vec<(String, String)> = course_words::table
        .filter(course_words::topic_id.eq_any(courses))
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

fn recognise(w: &WordRow, pool: &Distractors, rng: &mut impl rand::Rng) -> Exercise {
    Exercise {
        key: format!("w{}-recognise", w.id),
        kind: "recognise".into(),
        item_kind: "word".into(),
        item_id: w.id,
        is_new: false,
        prompt: w.pl.clone(),
        hint: pretty_pos(&w.pos),
        answer: w.meaning.clone(),
        options: options(&w.meaning, &pool.meanings, rng),
        tokens: vec![],
        gaps: vec![],
        pairs: vec![],
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
    }
}

fn produce(w: &WordRow, pool: &Distractors, rng: &mut impl rand::Rng) -> Exercise {
    Exercise {
        key: format!("w{}-produce", w.id),
        kind: "produce".into(),
        item_kind: "word".into(),
        item_id: w.id,
        is_new: false,
        prompt: w.meaning.clone(),
        hint: pretty_pos(&w.pos),
        answer: w.pl.clone(),
        options: options(&w.pl, &pool.polish, rng),
        tokens: vec![],
        gaps: vec![],
        pairs: vec![],
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
    }
}

fn word_order(s: &SentenceRow) -> Exercise {
    let mut tokens: Vec<String> = s.pl.split_whitespace().map(str::to_string).collect();
    tokens.shuffle(&mut rand::rng());
    Exercise {
        key: format!("s{}-order", s.id),
        kind: "wordOrder".into(),
        item_kind: "sentence".into(),
        item_id: s.id,
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
    }
}

/// Blanks out a few lines of the conversation and asks which one belongs.
fn dialogue_gaps(
    dialogue: &[DialogueLine],
    block_id: i64,
    rng: &mut impl rand::Rng,
) -> Vec<Exercise> {
    if dialogue.len() < 5 {
        return vec![];
    }
    let mut indices: Vec<usize> = (1..dialogue.len()).collect();
    indices.shuffle(rng);
    indices.truncate(DIALOGUE_GAPS);
    indices.sort_unstable();

    let all_polish: Vec<String> = dialogue.iter().map(|l| l.pl.clone()).collect();

    indices
        .into_iter()
        .map(|i| {
            let line = &dialogue[i];
            let previous = &dialogue[i - 1];
            Exercise {
                key: format!("d{block_id}-gap{i}"),
                kind: "dialogueGap".into(),
                item_kind: "dialogue".into(),
                item_id: block_id,
                is_new: false,
                prompt: previous.pl.clone(),
                hint: format!("{} — {}", previous.speaker, line.speaker),
                answer: line.pl.clone(),
                options: options(&line.pl, &all_polish, rng),
                tokens: vec![],
                gaps: vec![],
                pairs: vec![],
                pl: line.pl.clone(),
                meaning: line.meaning.clone(),
            }
        })
        .collect()
}

/// The first of a gloss's alternatives: "hi, bye" becomes "hi", so a card stays short.
fn short_gloss(meaning: &str) -> String {
    meaning.split([',', ';']).next().unwrap_or(meaning).trim().to_string()
}

/// Face-down cards: find each Polish word's English partner.
fn memory_game(words: &[&WordRow], key: String, item_id: i64, rng: &mut impl rand::Rng) -> Option<Exercise> {
    let mut pool: Vec<&&WordRow> = words.iter().collect();
    pool.shuffle(rng);
    let mut pairs: Vec<Pair> = Vec::new();
    for w in pool {
        let en = short_gloss(&w.meaning);
        // Two cards that read the same would make the game unwinnable.
        if en.is_empty() || pairs.iter().any(|p| p.en.eq_ignore_ascii_case(&en) || p.pl == w.pl) {
            continue;
        }
        pairs.push(Pair { pl: w.pl.clone(), en });
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
        item_id,
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
    })
}

/// Splits "Anna." into ("", "Anna", "."), so punctuation stays out of the gap.
fn split_punctuation(token: &str) -> (&str, &str, &str) {
    let start = token.find(|c: char| c.is_alphanumeric()).unwrap_or(token.len());
    let end = token
        .rfind(|c: char| c.is_alphanumeric())
        .map(|i| i + token[i..].chars().next().map_or(1, char::len_utf8))
        .unwrap_or(start);
    (&token[..start], &token[start..end.max(start)], &token[end.max(start)..])
}

/// Takes the most useful word out of a sentence: one of the topic's own
/// words if it has one, else its longest word.
fn make_gap(pl: &str, meaning: &str, topic_words: &std::collections::HashSet<String>) -> Option<Gap> {
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
        .find(|(_, word)| topic_words.contains(&word.to_lowercase()))
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

/// A few of the topic's sentences with a word missing from each, and a word
/// bank of the missing words plus two that do not belong.
fn gap_fill(
    conn: &mut Conn,
    block_id: i64,
    session: &[&SentenceRow],
    // Only the given sentences, rather than topping up from the topic.
    only_session: bool,
    rng: &mut impl rand::Rng,
) -> Result<Option<Exercise>> {
    let courses = courses_of(conn, block_id)?;
    let mut topic_sentences: Vec<(String, String)> = course_sentences::table
        .filter(course_sentences::topic_id.eq_any(&courses))
        .select((course_sentences::pl, course_sentences::en))
        .distinct()
        .load(conn)?;
    let topic_words: HashSet<String> = course_words::table
        .filter(course_words::topic_id.eq_any(&courses))
        .select(course_words::pl)
        .distinct()
        .load::<String>(conn)?
        .into_iter()
        .filter(|w| !w.contains(' '))
        .map(|w| w.to_lowercase())
        .collect();

    // Today's sentences first, then the rest of the topic in random order.
    topic_sentences.shuffle(rng);
    let mut ordered: Vec<(String, String)> =
        session.iter().map(|s| (s.pl.clone(), s.meaning.clone())).collect();
    for sentence in topic_sentences.iter().filter(|_| !only_session) {
        if !ordered.iter().any(|(pl, _)| pl == &sentence.0) {
            ordered.push(sentence.clone());
        }
    }

    let mut gaps: Vec<Gap> = Vec::new();
    for (pl, en) in &ordered {
        if let Some(gap) = make_gap(pl, en, &topic_words) {
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

    let mut decoys: Vec<String> = topic_sentences
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
        key: format!("t{block_id}-gaps"),
        kind: "gapFill".into(),
        item_kind: "set".into(),
        item_id: block_id,
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
    fn a_gap_prefers_a_topic_word() {
        let words: std::collections::HashSet<String> = ["kawę".to_string()].into();
        let gap = make_gap("Poproszę kawę z mlekiem.", "A coffee with milk, please.", &words).unwrap();
        assert_eq!(gap.answer, "kawę");
        assert_eq!(gap.before, "Poproszę ");
        assert_eq!(gap.after, " z mlekiem.");
        // Without a topic word, the longest word goes.
        let gap = make_gap("Skąd pan pochodzi?", "Where are you from?", &Default::default()).unwrap();
        assert_eq!(gap.answer, "pochodzi");
        assert_eq!(gap.after, "?");
        assert!(make_gap("Dzień dobry", "Hello", &Default::default()).is_none());
    }
}
