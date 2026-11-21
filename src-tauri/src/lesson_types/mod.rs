//! Lesson types: one flat list, one module each.
//!
//! A lesson's type says everything about it: its name and icon, what it is
//! for (the tutor reads this), how it is practised — a drill of exercises,
//! or a conversation with the examiner — where it goes in a course's path,
//! what its page shows, and what mastering it takes. Nothing else in the app
//! branches on a lesson's type: it asks the type, through [`get`].
//!
//! To add a type: write a module here with a `pub const TYPE: LessonType`,
//! list it in [`REGISTRY`], and give the screens anything new it needs (an
//! exercise it introduces: src/exercises/; an icon: src/lib/lessonTypes.ts).
//! The contract test at the bottom runs every registered type through
//! creating and practising it. See ADDING_A_LESSON_TYPE.md.

use serde::Serialize;

use crate::db::Conn;
use crate::error::{Error, Result};
use crate::lessons::Lesson;
use crate::session::{Built, Drill};

mod dialogue;
mod gaps;
mod listening;
mod memory;
mod pictures;
pub(crate) mod scene;
mod sentences;
mod story;
mod study;
mod typing;
mod words;
mod writing;

/// Every lesson type, in the order they are offered.
// Spec: LSN-1, LSN-2
pub const REGISTRY: &[&LessonType] = &[
    &words::TYPE,
    &pictures::TYPE,
    &scene::TYPE,
    &story::TYPE,
    &typing::TYPE,
    &memory::TYPE,
    &sentences::TYPE,
    &gaps::TYPE,
    &dialogue::TYPE,
    &study::TYPE,
    &listening::TYPE,
    &writing::TYPE,
];

/// How a lesson is practised.
pub enum Run {
    /// A drill: a list of exercises, built fresh each time from the lesson's
    /// items and their history.
    Exercises(fn(&mut Conn, &Drill) -> Result<Built>),
    /// A check-in: a conversation with the examiner (or, for writing, a
    /// partner in a situation), ending in a verdict.
    Conversation(Conversation),
}

/// How a conversation lesson differs from a plain examiner interview.
pub struct Conversation {
    /// The other side is a partner in a situation, not an examiner: its own
    /// prompt (see examiner::writing_prompt) and its own model.
    pub partner: bool,
    /// Its check-in pays a bonus per goal (writing).
    pub goals_pay: bool,
    /// After the verdict, e.g. stepping the writing help down.
    pub after_verdict: Option<fn(&mut Conn, &Lesson, &str) -> Result<()>>,
}

/// Where a type goes when a course's lessons are arranged into a path:
/// the conversation first, then words and sentences in rounds with study
/// lessons between, listening after, writing last (courses::arrange_path).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathGroup {
    Dialogue,
    Words,
    Sentences,
    Study,
    Listening,
    Writing,
}

pub struct LessonType {
    /// Stored in lessons.lesson_type.
    pub id: &'static str,
    /// What the learner sees: "Memory cards".
    pub name: &'static str,
    /// For the screens: drill | study | write | song | picture.
    pub icon: &'static str,
    /// What it is, for the learner (the "How to use" page) and the tutor.
    pub description: &'static str,
    /// For the tutor only: when to use it, what to put in it.
    pub tutor_hint: &'static str,
    pub run: Run,
    pub path: PathGroup,
    /// Takes words, sentences or dialogue lines when it is made (the tutor's
    /// create_drill_lesson offers these types).
    pub takes_items: bool,
    /// Writing: mastering it also takes doing it once without help.
    pub unaided_to_master: bool,
    /// What its page shows, in order: objectives | goals | text | situation |
    /// note | sources | song | writing | dialogue | words | sentences |
    /// pictures | scene | where | story (each drawn by src/screens/LessonPage.tsx).
    pub sections: &'static [&'static str],
    /// Its page's button, the first time and after.
    pub start_label: &'static str,
    pub again_label: &'static str,
}

impl LessonType {
    pub fn is_drill(&self) -> bool {
        matches!(self.run, Run::Exercises(_))
    }

    pub fn engine(&self) -> &'static str {
        if self.is_drill() {
            "exercises"
        } else {
            "conversation"
        }
    }

    /// A sensible check-in period for a new lesson, in days.
    // Spec: SCHED-1
    pub fn default_period(&self, complexity: &str) -> i64 {
        match (self.is_drill(), complexity) {
            (true, "low") => 2,
            (true, "medium") => 4,
            (true, _) => 7,
            (false, "low") => 5,
            (false, "medium") => 7,
            _ => 10,
        }
    }

    /// About how many minutes a lesson takes: what the daily budget is
    /// counted in.
    // Spec: LSN-6
    pub fn minutes(&self, complexity: &str) -> i64 {
        match (self.is_drill(), complexity) {
            (true, "low") => 2,
            (true, "medium") => 4,
            (true, _) => 8,
            (false, "low") => 5,
            (false, "medium") => 10,
            _ => 20,
        }
    }
}

/// A type by its id.
pub fn get(id: &str) -> Result<&'static LessonType> {
    REGISTRY
        .iter()
        .copied()
        .find(|t| t.id == id)
        .ok_or_else(|| Error::msg(format!("Unknown lesson type '{id}'.")))
}

/// Minutes for a lesson of a type; an unknown type counts as a drill.
pub fn minutes(id: &str, complexity: &str) -> i64 {
    get(id).unwrap_or(&words::TYPE).minutes(complexity)
}

/// The types a drill can be made as from words or sentences.
pub fn item_types() -> impl Iterator<Item = &'static LessonType> {
    REGISTRY.iter().copied().filter(|t| t.takes_items)
}

/// A type as the screens see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub description: &'static str,
    /// "exercises" | "conversation"
    pub engine: &'static str,
    pub unaided_to_master: bool,
    pub sections: &'static [&'static str],
    pub start_label: &'static str,
    pub again_label: &'static str,
    /// Minutes it takes, by complexity [low, medium, high] — the one table
    /// the badges, the summaries and the daily budget all use.
    pub minutes: [i64; 3],
}

/// Every lesson type, for the screens.
#[tauri::command]
pub fn lesson_types() -> Vec<TypeInfo> {
    REGISTRY
        .iter()
        .map(|t| TypeInfo {
            id: t.id,
            name: t.name,
            icon: t.icon,
            description: t.description,
            engine: t.engine(),
            unaided_to_master: t.unaided_to_master,
            sections: t.sections,
            start_label: t.start_label,
            again_label: t.again_label,
            minutes: [t.minutes("low"), t.minutes("medium"), t.minutes("high")],
        })
        .collect()
}

/// The lesson types, as the tutor's instructions list them.
pub fn for_tutor() -> String {
    REGISTRY
        .iter()
        .map(|t| match t.tutor_hint {
            "" => format!("  - {} ({}): {}", t.id, t.name, t.description),
            hint => format!("  - {} ({}): {} {hint}", t.id, t.name, t.description),
        })
        .collect::<Vec<_>>()
        .join("\n")
}


