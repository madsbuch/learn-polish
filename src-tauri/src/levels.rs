//! How advanced a course is, on the CEFR scale learners know: A1 (a first
//! course) to C2. The tutor gives each course a level when it makes it, and
//! a level given always stands; a course without one gets it from a small
//! model's reading of its core lessons. A lesson can carry its own level — a
//! challenge above its course — and otherwise has its course's. A level is
//! shown as a badge, never written into a title (see [`split_title`]).
//!
//! Levels describe material; they never limit what can be made or done.

use diesel::prelude::*;
use serde::Deserialize;
use tauri::Manager;

use crate::ai;
use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
use crate::schema::{conversation_lines, course_lessons, courses, lessons, sentences, words};

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

/// What the checker reads: the course's own material, briefly.
fn material(conn: &mut Conn, course_id: i64) -> Result<String> {
    let (title, summary): (String, String) =
        courses::table.find(course_id).select((courses::title_en, courses::summary_en)).first(conn)?;
    // Challenges are beyond the course's level by design: they would pull
    // the reading up, so the check reads the core only.
    let ids: Vec<i64> = course_lessons::table
        .inner_join(lessons::table)
        .filter(course_lessons::course_id.eq(course_id))
        .filter(lessons::challenge.eq(0))
        .select(course_lessons::lesson_id)
        .load(conn)?;
    let listed: Vec<(String, String, String)> = lessons::table
        .filter(lessons::id.eq_any(&ids))
        .select((lessons::lesson_type, lessons::title, lessons::objectives))
        .load(conn)?;
    let vocabulary: Vec<String> =
        words::table.filter(words::lesson_id.eq_any(&ids)).select(words::pl).limit(60).load(conn)?;
    let examples: Vec<String> =
        sentences::table.filter(sentences::lesson_id.eq_any(&ids)).select(sentences::pl).limit(25).load(conn)?;
    let lines: Vec<String> = conversation_lines::table
        .filter(conversation_lines::lesson_id.eq_any(&ids))
        .select(conversation_lines::pl)
        .limit(20)
        .load(conn)?;
    let mut text = format!("Course: {title}\n{summary}\n\nLessons:\n");
    for (kind, title, objectives) in listed {
        text.push_str(&format!("- {kind}: {title} {objectives}\n"));
    }
    text.push_str(&format!("\nWords: {}\n", vocabulary.join(", ")));
    text.push_str(&format!("\nSentences:\n{}\n", examples.join("\n")));
    text.push_str(&format!("\nDialogue:\n{}\n", lines.join("\n")));
    Ok(text)
}

const CHECK_RULES: &str = "You judge how advanced language-learning material is, on the CEFR scale \
    (A1, A2, B1, B2, C1, C2), for an adult learning Polish from English. Judge the material \
    itself — its vocabulary, grammar and the tasks its lessons set — not how long it is. Reply \
    with JSON only: {\"level\": \"A2\", \"why\": \"one short sentence\"}.";

#[derive(Deserialize)]
struct Verdict {
    level: String,
    #[serde(default)]
    why: String,
}

/// What a check decided: the level kept, and whether the check changed it.
#[derive(Debug, PartialEq)]
pub struct Checked {
    pub level: &'static str,
    pub changed: bool,
}

/// What a checked level does to a course's own: an unlevelled course takes
/// it; a level that was given — by the learner, or the tutor on their
/// behalf — always stands (PRINCIPLES.md: levels never limit, and the app
/// is not paternal). The check only fills in what is missing.
pub fn reconcile(given: Option<&str>, checked: &'static str) -> Checked {
    match given.and_then(normalize) {
        None => Checked { level: checked, changed: true },
        Some(given) => Checked { level: given, changed: false },
    }
}

/// Checks one course's level with a small model, and records the outcome.
pub async fn check(db: &Db, course_id: i64) -> Result<Checked> {
    let (key, model, text, given, title) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let (given, title): (Option<String>, String) =
            courses::table.find(course_id).select((courses::level, courses::title_en)).first(conn)?;
        (ai::api_key(conn)?, ai::partner_model(conn)?, material(conn, course_id)?, given, title)
    };
    let messages = [ai::Wire::system(CHECK_RULES), ai::Wire::user(text)];
    let reply = ai::complete_json(&key, &model, &messages, 300).await?;
    let verdict = ai::json_value(&reply.text)
        .and_then(|v| serde_json::from_value::<Verdict>(v).ok())
        .and_then(|v| normalize(&v.level).map(|level| (level, v.why)));
    let Some((checked, why)) = verdict else {
        crate::diagnostics::unreadable_reply(&format!("The level check of “{title}”"), &reply.text);
        return Err(Error::msg("The level check did not answer."));
    };
    let outcome = reconcile(given.as_deref(), checked);
    let mut guard = db.lock();
    let conn = &mut *guard;
    let _ = crate::costs::record_tokens(conn, crate::costs::PARTNER, &model, reply.cost, "level check", reply.tokens);
    diesel::update(courses::table.find(course_id))
        .set((courses::level.eq(outcome.level), courses::level_checked_at.eq(db::now_secs())))
        .execute(conn)?;
    if outcome.changed && given.is_some() {
        crate::diagnostics::note(
            "level",
            format!(
                "“{title}” was made as {}, but reads as {checked} ({why}): it is shown as {checked}.",
                given.as_deref().unwrap_or("?")
            ),
        );
    }
    Ok(outcome)
}

/// Checks, in the background, every course not checked yet: new ones, and
/// those from before levels — one at a time, so it costs little.
pub fn check_unchecked_in_background(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let db = app.state::<Db>();
        let ids: Vec<i64> = {
            let mut guard = db.lock();
            if ai::api_key(&mut guard).is_err() {
                return;
            }
            courses::table
                .filter(courses::level_checked_at.is_null())
                // A given level stands: only unlevelled courses are checked.
                .filter(courses::level.is_null())
                .filter(courses::closed_at.is_null())
                // A folder of one-offs has no level of its own.
                .filter(courses::slug.ne_all(crate::courses::OPEN_ENDED))
                .order(courses::id)
                .select(courses::id)
                .load(&mut *guard)
                .unwrap_or_default()
        };
        for id in ids {
            if let Err(error) = check(&db, id).await {
                crate::diagnostics::note("background", format!("Checking course {id}'s level failed: {error}"));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_read_loosely_and_checked_within_a_step() {
        assert_eq!(normalize(" a2 "), Some("A2"));
        assert_eq!(normalize("B1+"), Some("B1"));
        assert_eq!(normalize("beginner"), None);
        // The tutor's level stands when the check is within a step of it.
        assert_eq!(reconcile(Some("A2"), "B1"), Checked { level: "A2", changed: false });
        assert_eq!(reconcile(Some("A2"), "A1"), Checked { level: "A2", changed: false });
        // Further apart, the check wins; an unlevelled course takes it.
        assert_eq!(reconcile(Some("A1"), "B2"), Checked { level: "A1", changed: false }, "a given level stands");
        assert_eq!(reconcile(None, "A2"), Checked { level: "A2", changed: true });
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
