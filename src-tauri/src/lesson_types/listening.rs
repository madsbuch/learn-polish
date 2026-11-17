//! Listening: play a song or podcast, then the examiner asks what was heard.

use super::{Conversation, LessonType, PathGroup, Run};

pub const TYPE: LessonType = LessonType {
    id: "listening",
    name: "Listening",
    icon: "song",
    description: "A song or podcast to listen to, then the examiner asks about what was heard: meaning, missing words, translation, grammar.",
    tutor_hint: "",
    run: Run::Conversation(Conversation { partner: false, goals_pay: false, after_verdict: None }),
    path: PathGroup::Listening,
    takes_items: false,
    unaided_to_master: false,
    sections: &["song", "objectives", "note", "sources"],
    start_label: "I've listened — check me",
    again_label: "Check in",
};
