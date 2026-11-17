//! Memory cards: find the pairs, word and meaning.

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{self, Built, Drill, WordRow, MEMORY_PAIRS};

pub const TYPE: LessonType = LessonType {
    id: "memory",
    name: "Memory cards",
    icon: "drill",
    description: "A memory game: turn cards to match each word with its meaning.",
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
    let id = drill.lesson.id;
    let all: Vec<&WordRow> = drill.words.iter().map(|(w, _)| w).collect();
    let exercises = all
        .chunks(MEMORY_PAIRS)
        .enumerate()
        .filter_map(|(n, chunk)| session::memory_game(chunk, format!("b{id}-memory-{n}"), id, rng))
        .collect();
    Ok(Built { exercises, ..Default::default() })
}
