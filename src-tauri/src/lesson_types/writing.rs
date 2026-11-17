//! Writing: a situation played out in writing with a partner who understands
//! only the language learnt. Help (a word bank, hints) steps down as the
//! learner passes; mastering it takes doing it once without help.

use diesel::prelude::*;

use super::{Conversation, LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::lessons::Lesson;

pub const TYPE: LessonType = LessonType {
    id: "writing",
    name: "Writing",
    icon: "write",
    description: "A situation written out with a partner — tell a neighbour where you live, order dinner — who writes and understands only the language learnt.",
    run: Run::Conversation(Conversation { partner: true, goals_pay: true, after_verdict: Some(step_help) }),
    path: PathGroup::Writing,
    takes_items: false,
    unaided_to_master: true,
    sections: &["writing", "goals", "situation"],
    start_label: "Start writing",
    again_label: "Write it again",
};

/// Help follows how it went, even on a replay the same day.
fn step_help(conn: &mut Conn, lesson: &Lesson, result: &str) -> Result<()> {
    use crate::schema::lessons;
    diesel::update(lessons::table.find(lesson.id))
        .set(lessons::support_level.eq(crate::examiner::next_support_level(lesson.support_level, result)))
        .execute(conn)?;
    Ok(())
}
