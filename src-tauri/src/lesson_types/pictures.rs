//! Pictures: everyday objects, each drawn once, learnt by linking the word
//! and its picture — see the picture and pick the word, see the word and
//! pick the picture — and a memory round of pictures and words. The words
//! are the lesson's own; their pictures are drawn in the background
//! (crate::pictures).

use rand::seq::SliceRandom;

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{self, Built, Drill, WordRow};

pub const TYPE: LessonType = LessonType {
    id: "pictures",
    name: "Pictures",
    icon: "picture",
    description: "Everyday objects, each drawn as a picture: see the picture and pick the word, see the word and pick the picture. Only concrete things you can point at, each unmistakable on its own.",
    run: Run::Exercises(build),
    path: PathGroup::Words,
    // Made with its own tutor tool (create_picture_lesson), objects only.
    takes_items: false,
    unaided_to_master: false,
    sections: &["pictures"],
    start_label: "Start",
    again_label: "Practise again",
};

fn build(conn: &mut Conn, drill: &Drill) -> Result<Built> {
    let rng = &mut rand::rng();
    let images = session::picture_pool(conn, drill.lesson.id)?;
    let mut exercises = Vec::new();
    let mut known = Vec::new();
    for (w, is_new) in &drill.words {
        // A word whose picture is not drawn yet waits: it has no picture to
        // be linked with.
        if w.image.is_none() {
            continue;
        }
        if *is_new {
            exercises.push(session::picture_intro(w));
            exercises.push(session::practise_picture(w, &drill.pool, &images, rng));
        } else {
            known.push(session::practise_picture(w, &drill.pool, &images, rng));
        }
    }
    known.shuffle(rng);
    exercises.extend(known);
    let pictured: Vec<&WordRow> = drill.words.iter().map(|(w, _)| w).filter(|w| w.image.is_some()).collect();
    let id = drill.lesson.id;
    exercises.extend(session::picture_memory(&pictured, format!("b{id}-pictures"), id, rng));
    Ok(Built { exercises, ..Default::default() })
}
