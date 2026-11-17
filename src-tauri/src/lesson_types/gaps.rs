//! Fill the gaps: sentences with a word missing.

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::session::{Built, Drill};

pub const TYPE: LessonType = LessonType {
    id: "gaps",
    name: "Fill the gaps",
    icon: "drill",
    description: "Sentences with a word missing, chosen from a word bank.",
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
    Ok(Built { exercises: super::sentences::gaps(conn, drill, rng)?, ..Default::default() })
}
