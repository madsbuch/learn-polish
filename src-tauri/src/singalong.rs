//! What the words of a song mean, for the music player's transcript: tap a
//! word to see its meaning, and Sing along, where the words to fill in are
//! the lesson's own first.
//!
//! The meanings come from what the app already knows — the words of the
//! course the song is filed in (its listening lesson's), the words the song
//! was written from (its origin: "Polish = English"), and every word the
//! learner has — matched to the sung forms. Polish words change their
//! endings ("kawę" is "kawa"), so a sung form also matches a word that
//! shares all but its last letters (see [`same_word`]); the screen then
//! shows the dictionary form with the meaning. Nothing is asked of a model
//! and nothing is written: a word with no meaning offers the tutor instead.

use std::collections::HashMap;

use diesel::prelude::*;
use serde::Serialize;
use tauri::State;

use crate::db::{Conn, Db};
use crate::error::Result;
use crate::schema::{course_words, words};
use crate::session::split_punctuation;
#[allow(unused_imports)]
use crate::ids::*;

/// A sung word's meaning.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GlossWord {
    /// The word as sung, lowercased, without punctuation: what the screen
    /// looks a word up by.
    pub form: String,
    /// Its dictionary form, as the learner's words have it.
    pub pl: String,
    pub en: String,
    /// The learner's word, when it is one (to talk about it with the tutor).
    pub word_id: Option<WordId>,
    /// One of the words of the course the song is filed in: Sing along
    /// blanks these first.
    pub lesson: bool,
}

/// Lowercased, without the punctuation around it.
pub fn form_of(token: &str) -> String {
    split_punctuation(token).1.to_lowercase()
}

/// Whether a sung form is a form of a dictionary word: the same, or sharing
/// all but the ending — at least three letters, and no more than the last
/// letter of the shorter left over ("kawę" / "kawa", "mlekiem" / "mleko").
pub fn same_word(form: &str, base: &str) -> bool {
    let (f, b): (Vec<char>, Vec<char>) = (form.chars().collect(), base.to_lowercase().chars().collect());
    if f == b {
        return true;
    }
    let shared = f.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let shorter = f.len().min(b.len());
    shared >= 3 && shared + 1 >= shorter && f.len().abs_diff(b.len()) <= 3
}

/// One candidate meaning: (Polish, English, word, from the song's course).
type Known = (String, String, Option<WordId>, bool);

/// Finds a form's meaning: an exact match first, then a form of a word;
/// within each, in the order given (the course's words first).
fn look_up(form: &str, known: &[Known]) -> Option<GlossWord> {
    let single = known.iter().filter(|(pl, ..)| !pl.contains(' '));
    let exact = single.clone().find(|(pl, ..)| pl.to_lowercase() == form);
    let found = exact.or_else(|| single.clone().find(|(pl, ..)| form.chars().count() >= 3 && same_word(form, pl)))?;
    Some(GlossWord { form: form.into(), pl: found.0.clone(), en: found.1.clone(), word_id: found.2, lesson: found.3 })
}

/// The meanings of a song's words, for every sung form that has one.
pub fn glossary(conn: &mut Conn, song_id: SongId) -> Result<Vec<GlossWord>> {
    let song = crate::songs::load_song(conn, song_id)?;
    let mut forms: Vec<String> = Vec::new();
    let sung = song.sections.iter().flat_map(|s| s.lines.iter().map(|l| l.pl.clone()));
    let timed = song.tracks.iter().flat_map(|t| t.lines.iter().flatten().filter(|l| l.lang == "pl").map(|l| l.text.clone()));
    for line in sung.chain(timed) {
        for token in line.split_whitespace() {
            let form = form_of(token);
            if !form.is_empty() && !forms.contains(&form) {
                forms.push(form);
            }
        }
    }

    let mut known: Vec<Known> = Vec::new();
    if let Some(course) = song.course_id {
        let rows: Vec<(WordId, String, String)> = course_words::table
            .filter(course_words::course_id.eq(course))
            .order(course_words::ord)
            .select((course_words::id, course_words::pl, course_words::en))
            .load(conn)?;
        known.extend(rows.into_iter().map(|(id, pl, en)| (pl, en, Some(id), true)));
    }
    for given in &song.origin.given.words {
        if let Some((pl, en)) = given.split_once('=') {
            known.push((pl.trim().into(), en.trim().into(), None, false));
        }
    }
    let rows: Vec<(WordId, String, String)> = words::table.select((words::id, words::pl, words::en)).load(conn)?;
    known.extend(rows.into_iter().map(|(id, pl, en)| (pl, en, Some(id), false)));

    // A word the song was written from that the learner has: its id, so the
    // tutor can be asked about it.
    let ids: HashMap<String, WordId> =
        known.iter().filter_map(|(pl, _, id, _)| Some((pl.to_lowercase(), (*id)?))).collect();
    Ok(forms
        .iter()
        .filter_map(|form| look_up(form, &known))
        .map(|mut g| {
            g.word_id = g.word_id.or_else(|| ids.get(&g.pl.to_lowercase()).copied());
            g
        })
        .collect())
}

#[tauri::command]
pub async fn song_glossary(db: State<'_, Db>, song_id: SongId) -> Result<Vec<GlossWord>> {
    glossary(&mut db.lock(), song_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sung_form_finds_its_dictionary_word() {
        assert!(same_word("kawę", "kawa"));
        assert!(same_word("mlekiem", "mleko"));
        assert!(same_word("domu", "dom"));
        assert!(same_word("kraków", "Kraków"));
        assert!(!same_word("kot", "kto"));
        assert!(!same_word("piję", "pić"), "too little in common to be sure");
        assert!(!same_word("lato", "las"));
    }

    #[test]
    fn exact_words_come_before_forms_and_the_course_first() {
        let known: Vec<Known> = vec![
            ("kawa".into(), "coffee".into(), None, true),
            ("kawał".into(), "joke".into(), None, false),
            ("kawę".into(), "coffee (as an object)".into(), None, false),
        ];
        assert_eq!(look_up("kawę", &known).unwrap().en, "coffee (as an object)", "an exact match wins");
        let g = look_up("kawy", &known).unwrap();
        assert_eq!((g.pl.as_str(), g.lesson), ("kawa", true));
        assert_eq!(look_up("herbata", &known), None);
        assert_eq!(form_of("„Kawę,"), "kawę");
    }
}
