//! How advanced a course is, on the CEFR scale learners know: A1 (a first
//! course) to C2. The tutor gives each course a level when it makes it; a
//! course without one gets it when the learner asks — the Exercises tab
//! offers "Add levels" then, which asks the tutor. Nothing levels courses
//! behind the learner's back (PRINCIPLES.md). A lesson can carry its own
//! level — a challenge above its course — and otherwise has its course's. A
//! level is shown as a badge, never written into a title.
//!
//! Levels describe material; they never limit what can be made or done.

use diesel::prelude::*;

use crate::db::Conn;
use crate::error::{Error, Result};
use crate::schema::{courses, lessons};
#[allow(unused_imports)]
use crate::ids::*;

/// The CEFR scale, lowest first.
pub const LEVELS: &[&str] = &["A1", "A2", "B1", "B2", "C1", "C2"];

/// A level as written ("a2", "A2+", "b1 ") to one of [`LEVELS`].
pub fn normalize(level: &str) -> Option<&'static str> {
    let upper = level.trim().to_uppercase();
    LEVELS.iter().copied().find(|l| upper.starts_with(l))
}

/// Sets a course's level, as the tutor (or the learner) says it is.
pub fn set_course_level(conn: &mut Conn, course_id: CourseId, level: &str) -> Result<&'static str> {
    crate::courses::require_open_course(conn, course_id)?;
    let level = normalize(level).ok_or_else(|| Error::msg(format!("'{level}' is not a level: use A1–C2.")))?;
    diesel::update(courses::table.find(course_id)).set(courses::level.eq(level)).execute(conn)?;
    Ok(level)
}

/// Sets (or, with None, clears) a lesson's own level.
pub fn set_lesson_level(conn: &mut Conn, lesson_id: LessonId, level: Option<&str>) -> Result<Option<&'static str>> {
    let level = match level.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => Some(normalize(l).ok_or_else(|| Error::msg(format!("'{l}' is not a level: use A1–C2.")))?),
        None => None,
    };
    diesel::update(lessons::table.find(lesson_id)).set(lessons::level.eq(level)).execute(conn)?;
    Ok(level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_read_loosely() {
        assert_eq!(normalize(" a2 "), Some("A2"));
        assert_eq!(normalize("B1+"), Some("B1"));
        assert_eq!(normalize("beginner"), None);
    }


}
