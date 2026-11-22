//! Sentences: build each from its words, write it down as it is said, or
//! say it aloud — each by the card its familiarity and the lesson's level
//! give (difficulty.rs); then a short round of sound pairs with words of the
//! sentences (sounds.rs), and fill gaps in them.

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{self, Built, Drill, SentenceRow, GAP_SENTENCES};

pub const TYPE: LessonType = LessonType {
    id: "sentences",
    name: "Sentences",
    icon: "drill",
    description: "Build sentences from their words as tiles (picking the ending where a word changes form, from two or from every form as the level allows), write them down as they are said (from a word bank first), type a missing word, or say them aloud, tell a sound pair or two with their words apart by ear, then fill the gaps in them.",
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
    // Where the device can hear the learner, a known sentence whose
    // familiarity and level reach it is said aloud (speaking.rs), up to a
    // couple a lesson; the rest step down to the card below.
    let mut say = if crate::speaking::available(conn) { crate::speaking::SENTENCE_CARDS } else { 0 };
    for (s, is_new) in &drill.sentences {
        if *is_new {
            // A new sentence is read, then built from the easiest tiles.
            // Spec: DRILL-1, SNT-1
            exercises.push(session::sentence_intro(s));
        }
        // Spec: SNT-2, SNT-3, DRILL-18
        let card = session::practise_sentence(s, *is_new, drill, |k| k != "say-sentence" || say > 0, rng);
        if card.kind == "sayIt" {
            say -= 1;
        }
        exercises.push(card);
    }
    // Spec: SPK-11
    let rows: Vec<&SentenceRow> = drill.sentences.iter().map(|(s, _)| s).collect();
    let near = session::course_words_with_near(conn, drill.lesson.id)?;
    exercises.extend(crate::sounds::sentence_round(&rows, &near, drill.lesson.id, rng));
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
