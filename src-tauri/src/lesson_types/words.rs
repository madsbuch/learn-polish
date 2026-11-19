//! Words: new-word cards, multiple choice, typing once a word is known, a
//! short round of sound pairs when the words have sounds an English ear
//! mixes up, and a memory round to end.

use rand::seq::{IndexedRandom, SliceRandom};

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{self, Built, Drill, WordRow};

pub const TYPE: LessonType = LessonType {
    id: "words",
    name: "Words",
    icon: "drill",
    description: "Learn a handful of words: cards, then multiple choice, then typing them from memory (now and then saying one aloud), a couple of sound pairs to tell apart by ear when the words have tricky sounds, and a memory round.",
    tutor_hint: "",
    run: Run::Exercises(build),
    path: PathGroup::Words,
    takes_items: true,
    unaided_to_master: false,
    sections: &["words"],
    start_label: "Start",
    again_label: "Practise again",
};

fn build(conn: &mut Conn, drill: &Drill) -> Result<Built> {
    let rng = &mut rand::rng();
    let mut exercises = Vec::new();
    // Where the device can hear the learner, a word met before (one with a
    // few known around it) is said aloud instead of practised (speaking.rs).
    let known_words: Vec<&WordRow> = drill.words.iter().filter(|(w, n)| !n && w.reps >= 1).map(|(w, _)| w).collect();
    let mut said: Vec<crate::ids::WordId> = Vec::new();
    if known_words.len() >= 3 && crate::speaking::available(conn) {
        said = known_words.choose_multiple(rng, crate::speaking::WORD_CARDS).map(|w| w.id).collect();
    }
    // New words are introduced first, in order; known ones are practised in
    // a shuffled order.
    let mut known = Vec::new();
    for (w, is_new) in &drill.words {
        if *is_new {
            exercises.push(session::word_intro(w));
            exercises.push(session::recognise(w, &drill.pool, rng));
        } else if said.contains(&w.id) {
            known.push(session::say_word(w));
        } else {
            known.push(session::practise_word(w, &drill.pool, rng));
        }
    }
    known.shuffle(rng);
    exercises.extend(known);
    let all: Vec<&WordRow> = drill.words.iter().map(|(w, _)| w).collect();
    let id = drill.lesson.id;
    exercises.extend(crate::sounds::round(&all, id, rng));
    exercises.extend(session::memory_game(&all, format!("b{id}-memory"), id, rng));
    Ok(Built { exercises, ..Default::default() })
}
