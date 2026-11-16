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
        }
        db::set_setting(conn, SEEDED, "1")
    })
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
        let (id, slug): (i64, String) = courses_table::table.select((courses_table::id, courses_table::slug)).first(&mut conn).unwrap();
        assert_eq!(slug, SLUG);
        assert_eq!(words::table.count().get_result::<i64>(&mut conn).unwrap(), 12);
        assert_eq!(sentences::table.count().get_result::<i64>(&mut conn).unwrap(), 4);
        assert_eq!(conversation_lines::table.count().get_result::<i64>(&mut conn).unwrap(), 6);
        assert_eq!(lessons::table.count().get_result::<i64>(&mut conn).unwrap(), 4);
        assert_eq!(lessons::table.filter(lessons::kind.ne("drill")).count().get_result::<i64>(&mut conn).unwrap(), 0);
        assert!(!crate::lessons::overview(&mut conn).unwrap().next.is_empty());
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
        let id: i64 = courses_table::table.select(courses_table::id).first(&mut conn).unwrap();
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
