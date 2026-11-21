//! Typing: each word written from memory.

use rand::seq::SliceRandom;

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{self, Built, Drill};

pub const TYPE: LessonType = LessonType {
    id: "typing",
    name: "Typing",
    icon: "drill",
    description: "Type each word from its meaning.",
    tutor_hint: "For words that are known but need recalling.",
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
    let mut known = Vec::new();
    for (w, is_new) in &drill.words {
        if *is_new {
            exercises.push(session::word_intro(w));
            exercises.push(session::typing(w));
        } else {
            known.push(session::typing(w));
        }
    }
    known.shuffle(rng);
    exercises.extend(known);
    Ok(Built { exercises, ..Default::default() })
}
