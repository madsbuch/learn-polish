//! Words: new-word cards, then cards that get harder as each word is known
//! (up to what the lesson's level allows: difficulty.rs), a
//! short round of sound pairs when the words have sounds an English ear
//! mixes up, and a memory round to end.

use rand::seq::SliceRandom;

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{self, Built, Drill, WordRow};

pub const TYPE: LessonType = LessonType {
    id: "words",
    name: "Words",
    icon: "drill",
    description: "Learn a handful of words: cards, then multiple choice, then — as far as the lesson's level allows — typing them from memory, writing them from hearing them and saying one aloud, a couple of sound pairs to tell apart by ear when the words have tricky sounds, and a memory round.",
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
    // Where the device can hear the learner, a known word whose familiarity
    // and level reach it is said aloud (speaking.rs), a few a lesson at
    // most; the rest step down to the card below.
    let mut say = if crate::speaking::available(conn) { crate::speaking::WORD_CARDS } else { 0 };
    // New words are introduced first, in order; known ones are practised in
    // a shuffled order.
    let mut known = Vec::new();
    for (w, is_new) in &drill.words {
        if *is_new {
            // Spec: DRILL-1, WRD-1
            exercises.push(session::word_intro(w));
            exercises.push(session::recognise(w, &drill.pool, rng));
            continue;
        }
        // Spec: DRILL-15, DRILL-18, WRD-2
        let card = crate::difficulty::pick(crate::difficulty::WORD_CARDS, drill.level, w.familiarity, false, |k| k != "say-word" || say > 0);
        if card == "say-word" {
            say -= 1;
        }
        known.push(session::word_card(w, card, &drill.pool, rng));
    }
    known.shuffle(rng);
    exercises.extend(known);
    let all: Vec<&WordRow> = drill.words.iter().map(|(w, _)| w).collect();
    let id = drill.lesson.id;
    exercises.extend(crate::sounds::round(&all, id, rng));
    exercises.extend(session::memory_game(&all, format!("b{id}-memory"), id, rng));
    Ok(Built { exercises, ..Default::default() })
}
