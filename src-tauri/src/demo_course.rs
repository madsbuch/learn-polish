//! One small, optional starting point. It is seeded once, then belongs to the learner.

use diesel::prelude::*;

use crate::courses::{self, NewLine, NewSentence, NewWord};
use crate::db::{self, Conn};
use crate::error::{Error, Result};
use crate::schema::courses as courses_table;

pub(crate) const SLUG: &str = "demo-first-steps";
pub(crate) const SEEDED: &str = "demo_first_steps_seeded_v1";

pub fn seed_once(conn: &mut Conn) -> Result<()> {
    if db::get_setting(conn, SEEDED)?.is_some() {
        return Ok(());
    }
    conn.transaction::<_, Error, _>(|conn| {
        let exists = courses_table::table.filter(courses_table::slug.eq(SLUG)).count().get_result::<i64>(conn)? > 0;
        // An existing course with this identity is user data: never refill it.
        if !exists {
            let course = courses::ensure(
                conn, SLUG, "First steps", "Pierwsze kroki",
                "A tiny introduction: say hello, introduce yourself, and ask about language.",
            )?;
            let words = [
                ("cześć", "hello, hi", "interj"),
                ("do widzenia", "goodbye", "phr"),
                ("dziękuję", "thank you", "v"),
                ("przepraszam", "excuse me, sorry", "v"),
                ("tak", "yes", "part"),
                ("nie", "no", "part"),
                ("ja", "I", "pron"),
                ("ty", "you", "pron"),
                ("po polsku", "in Polish", "phr"),
                ("po angielsku", "in English", "phr"),
                ("nazywać się", "to be called", "v"),
                ("rozumieć", "to understand", "v"),
            ].into_iter().map(|(pl, en, pos)| NewWord { pl: pl.into(), en: en.into(), pos: pos.into() }).collect::<Vec<_>>();
            let sentences = [
                ("Cześć, nazywam się Anna.", "Hello, my name is Anna."),
                ("Jak się nazywasz?", "What is your name?"),
                ("Mówię po angielsku.", "I speak English."),
                ("Nie rozumiem.", "I do not understand."),
            ].into_iter().map(|(pl, en)| NewSentence { pl: pl.into(), en: en.into() }).collect::<Vec<_>>();
            let dialogue = [
                ("Anna", "f", "Cześć! Nazywam się Anna.", "Hello! My name is Anna."),
                ("Adam", "m", "Cześć! Nazywam się Adam.", "Hello! My name is Adam."),
                ("Anna", "f", "Mówisz po polsku?", "Do you speak Polish?"),
                ("Adam", "m", "Nie, mówię po angielsku.", "No, I speak English."),
                ("Anna", "f", "Ja też mówię po angielsku.", "I also speak English."),
                ("Adam", "m", "Dziękuję! Do widzenia.", "Thank you! Goodbye."),
            ].into_iter().map(|(speaker, gender, pl, en)| NewLine {
                speaker: speaker.into(), gender: gender.into(), pl: pl.into(), en: en.into(),
            }).collect::<Vec<_>>();
            courses::cut_into_drills(conn, course, &words, &sentences, &dialogue, "starter")?;
            rekey(conn)?;
        }
        db::set_setting(conn, SEEDED, "1")
    })
}

/// Gives the demo course's lessons, words, sentences and lines ids derived
/// from the course and their places in it, so two devices that install it
/// agree on every row (DATA.md). Run after seeding, and once when a database
/// is converted to tagged ids; a no-op when the ids are derived already.
/// References follow each id; foreign keys are checked when the transaction
/// commits.
pub fn rekey(conn: &mut Conn) -> Result<()> {
    use crate::ids::*;
    use crate::schema::{
        checkins, conversation_lines, course_lessons, lessons, reviews, sentences, settings, wiki_sources, word_examples,
        words, xp_log,
    };
    use diesel::connection::SimpleConnection;
    conn.batch_execute("PRAGMA defer_foreign_keys = ON")?;
    let Some(course) = courses_table::table
        .filter(courses_table::slug.eq(SLUG))
        .select(courses_table::id)
        .first::<CourseId>(conn)
        .optional()?
    else {
        return Ok(());
    };
    let listed: Vec<LessonId> = course_lessons::table
        .filter(course_lessons::course_id.eq(course))
        .order(course_lessons::ord)
        .select(course_lessons::lesson_id)
        .load(conn)?;
    for (place, old) in listed.into_iter().enumerate() {
        let key = format!("{SLUG}/{place}");
        let new = LessonId::derived("lesson", &key);
        if new != old {
            diesel::update(lessons::table.find(old)).set(lessons::id.eq(new)).execute(conn)?;
            diesel::update(course_lessons::table.filter(course_lessons::lesson_id.eq(old))).set(course_lessons::lesson_id.eq(new)).execute(conn)?;
            diesel::update(words::table.filter(words::lesson_id.eq(old))).set(words::lesson_id.eq(new)).execute(conn)?;
            diesel::update(sentences::table.filter(sentences::lesson_id.eq(old))).set(sentences::lesson_id.eq(new)).execute(conn)?;
            diesel::update(conversation_lines::table.filter(conversation_lines::lesson_id.eq(old)))
                .set(conversation_lines::lesson_id.eq(new))
                .execute(conn)?;
            diesel::update(checkins::table.filter(checkins::lesson_id.eq(old))).set(checkins::lesson_id.eq(new)).execute(conn)?;
            diesel::update(wiki_sources::table.filter(wiki_sources::lesson_id.eq(old))).set(wiki_sources::lesson_id.eq(new)).execute(conn)?;
            diesel::update(xp_log::table.filter(xp_log::lesson_id.eq(old))).set(xp_log::lesson_id.eq(new)).execute(conn)?;
            diesel::update(settings::table.filter(settings::key.eq("daily_ids")))
                .set(settings::value.eq(diesel::dsl::sql::<diesel::sql_types::Text>(&format!("replace(value, '{old}', '{new}')"))))
                .execute(conn)?;
        }
        let owned: Vec<WordId> = words::table.filter(words::lesson_id.eq(new)).order((words::ord, words::id)).select(words::id).load(conn)?;
        for (n, old) in owned.into_iter().enumerate() {
            let fresh = WordId::derived("word", &format!("{key}/{n}"));
            if fresh != old {
                diesel::update(words::table.find(old)).set(words::id.eq(fresh)).execute(conn)?;
                diesel::update(reviews::table.filter(reviews::kind.eq("word")).filter(reviews::item_id.eq(old.to_string())))
                    .set(reviews::item_id.eq(fresh.to_string()))
                    .execute(conn)?;
                diesel::update(word_examples::table.filter(word_examples::word_id.eq(old))).set(word_examples::word_id.eq(fresh)).execute(conn)?;
            }
        }
        let owned: Vec<SentenceId> =
            sentences::table.filter(sentences::lesson_id.eq(new)).order((sentences::ord, sentences::id)).select(sentences::id).load(conn)?;
        for (n, old) in owned.into_iter().enumerate() {
            let fresh = SentenceId::derived("sentence", &format!("{key}/{n}"));
            if fresh != old {
                diesel::update(sentences::table.find(old)).set(sentences::id.eq(fresh)).execute(conn)?;
                diesel::update(reviews::table.filter(reviews::kind.eq("sentence")).filter(reviews::item_id.eq(old.to_string())))
                    .set(reviews::item_id.eq(fresh.to_string()))
                    .execute(conn)?;
            }
        }
        let owned: Vec<LineId> = conversation_lines::table
            .filter(conversation_lines::lesson_id.eq(new))
            .order((conversation_lines::ord, conversation_lines::id))
            .select(conversation_lines::id)
            .load(conn)?;
        for (n, old) in owned.into_iter().enumerate() {
            let fresh = LineId::derived("line", &format!("{key}/{n}"));
            if fresh != old {
                diesel::update(conversation_lines::table.find(old)).set(conversation_lines::id.eq(fresh)).execute(conn)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{lessons, conversation_lines, sentences, words};

    fn path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("polish-demo-{name}-{}.db", std::process::id()))
    }

    fn fresh(name: &str) -> Conn {
        let file = path(name);
        let _ = std::fs::remove_file(&file);
        db::open(&file).unwrap()
    }

    #[test]
    fn a_fresh_install_has_one_small_demo_without_grammar_courses() {
        let mut conn = fresh("fresh");
        assert_eq!(courses_table::table.count().get_result::<i64>(&mut conn).unwrap(), 1);
        let (id, slug): (crate::ids::CourseId, String) = courses_table::table.select((courses_table::id, courses_table::slug)).first(&mut conn).unwrap();
        assert_eq!(slug, SLUG);
        assert_eq!(words::table.count().get_result::<i64>(&mut conn).unwrap(), 12);
        assert_eq!(sentences::table.count().get_result::<i64>(&mut conn).unwrap(), 4);
        assert_eq!(conversation_lines::table.count().get_result::<i64>(&mut conn).unwrap(), 6);
        assert_eq!(lessons::table.count().get_result::<i64>(&mut conn).unwrap(), 4);
        assert_eq!(lessons::table.filter(lessons::lesson_type.eq_any(["study", "writing", "listening"])).count().get_result::<i64>(&mut conn).unwrap(), 0);
        assert!(!crate::lessons::overview(&mut conn).unwrap().due.is_empty(), "today offers the demo's first lessons");
        crate::stats::collect(&mut conn).unwrap();
        // Reopening neither duplicates the demo nor overwrites a learner's rename.
        diesel::update(courses_table::table.find(id)).set(courses_table::title_en.eq("My first Polish")).execute(&mut conn).unwrap();
        drop(conn);
        let mut conn = db::open(&path("fresh")).unwrap();
        assert_eq!(courses_table::table.count().get_result::<i64>(&mut conn).unwrap(), 1);
        assert_eq!(words::table.count().get_result::<i64>(&mut conn).unwrap(), 12);
        assert_eq!(courses_table::table.find(id).select(courses_table::title_en).first::<String>(&mut conn).unwrap(), "My first Polish");
        assert_eq!(db::foreign_key_violations(&mut conn).unwrap(), 0);
    }

    #[test]
    fn deleting_the_demo_is_respected_after_reset_and_reopen() {
        let mut conn = fresh("deleted");
        let id: crate::ids::CourseId = courses_table::table.select(courses_table::id).first(&mut conn).unwrap();
        courses::delete(&mut conn, id).unwrap();
        crate::commands::reset_progress_in(&mut conn).unwrap();
        drop(conn);
        let mut conn = db::open(&path("deleted")).unwrap();
        assert_eq!(courses_table::table.count().get_result::<i64>(&mut conn).unwrap(), 0);
        assert_eq!(lessons::table.count().get_result::<i64>(&mut conn).unwrap(), 0);
        assert_eq!(words::table.count().get_result::<i64>(&mut conn).unwrap(), 0);
        assert_eq!(db::foreign_key_violations(&mut conn).unwrap(), 0);
    }
}
