//! Sentences: build each from its words, then fill gaps in them.

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{self, Built, Drill, SentenceRow, GAP_SENTENCES};

pub const TYPE: LessonType = LessonType {
    id: "sentences",
    name: "Sentences",
    icon: "drill",
    description: "Build sentences from their words, then fill the gaps in them.",
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
    for (s, is_new) in &drill.sentences {
        if *is_new {
            exercises.push(session::sentence_intro(s));
        }
        exercises.push(session::word_order(s));
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
