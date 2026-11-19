//! The words each answer involved (`attempt_items`), written with the
//! answer (learnlog.rs) in its transaction, so a word's statistics include
//! every time it appeared — on its own card, inside a sentence, a dialogue
//! line, a story or a song line, or as a wrong option beside another word.
//!
//! A sentence's words are found the way the app already finds them, best
//! first: the tutor's annotation of the sentence (sentence_tokens: the
//! word itself, or its dictionary form), the sentence's endings (tiles.rs:
//! "kawę" is a form of "kawa"), and the glossary matcher the lyrics use
//! (singalong.rs: an exact word, then a form sharing all but its ending).
//! Each link keeps the form as it appeared, its place in the sentence
//! (numbered as grammar::words_of numbers it), and — where the exercise
//! grades word by word (dictation, Say it, tiles) — that word's own result.

use std::collections::HashMap;

use diesel::prelude::*;

use crate::db::Conn;
use crate::error::Result;
#[allow(unused_imports)]
use crate::ids::*;
use crate::learnlog::NewAttempt;
use crate::schema::{attempt_items, conversation_lines, lessons, sentences, words};

/// Every role a link can have, and what it means.
pub const ROLES: &[(&str, &str)] = &[
    ("target", "the card's own word, sentence or line"),
    ("in_sentence", "a word of the sentence the card was about"),
    ("in_line", "a word of a dialogue line"),
    ("in_story", "a word of a story's sentence"),
    ("in_song", "a word of a song's line"),
    ("distractor", "a wrong option shown beside the answer"),
];

pub fn role_label(role: &str) -> &'static str {
    match role {
        "target" => "On its own",
        "in_sentence" => "In a sentence",
        "in_line" => "In a dialogue",
        "in_story" => "In a story",
        "in_song" => "In a song",
        "distractor" => "As a wrong option",
        _ => "Elsewhere",
    }
}

/// One link, before it is written.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub item_id: String,
    pub item_kind: &'static str,
    pub role: &'static str,
    pub form: String,
    pub position: i64,
    pub correct: Option<bool>,
}

/// Writes an answer's links (inside the answer's transaction).
pub fn write(conn: &mut Conn, attempt: AttemptId, a: &NewAttempt) -> Result<()> {
    for link in links_for(conn, a)? {
        diesel::insert_or_ignore_into(attempt_items::table)
            .values((
                attempt_items::attempt_id.eq(attempt),
                attempt_items::item_id.eq(&link.item_id),
                attempt_items::item_kind.eq(link.item_kind),
                attempt_items::role.eq(link.role),
                attempt_items::form.eq(&link.form),
                attempt_items::position.eq(link.position),
                attempt_items::correct.eq(link.correct.map(i64::from)),
            ))
            .execute(conn)?;
    }
    Ok(())
}

/// The role of a word inside text practised in a lesson of this type.
fn role_in(lesson_type: &str) -> &'static str {
    match lesson_type {
        "story" => "in_story",
        "listening" => "in_song",
        "dialogue" => "in_line",
        _ => "in_sentence",
    }
}

fn lesson_type(conn: &mut Conn, lesson: Option<LessonId>) -> Result<String> {
    Ok(match lesson {
        Some(id) => lessons::table.find(id).select(lessons::lesson_type).first(conn).optional()?.unwrap_or_default(),
        None => String::new(),
    })
}

/// Which links an answer makes.
pub fn links_for(conn: &mut Conn, a: &NewAttempt) -> Result<Vec<Link>> {
    let mut out: Vec<Link> = Vec::new();
    let kind = crate::ids::kind_of(a.item_id);
    match kind {
        Some("word") => {
            let Ok(id) = WordId::parse(a.item_id) else { return Ok(out) };
            let pl: Option<String> = words::table.find(id).select(words::pl).first(conn).optional()?;
            out.push(Link {
                item_id: a.item_id.into(),
                item_kind: "word",
                role: "target",
                form: pl.unwrap_or_else(|| a.expected.to_string()),
                position: 0,
                correct: Some(a.correct),
            });
            out.extend(distractors(conn, a, id)?);
        }
        Some("sentence") => {
            let Ok(id) = SentenceId::parse(a.item_id) else { return Ok(out) };
            let row: Option<(String, String, LessonId)> =
                sentences::table.find(id).select((sentences::pl, sentences::endings, sentences::lesson_id)).first(conn).optional()?;
            let Some((pl, endings, lesson)) = row else { return Ok(out) };
            out.push(Link { item_id: a.item_id.into(), item_kind: "sentence", role: "target", form: pl.clone(), position: 0, correct: Some(a.correct) });
            let role = role_in(&lesson_type(conn, Some(lesson))?);
            out.extend(words_in(conn, a, &pl, Some(id), &crate::tiles::parse(&endings), Some(lesson), role)?);
        }
        _ if a.exercise == "checkinMistake" => {
            out.extend(words_in(conn, a, a.expected, None, &[], a.lesson_id, "in_sentence")?);
        }
        Some("lesson") if a.item_kind == "dialogue" || a.exercise.starts_with("dialogue") => {
            // A dialogue card: the line it was about, and its words.
            let text = if a.pl.trim().is_empty() { a.expected } else { a.pl };
            if let Some(lesson) = a.lesson_id {
                let line: Option<LineId> = conversation_lines::table
                    .filter(conversation_lines::lesson_id.eq(lesson))
                    .filter(conversation_lines::pl.eq(text.trim()))
                    .select(conversation_lines::id)
                    .first(conn)
                    .optional()?;
                if let Some(line) = line {
                    out.push(Link { item_id: line.to_string(), item_kind: "line", role: "target", form: text.trim().into(), position: 0, correct: Some(a.correct) });
                }
            }
            out.extend(words_in(conn, a, text, None, &[], a.lesson_id, "in_line")?);
        }
        Some("lesson") if !a.pl.trim().is_empty() && a.exercise != "checkin" => {
            // A card about a whole lesson with Polish on it (a story
            // question, a scene card): the words in it.
            let role = role_in(&lesson_type(conn, a.lesson_id)?);
            let found = words_in(conn, a, a.pl, None, &[], a.lesson_id, role)?;
            if found.len() == 1 && crate::grammar::words_of(a.pl).len() == 1 {
                // One word on the card: it is the card's own.
                out.extend(found.into_iter().map(|l| Link { role: "target", correct: Some(a.correct), ..l }));
            } else {
                out.extend(found);
            }
        }
        _ => {}
    }
    Ok(out)
}

/// The wrong options beside a word that are words of the learner's too: a
/// distractor is "right" when it was not picked.
fn distractors(conn: &mut Conn, a: &NewAttempt, target: WordId) -> Result<Vec<Link>> {
    let wrong: Vec<&String> = a.options.iter().filter(|o| crate::practice::normalize(o) != crate::practice::normalize(a.expected)).collect();
    if wrong.is_empty() {
        return Ok(vec![]);
    }
    let values: Vec<String> = wrong.iter().map(|o| o.trim().to_string()).collect();
    let mut rows: Vec<(WordId, String, String, LessonId)> = words::table
        .filter(words::pl.eq_any(&values).or(words::en.eq_any(&values)))
        .filter(words::id.ne(target))
        .select((words::id, words::pl, words::en, words::lesson_id))
        .load(conn)?;
    // The lesson's own words first: the options come from around it.
    rows.sort_by_key(|(.., lesson)| Some(*lesson) != a.lesson_id);
    let mut out = Vec::new();
    for (i, option) in wrong.iter().enumerate() {
        let found = rows.iter().find(|(_, pl, en, _)| pl == option.trim() || en == option.trim());
        if let Some((id, pl, ..)) = found {
            if out.iter().any(|l: &Link| l.item_id == id.to_string()) {
                continue;
            }
            let picked = crate::practice::normalize(a.given) == crate::practice::normalize(option);
            out.push(Link {
                item_id: id.to_string(),
                item_kind: "word",
                role: "distractor",
                form: pl.clone(),
                position: crate::convert::count(i),
                correct: Some(!picked),
            });
        }
    }
    Ok(out)
}

/// The learner's words in a text, each with its form, place and own result.
fn words_in(
    conn: &mut Conn,
    a: &NewAttempt,
    text: &str,
    sentence: Option<SentenceId>,
    endings: &[crate::tiles::Ending],
    lesson: Option<LessonId>,
    role: &'static str,
) -> Result<Vec<Link>> {
    let tokens = crate::grammar::words_of(text);
    if tokens.is_empty() {
        return Ok(vec![]);
    }
    let annotated: HashMap<i64, crate::grammar::Token> = match sentence {
        Some(id) => crate::grammar::tokens_of(conn, id)?.into_iter().map(|t| (t.position, t)).collect(),
        None => HashMap::new(),
    };
    // Every word, the lesson's own first (the glossary takes the first match).
    let rows: Vec<(WordId, String, String, LessonId)> = words::table.select((words::id, words::pl, words::en, words::lesson_id)).load(conn)?;
    let (mut known, others): (Vec<_>, Vec<_>) = rows.into_iter().partition(|(.., l)| Some(*l) == lesson);
    known.extend(others);
    let known: Vec<crate::singalong::Known> = known.into_iter().map(|(id, pl, en, _)| (pl, en, Some(id), false)).collect();
    let by_pl: HashMap<String, WordId> = known.iter().rev().filter_map(|(pl, _, id, _)| Some((pl.to_lowercase(), (*id)?))).collect();
    let base_of: HashMap<String, String> = endings.iter().map(|e| (e.form.to_lowercase(), e.base.to_lowercase())).collect();
    let results = per_word(a, &tokens);

    let mut out = Vec::new();
    for (i, (written, lower)) in tokens.iter().enumerate() {
        let position = crate::convert::count(i);
        let annotation = annotated.get(&position);
        let word = annotation
            .and_then(|t| t.word_id)
            .or_else(|| annotation.and_then(|t| by_pl.get(&t.lemma.to_lowercase()).copied()))
            .or_else(|| base_of.get(lower).and_then(|base| by_pl.get(base).copied()))
            .or_else(|| crate::singalong::look_up(lower, &known).filter(|g| close_enough(lower, &g.pl)).and_then(|g| g.word_id));
        if let Some(word) = word {
            out.push(Link {
                item_id: word.to_string(),
                item_kind: "word",
                role,
                form: written.clone(),
                position,
                correct: results[i],
            });
        }
    }
    Ok(out)
}

/// The glossary's forms, held to a stricter bar for statistics: the form
/// must keep all but the last two letters of the dictionary word, and most
/// of it ("mlekiem" for mleko, "kawę" for kawa — but not "jest" for
/// jeszcze, which shares only "jes"). A wrong link would put one word's
/// answers on another.
fn close_enough(form: &str, base: &str) -> bool {
    let (f, b): (Vec<char>, Vec<char>) = (form.chars().collect(), base.to_lowercase().chars().collect());
    if f == b {
        return true;
    }
    let shared = f.iter().zip(&b).take_while(|(x, y)| x == y).count();
    shared + 2 >= b.len() && shared * 10 >= b.len() * 6
}

/// Each token's own result, where the exercise graded word by word: the
/// word-by-word results matched to the tokens in order. Without them, a
/// right answer had every word right; a wrong one says nothing per word.
fn per_word(a: &NewAttempt, tokens: &[(String, String)]) -> Vec<Option<bool>> {
    let graded = &a.mistake.words;
    if graded.is_empty() {
        return tokens.iter().map(|_| a.correct.then_some(true)).collect();
    }
    let mut cursor = 0;
    tokens
        .iter()
        .map(|(_, lower)| {
            let want = crate::practice::normalize(lower);
            let found = graded[cursor..].iter().position(|w| crate::practice::normalize(&w.e) == want)?;
            let w = &graded[cursor + found];
            cursor += found + 1;
            Some(matches!(w.s, 'r' | 'a'))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_link_to_their_word_and_lookalikes_do_not() {
        assert!(close_enough("mlekiem", "mleko"));
        assert!(close_enough("kawę", "kawa"));
        assert!(close_enough("kobietami", "kobieta"));
        assert!(!close_enough("jest", "jeszcze"));
        assert!(!close_enough("kot", "kotlet"));
    }
}
