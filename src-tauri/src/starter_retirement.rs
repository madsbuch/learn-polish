//! Retire the old bundled courses without treating a learner's edits as defaults.
//!
//! Only definitions that match a frozen fingerprint of a shipped course are
//! removed. Practice progress is deliberately absent from that fingerprint;
//! changed content, added lessons, shared lessons, songs and sources are kept.

use diesel::prelude::*;
use sha2::{Digest, Sha256};

use crate::db::{self, Conn};
use crate::error::{Error, Result};
use crate::schema::{lessons, conversation_lines, course_lessons, sentences, songs, courses, wiki_sources, words};
#[allow(unused_imports)]
use crate::ids::*;

pub(crate) const RETIRED: &str = "bundled_courses_retired_v1";

// SHA-256 of the definitions shipped before default courses were removed.
// These hashes contain no vocabulary or lesson content and cannot seed courses.
const LEGACY_COURSES: &[(&str, &str)] = &[
    ("basics", "1a487251ac72c0d891e1b10298d4f9fe5131a3ff7dbd86dd62b73bcae29d1798"),
    ("family", "b6588156a97dd9656cbf49f77b07c022fedd70842fdc0966ffafe71dfecc56cd"),
    ("food", "881360c3501436cb399921844bd1c758a3a5d799f20ae5ea9d49402121e24f71"),
    ("shopping", "76fdee68bc00a9dc68e16a419d150faa3f287446ae1e78e89f22ff3f23c71255"),
    ("travel", "c8afb2736c7aecf301dee16ae24d3689eed4acf8612cd680cc11aad0a4c51bb3"),
    ("home", "18cfa4918da9a29a48aa7d0310d57f67b9ccc963ce5eb0b9f2cd35d77cf7dff3"),
    ("work", "83774d9a597049de32d0fbcb8f56b96b2c2b70757993f22d4f35814a73a23429"),
    ("health", "e0ea8d970505cde13856a322f37cb3129a59fd6d17d9673256a7381240242922"),
    ("time-nature", "e4cbcf0b4949f73b80fb323aa50bd46fd6e4e4047be521a238d418e7eeb4e5aa"),
    ("leisure", "2adf2169082039e2e0bb08e8189282504ed2a4d4b28e8a2ec53f335baf1b83e4"),
    ("grammar-essentials", "fe74f3c0448e09c9af96bfbf1641e6e9d1ffc0392932201f88c2a6194ff6bffb"),
];

/// Hash the complete editable definition, without database IDs or review state.
fn fingerprint(conn: &mut Conn, course_id: CourseId) -> Result<Option<String>> {
    let course: (String, String, String, String, i64, Option<FileId>) = courses::table
        .find(course_id)
        .select((courses::slug, courses::title_pl, courses::title_en, courses::summary_en, courses::created_at, courses::image_file_id))
        .first(conn)?;
    if course.4 != 0 || course.5.is_some() {
        return Ok(None);
    }
    if songs::table.filter(songs::course_id.eq(course_id)).count().get_result::<i64>(conn)? > 0 {
        return Ok(None);
    }
    let mut definition = vec![serde_json::to_value((&course.0, &course.1, &course.2, &course.3))?];
    let listed: Vec<(LessonId, i64)> = course_lessons::table
        .filter(course_lessons::course_id.eq(course_id))
        .order((course_lessons::ord, course_lessons::lesson_id))
        .select((course_lessons::lesson_id, course_lessons::ord))
        .load(conn)?;
    for (lesson_id, ord) in listed {
        if course_lessons::table.filter(course_lessons::lesson_id.eq(lesson_id)).count().get_result::<i64>(conn)? != 1
            || wiki_sources::table.filter(wiki_sources::lesson_id.eq(lesson_id)).count().get_result::<i64>(conn)? > 0
        {
            return Ok(None);
        }
        let mut lesson: (Option<String>, String, String, String, String, String, String, String, i64, String, i64, Option<SongId>) = lessons::table
            .find(lesson_id)
            .select((lessons::slug, lessons::lesson_type, lessons::lesson_type, lessons::title, lessons::summary,
                lessons::complexity, lessons::content, lessons::objectives, lessons::period_days,
                lessons::origin, lessons::challenge, lessons::song_id))
            .first(conn)?;
        // The fingerprints were taken when a lesson had a kind and a format:
        // they are rebuilt from its type, so the old courses still match.
        let (kind, format) = legacy_kind_and_format(&lesson.1);
        (lesson.1, lesson.2) = (kind.to_string(), format);
        if lesson.9 != "starter" || lesson.11.is_some() {
            return Ok(None);
        }
        let vocabulary: Vec<(i64, String, String, String)> = words::table
            .filter(words::lesson_id.eq(lesson_id)).order(words::ord)
            .select((words::ord, words::pl, words::en, words::pos)).load(conn)?;
        let phrases: Vec<(i64, String, String)> = sentences::table
            .filter(sentences::lesson_id.eq(lesson_id)).order(sentences::ord)
            .select((sentences::ord, sentences::pl, sentences::en)).load(conn)?;
        let dialogue: Vec<(i64, String, String, String, String)> = conversation_lines::table
            .filter(conversation_lines::lesson_id.eq(lesson_id)).order(conversation_lines::ord)
            .select((conversation_lines::ord, conversation_lines::speaker, conversation_lines::gender,
                conversation_lines::pl, conversation_lines::en)).load(conn)?;
        definition.push(serde_json::to_value((ord, lesson, vocabulary, phrases, dialogue))?);
    }
    Ok(Some(format!("{:x}", Sha256::digest(serde_json::to_vec(&definition)?))))
}

/// A type as the kind (drill | study) and format it was stored as before
/// lesson types were flattened.
fn legacy_kind_and_format(lesson_type: &str) -> (&'static str, String) {
    match lesson_type {
        "study" => ("study", String::new()),
        "writing" => ("study", "writing".into()),
        "listening" => ("study", "song".into()),
        drill => ("drill", drill.to_string()),
    }
}

fn retire_matching(conn: &mut Conn, manifest: &[(&str, &str)]) -> Result<()> {
    for (slug, expected) in manifest {
        let id = courses::table.filter(courses::slug.eq(slug)).select(courses::id).first::<CourseId>(conn).optional()?;
        if let Some(id) = id {
            if fingerprint(conn, id)?.as_deref() == Some(*expected) {
                crate::courses::delete(conn, id)?;
            }
        }
    }
    Ok(())
}

pub fn run(conn: &mut Conn) -> Result<()> {
    if db::get_setting(conn, RETIRED)?.is_some() {
        return Ok(());
    }
    conn.transaction::<_, Error, _>(|conn| {
        retire_matching(conn, LEGACY_COURSES)?;
        db::set_setting(conn, RETIRED, "1")?;
        // An older build must not reseed content when opening this database.
        db::set_setting(conn, "starter_seeded", "1")
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lessons as practice_lessons, schema, stats};

    fn path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("polish-retirement-{name}-{}.db", std::process::id()))
    }

    fn empty(name: &str) -> Conn {
        let file = path(name);
        let _ = std::fs::remove_file(&file);
        db::open(&file).unwrap()
    }

    fn legacy(name: &str) -> Conn {
        let mut conn = empty(name);
        db::seed_test_courses(&mut conn).unwrap();
        db::unify_courses(&mut conn).unwrap();
        db::delete_setting(&mut conn, RETIRED).unwrap();
        db::delete_setting(&mut conn, crate::demo_course::SEEDED).unwrap();
        conn
    }

    fn course(conn: &mut Conn, slug: &str) -> CourseId {
        courses::table.filter(courses::slug.eq(slug)).select(courses::id).first(conn).unwrap()
    }

    #[test]
    fn released_defaults_are_retired_once_without_resetting_the_learner() {
        let mut conn = legacy("released");
        assert_eq!(courses::table.count().get_result::<i64>(&mut conn).unwrap(), crate::convert::count(LEGACY_COURSES.len()));
        for (slug, expected) in LEGACY_COURSES {
            let id = course(&mut conn, slug);
            assert_eq!(fingerprint(&mut conn, id).unwrap().as_deref(), Some(*expected), "{slug}");
        }
        let today = db::today(&mut conn);
        stats::add_xp(&mut conn, today, 42).unwrap();
        db::set_setting(&mut conn, "daily_goal", "250").unwrap();
        diesel::update(lessons::table).set((lessons::reps.eq(3), lessons::due_day.eq(today + 8), lessons::passed.eq(1))).execute(&mut conn).unwrap();
        run(&mut conn).unwrap();
        assert_eq!(courses::table.count().get_result::<i64>(&mut conn).unwrap(), 0);
        assert_eq!(lessons::table.count().get_result::<i64>(&mut conn).unwrap(), 0);
        assert_eq!(words::table.count().get_result::<i64>(&mut conn).unwrap(), 0);
        assert_eq!(db::get_setting(&mut conn, "daily_goal").unwrap().as_deref(), Some("250"));
        assert_eq!(schema::day_totals::table.find(today).select(schema::day_totals::xp).first::<i64>(&mut conn).unwrap(), 42);
        drop(conn);
        let mut conn = db::open(&path("released")).unwrap();
        assert_eq!(courses::table.count().get_result::<i64>(&mut conn).unwrap(), 1);
        assert_eq!(courses::table.select(courses::slug).first::<String>(&mut conn).unwrap(), crate::demo_course::SLUG);
        assert_eq!(words::table.count().get_result::<i64>(&mut conn).unwrap(), 31, "the demo's words");
        assert_eq!(db::foreign_key_violations(&mut conn).unwrap(), 0);
    }

    #[test]
    fn customized_extended_shared_and_user_courses_survive() {
        let mut conn = legacy("custom");
        let renamed = course(&mut conn, "basics");
        diesel::update(courses::table.find(renamed)).set(courses::title_en.eq("My Polish basics")).execute(&mut conn).unwrap();
        let extended = course(&mut conn, "family");
        crate::courses::extend(&mut conn, extended, &[crate::courses::NewWord { pl: "moje własne słowo".into(), en: "my own word".into(), pos: "phr".into(), ..Default::default() }], &[], &[], "you").unwrap();
        let edited = course(&mut conn, "food");
        let edited_lesson = practice_lessons::for_course(&mut conn, edited).unwrap()[0].id;
        practice_lessons::update(&mut conn, edited_lesson, practice_lessons::LessonChanges { title: Some("My dialogue".into()), ..Default::default() }).unwrap();
        let shared = course(&mut conn, "shopping");
        let shared_lesson = practice_lessons::for_course(&mut conn, shared).unwrap()[0].id;
        let own = crate::courses::create_empty(&mut conn, "My practice", "Made by me").unwrap();
        // Shared, as a lesson could be before version 18 (which runs this
        // before it takes a lesson off all but one of its courses).
        diesel::sql_query("DROP INDEX course_lessons_one_course").execute(&mut conn).unwrap();
        diesel::insert_into(course_lessons::table)
            .values((course_lessons::course_id.eq(own), course_lessons::lesson_id.eq(shared_lesson), course_lessons::ord.eq(1)))
            .execute(&mut conn)
            .unwrap();
        let changed_word = course(&mut conn, "travel");
        let word = schema::course_words::table.filter(schema::course_words::course_id.eq(changed_word)).select(schema::course_words::id).first::<WordId>(&mut conn).unwrap();
        diesel::update(words::table.find(word)).set(words::en.eq("my edited translation")).execute(&mut conn).unwrap();
        let scheduled = course(&mut conn, "home");
        let scheduled_lesson = practice_lessons::for_course(&mut conn, scheduled).unwrap()[0].id;
        practice_lessons::update(&mut conn, scheduled_lesson, practice_lessons::LessonChanges { period_days: Some(19), ..Default::default() }).unwrap();
        run(&mut conn).unwrap();
        let retained: Vec<CourseId> = courses::table.select(courses::id).load(&mut conn).unwrap();
        for id in [renamed, extended, edited, shared, changed_word, scheduled, own] {
            assert!(retained.contains(&id), "customized course {id} was lost");
        }
        assert_eq!(retained.len(), 7);
        assert_eq!(course_lessons::table.filter(course_lessons::lesson_id.eq(shared_lesson)).count().get_result::<i64>(&mut conn).unwrap(), 2);
        assert_eq!(db::foreign_key_violations(&mut conn).unwrap(), 0);
        // Once retained, restoring a title later must not trigger another sweep.
        diesel::update(courses::table.find(renamed)).set(courses::title_en.eq("Meeting for the first time")).execute(&mut conn).unwrap();
        run(&mut conn).unwrap();
        assert_eq!(courses::table.count().get_result::<i64>(&mut conn).unwrap(), 7);
        drop(conn);
        let mut conn = db::open(&path("custom")).unwrap();
        assert_eq!(courses::table.count().get_result::<i64>(&mut conn).unwrap(), 8);
        assert_eq!(courses::table.filter(courses::slug.eq(crate::demo_course::SLUG)).count().get_result::<i64>(&mut conn).unwrap(), 1);
        // Opening it again upgrades it: the shared lesson keeps one of its courses.
        assert_eq!(course_lessons::table.filter(course_lessons::lesson_id.eq(shared_lesson)).count().get_result::<i64>(&mut conn).unwrap(), 1);
        assert!(practice_lessons::load(&mut conn, shared_lesson).is_ok());
    }
}
