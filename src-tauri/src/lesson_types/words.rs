//! Words: new-word cards, multiple choice, typing once a word is known, and
//! a memory round to end.

use rand::seq::SliceRandom;

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{self, Built, Drill, WordRow};

pub const TYPE: LessonType = LessonType {
    id: "words",
    name: "Words",
    icon: "drill",
    description: "Learn a handful of words: cards, then multiple choice, then typing them from memory, and a memory round.",
    tutor_hint: "",
    run: Run::Exercises(build),
    path: PathGroup::Words,
    takes_items: true,
    unaided_to_master: false,
    sections: &["words"],
    start_label: "Start",
    again_label: "Practise again",
};

fn build(_conn: &mut Conn, drill: &Drill) -> Result<Built> {
    let rng = &mut rand::rng();
    let mut exercises = Vec::new();
    // New words are introduced first, in order; known ones are practised in
    // a shuffled order.
    let mut known = Vec::new();
    for (w, is_new) in &drill.words {
        if *is_new {
            exercises.push(session::word_intro(w));
            exercises.push(session::recognise(w, &drill.pool, rng));
        } else {
            known.push(session::practise_word(w, &drill.pool, rng));
        }
    }
    known.shuffle(rng);
    exercises.extend(known);
    let all: Vec<&WordRow> = drill.words.iter().map(|(w, _)| w).collect();
    let id = drill.lesson.id;
    exercises.extend(session::memory_game(&all, format!("b{id}-memory"), id, rng));
    Ok(Built { exercises, ..Default::default() })
}
