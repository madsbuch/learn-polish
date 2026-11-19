//! Conversation: read a short dialogue, say what comes next (one of the
//! lines out loud, where the device can hear: speaking.rs), and write one
//! line down as it is said.

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{self, Built, Drill};

pub const TYPE: LessonType = LessonType {
    id: "dialogue",
    name: "Dialogue",
    icon: "drill",
    description: "A short everyday dialogue: read and hear it, say what comes next (one line out loud, where speaking is on), and write down a line as it is said.",
    tutor_hint: "",
    run: Run::Exercises(build),
    path: PathGroup::Dialogue,
    takes_items: true,
    unaided_to_master: false,
    sections: &["dialogue"],
    start_label: "Start",
    again_label: "Practise again",
};

fn build(conn: &mut Conn, drill: &Drill) -> Result<Built> {
    let rng = &mut rand::rng();
    let id = drill.lesson.id;
    let dialogue = session::lesson_lines(conn, id)?;
    let mut exercises = Vec::new();
    if !dialogue.is_empty() {
        exercises.push(session::dialogue_read(id, drill.course));
    }
    let mut gaps = session::dialogue_gaps(&dialogue, id, rng);
    let asked: Vec<String> = gaps.iter().map(|g| g.answer.clone()).collect();
    // Where this device can hear the learner, one of the lines is said
    // instead of picked (speaking.rs): the one of a few words, if any.
    if crate::speaking::available(conn) {
        let words = |g: &crate::models::Exercise| g.answer.split_whitespace().count();
        if let Some(at) = (0..gaps.len()).max_by_key(|&i| ((2..=10).contains(&words(&gaps[i])), i)) {
            let gap = gaps.remove(at);
            gaps.insert(at, session::spoken_turn(gap, &dialogue));
        }
    }
    exercises.extend(gaps);
    exercises.extend(session::dialogue_dictation(&dialogue, id, &asked, rng));
    Ok(Built { exercises, dialogue })
}
