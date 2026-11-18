//! How advanced a course is, on the CEFR scale learners know: A1 (a first
//! course) to C2. The tutor gives each course a level when it makes it; a
//! course without one gets it when the learner asks — the Exercises tab
//! offers "Add levels" then, which asks the tutor. Nothing levels courses
//! behind the learner's back (PRINCIPLES.md). A lesson can carry its own
//! level — a challenge above its course — and otherwise has its course's. A
//! level is shown as a badge, never written into a title (see
//! [`split_title`]).
//!
//! Levels describe material; they never limit what can be made or done.

use diesel::prelude::*;

use crate::db::Conn;
use crate::error::{Error, Result};
use crate::schema::{courses, lessons};

/// The CEFR scale, lowest first.
pub const LEVELS: &[&str] = &["A1", "A2", "B1", "B2", "C1", "C2"];

/// A level as written ("a2", "A2+", "b1 ") to one of [`LEVELS`].
pub fn normalize(level: &str) -> Option<&'static str> {
    let upper = level.trim().to_uppercase();
    LEVELS.iter().copied().find(|l| upper.starts_with(l))
}

/// One word that is exactly a level ("A2", "b1", "B2+"), with what may wrap
/// it in a title ("(A2)", "[B1]", "Level A2" is two words — see below).
fn level_word(word: &str) -> Option<&'static str> {
    let word = word.trim_matches(|c: char| "()[]{}:,;|·-–—".contains(c)).trim_end_matches('+');
    LEVELS.iter().copied().find(|l| word.eq_ignore_ascii_case(l))
}

/// A title with a level written into it — "Mad og drikke (A2)", "Food – B1",
/// "[A1] Greetings", "B2: News", "Food, level A2" — as the title without it
/// and the level. None when the title has no level in it.
pub fn split_title(title: &str) -> Option<(String, &'static str)> {
    let words: Vec<&str> = title.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let tidy = |rest: &[&str]| {
        let text = rest.join(" ");
        let text = text.trim_matches(|c: char| c.is_whitespace() || "-–—:|·,;".contains(c)).to_string();
        // A dangling "level"/"niveau"/"CEFR" goes too.
        let cut = ["level", "niveau", "poziom", "cefr"].iter().find_map(|w| {
            let at = text.len().checked_sub(w.len())?;
            (text.is_char_boundary(at) && text[at..].eq_ignore_ascii_case(w)).then_some(at)
        });
        let text = cut.map_or(text.clone(), |at| text[..at].to_string());
        text.trim_matches(|c: char| c.is_whitespace() || "-–—:|·,;(".contains(c)).to_string()
    };
    if let Some(level) = level_word(words[words.len() - 1]) {
        let rest = tidy(&words[..words.len() - 1]);
        return (!rest.is_empty()).then_some((rest, level));
    }
    if let Some(level) = level_word(words[0]) {
        let rest = tidy(&words[1..]);
        return (!rest.is_empty()).then_some((rest, level));
    }
    None
}

/// Levels the tutor once wrote into course titles move to where they belong:
/// the level (shown as a badge), the title trimmed. A course that already
/// has a level keeps it. Runs at every start, and is cheap when there is
/// nothing to move.
pub fn move_levels_out_of_titles(conn: &mut Conn) -> Result<usize> {
    let all: Vec<(i64, String, String, Option<String>)> = courses::table
        .select((courses::id, courses::title_en, courses::title_pl, courses::level))
        .load(conn)?;
    let mut moved = 0;
    for (id, title, title_native, level) in all {
        let english = split_title(&title);
        let native = split_title(&title_native);
        let Some(found) = english.as_ref().or(native.as_ref()).map(|(_, l)| *l) else { continue };
        let title = english.map_or(title, |(t, _)| t);
        let title_native = native.map_or(title_native, |(t, _)| t);
        diesel::update(courses::table.find(id))
            .set((
                courses::title_en.eq(&title),
                courses::title_pl.eq(&title_native),
                courses::level.eq(level.as_deref().and_then(normalize).unwrap_or(found)),
            ))
            .execute(conn)?;
        moved += 1;
    }
    if moved > 0 {
        crate::diagnostics::note("levels", &format!("Moved the level out of {moved} course title(s)."));
    }
    Ok(moved)
}

/// Sets a course's level, as the tutor (or the learner) says it is.
pub fn set_course_level(conn: &mut Conn, course_id: i64, level: &str) -> Result<&'static str> {
    crate::courses::require_open_course(conn, course_id)?;
    let level = normalize(level).ok_or_else(|| Error::msg(format!("'{level}' is not a level: use A1–C2.")))?;
    diesel::update(courses::table.find(course_id)).set(courses::level.eq(level)).execute(conn)?;
    Ok(level)
}

/// Sets (or, with None, clears) a lesson's own level.
pub fn set_lesson_level(conn: &mut Conn, lesson_id: i64, level: Option<&str>) -> Result<Option<&'static str>> {
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


    #[test]
    fn levels_come_out_of_titles() {
        let split = |t: &str| split_title(t).map(|(t, l)| (t, l.to_string()));
        assert_eq!(split("Mad og drikke (A2)"), Some(("Mad og drikke".into(), "A2".into())));
        assert_eq!(split("Food – B1"), Some(("Food".into(), "B1".into())));
        assert_eq!(split("Food - b2+"), Some(("Food".into(), "B2".into())));
        assert_eq!(split("[A1] Greetings"), Some(("Greetings".into(), "A1".into())));
        assert_eq!(split("B2: The news"), Some(("The news".into(), "B2".into())));
        assert_eq!(split("Food, level A2"), Some(("Food".into(), "A2".into())));
        assert_eq!(split("Rejse (niveau B1)"), Some(("Rejse".into(), "B1".into())));
        assert_eq!(split("Ordering food"), None);
        assert_eq!(split("A2"), None, "a title that is only a level stays");
        assert_eq!(split("Vitamin B12"), None);
    }
}
