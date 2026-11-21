//! Study: a short text — a grammar point, a bit of culture — checked by the
//! examiner in a short interview.

use super::{Conversation, LessonType, PathGroup, Run};

pub const TYPE: LessonType = LessonType {
    id: "study",
    name: "Study",
    icon: "study",
    description: "A short text — a grammar point, a conjugation, a bit of culture — then a few questions from the examiner.",
    tutor_hint: "Give it objectives; culture and facts can come from Wikipedia (attach_source).",
    run: Run::Conversation(Conversation { partner: false, goals_pay: false, after_verdict: None }),
    path: PathGroup::Study,
    takes_items: false,
    unaided_to_master: false,
    sections: &["objectives", "text", "sources"],
    start_label: "I've studied it — check me",
    again_label: "Talk it through again",
};
