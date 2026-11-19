//! Sentences: build each from its words, write it down as it is said, or
//! (now and then, where the device can hear) say it aloud; then fill gaps
//! in them.

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{self, Built, Drill, SentenceRow, GAP_SENTENCES};

pub const TYPE: LessonType = LessonType {
    id: "sentences",
    name: "Sentences",
    icon: "drill",
    description: "Build sentences from their words as tiles (picking the ending where a word changes form), write them down as they are said, or say them aloud, then fill the gaps in them.",
    tutor_hint: "",
    run: Run::Exercises(build),
    path: PathGroup::Sentences,
    takes_items: true,
    unaided_to_master: false,
    sections: &["sentences"],
    start_label: "Start",
    again_label: "Practise again",
};

fn build(conn: &mut Conn, drill: &Drill) -> Result<Built> {
    let rng = &mut rand::rng();
    let mut exercises = Vec::new();
    // Where the device can hear the learner, a few known sentences are said
    // aloud instead (speaking.rs): one in three, by the same turn-taking,
    // up to a couple a lesson — the lesson stays as long as it was.
    let mut say = if crate::speaking::available(conn) { crate::speaking::SENTENCE_CARDS } else { 0 };
    for (n, (s, is_new)) in drill.sentences.iter().enumerate() {
        if *is_new {
            // A new sentence is read, then built: its words are given.
            exercises.push(session::sentence_intro(s));
            exercises.push(session::word_order(s));
        } else {
            // A known one is built or written from hearing it, in turn — by
            // the time it comes back and by its place, so a lesson has some
            // of each and a sentence alternates between them.
            let turn = s.reps + crate::convert::count(n);
            if say > 0 && turn % 3 == 2 {
                say -= 1;
                exercises.push(session::say_sentence(s));
                continue;
            }
            exercises.push(if turn % 2 == 1 {
                session::dictation(s)
            } else {
                session::word_order(s)
            });
        }
    }
    exercises.extend(gaps(conn, drill, rng)?);
    Ok(Built { exercises, ..Default::default() })
}

/// The fill-the-gaps exercises over a drill's sentences, a few at a time.
pub(super) fn gaps(conn: &mut Conn, drill: &Drill, rng: &mut impl rand::Rng) -> Result<Vec<crate::models::Exercise>> {
    let rows: Vec<&SentenceRow> = drill.sentences.iter().map(|(s, _)| s).collect();
    let mut exercises = Vec::new();
    for chunk in rows.chunks(GAP_SENTENCES) {
        exercises.extend(session::gap_fill(conn, drill.lesson.id, chunk, true, rng)?);
    }
    Ok(exercises)
}
