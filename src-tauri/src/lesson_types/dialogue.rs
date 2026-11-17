//! Conversation: read a short dialogue, then fill its lines.

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{self, Built, Drill};

pub const TYPE: LessonType = LessonType {
    id: "dialogue",
    name: "Conversation",
    icon: "drill",
    description: "A short everyday conversation: read and hear it, then say what comes next, line by line.",
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
    exercises.extend(session::dialogue_gaps(&dialogue, id, rng));
    Ok(Built { exercises, dialogue })
}
