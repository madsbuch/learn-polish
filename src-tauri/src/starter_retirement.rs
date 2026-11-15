//! Retire the old bundled courses without treating a learner's edits as defaults.
//!
//! Only definitions that match a frozen fingerprint of a shipped course are
//! removed. Practice progress is deliberately absent from that fingerprint;
//! changed content, added blocks, shared blocks, songs and sources are kept.

use diesel::prelude::*;
use sha2::{Digest, Sha256};

use crate::db::{self, Conn};
use crate::error::{Error, Result};
use crate::schema::{blocks, conversation_lines, course_blocks, sentences, songs, topics, wiki_sources, words};

const RETIRED: &str = "bundled_courses_retired_v1";

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
fn fingerprint(conn: &mut Conn, topic_id: i64) -> Result<Option<String>> {
    let course: (String, String, String, String, i64, Option<i64>) = topics::table
        .find(topic_id)
        .select((topics::slug, topics::title_pl, topics::title_en, topics::summary_en, topics::created_at, topics::image_file_id))
        .first(conn)?;
    if course.4 != 0 || course.5.is_some() {
        return Ok(None);
    }
    if songs::table.filter(songs::topic_id.eq(topic_id)).count().get_result::<i64>(conn)? > 0 {
        return Ok(None);
    }
    let mut definition = vec![serde_json::to_value((&course.0, &course.1, &course.2, &course.3))?];
    let listed: Vec<(i64, i64)> = course_blocks::table
        .filter(course_blocks::topic_id.eq(topic_id))
        .order((course_blocks::ord, course_blocks::block_id))
        .select((course_blocks::block_id, course_blocks::ord))
        .load(conn)?;
    for (block_id, ord) in listed {
        if course_blocks::table.filter(course_blocks::block_id.eq(block_id)).count().get_result::<i64>(conn)? != 1
            || wiki_sources::table.filter(wiki_sources::block_id.eq(block_id)).count().get_result::<i64>(conn)? > 0
        {
            return Ok(None);
        }
        let block: (Option<String>, String, String, String, String, String, String, String, i64, String, i64, Option<i64>) = blocks::table
            .find(block_id)
            .select((blocks::slug, blocks::kind, blocks::format, blocks::title, blocks::summary,
                blocks::complexity, blocks::lesson, blocks::objectives, blocks::period_days,
                blocks::origin, blocks::challenge, blocks::song_id))
            .first(conn)?;
        if block.9 != "starter" || block.11.is_some() {
            return Ok(None);
        }
        let vocabulary: Vec<(i64, String, String, String)> = words::table
            .filter(words::block_id.eq(block_id)).order(words::ord)
            .select((words::ord, words::pl, words::en, words::pos)).load(conn)?;
        let phrases: Vec<(i64, String, String)> = sentences::table
            .filter(sentences::block_id.eq(block_id)).order(sentences::ord)
            .select((sentences::ord, sentences::pl, sentences::en)).load(conn)?;
        let dialogue: Vec<(i64, String, String, String, String)> = conversation_lines::table
            .filter(conversation_lines::block_id.eq(block_id)).order(conversation_lines::ord)
            .select((conversation_lines::ord, conversation_lines::speaker, conversation_lines::gender,
                conversation_lines::pl, conversation_lines::en)).load(conn)?;
        definition.push(serde_json::to_value((ord, block, vocabulary, phrases, dialogue))?);
    }
    Ok(Some(format!("{:x}", Sha256::digest(serde_json::to_vec(&definition)?))))
}

fn retire_matching(conn: &mut Conn, manifest: &[(&str, &str)]) -> Result<()> {
    for (slug, expected) in manifest {
        let id = topics::table.filter(topics::slug.eq(slug)).select(topics::id).first::<i64>(conn).optional()?;
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
    use crate::{blocks as practice_blocks, courses, schema, stats};

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

    fn course(conn: &mut Conn, slug: &str) -> i64 {
        topics::table.filter(topics::slug.eq(slug)).select(topics::id).first(conn).unwrap()
    }

    #[test]
    fn released_defaults_are_retired_once_without_resetting_the_learner() {
        let mut conn = legacy("released");
        assert_eq!(topics::table.count().get_result::<i64>(&mut conn).unwrap(), LEGACY_COURSES.len() as i64);
        for (slug, expected) in LEGACY_COURSES {
            let id = course(&mut conn, slug);
            assert_eq!(fingerprint(&mut conn, id).unwrap().as_deref(), Some(*expected), "{slug}");
        }
        let today = db::today(&mut conn);
        stats::add_xp(&mut conn, today, 42).unwrap();
        db::set_setting(&mut conn, "daily_goal", "250").unwrap();
        diesel::update(blocks::table).set((blocks::reps.eq(3), blocks::due_day.eq(today + 8), blocks::passed.eq(1))).execute(&mut conn).unwrap();
        run(&mut conn).unwrap();
        assert_eq!(topics::table.count().get_result::<i64>(&mut conn).unwrap(), 0);
        assert_eq!(blocks::table.count().get_result::<i64>(&mut conn).unwrap(), 0);
        assert_eq!(words::table.count().get_result::<i64>(&mut conn).unwrap(), 0);
        assert_eq!(db::get_setting(&mut conn, "daily_goal").unwrap().as_deref(), Some("250"));
        assert_eq!(schema::day_log::table.find(today).select(schema::day_log::xp).first::<i64>(&mut conn).unwrap(), 42);
        drop(conn);
        let mut conn = db::open(&path("released")).unwrap();
        assert_eq!(topics::table.count().get_result::<i64>(&mut conn).unwrap(), 1);
        assert_eq!(topics::table.select(topics::slug).first::<String>(&mut conn).unwrap(), crate::demo_course::SLUG);
        assert_eq!(words::table.count().get_result::<i64>(&mut conn).unwrap(), 12);
        assert_eq!(db::foreign_key_violations(&mut conn).unwrap(), 0);
    }

    #[test]
    fn customized_extended_shared_and_user_courses_survive() {
        let mut conn = legacy("custom");
        let renamed = course(&mut conn, "basics");
        diesel::update(topics::table.find(renamed)).set(topics::title_en.eq("My Polish basics")).execute(&mut conn).unwrap();
        let extended = course(&mut conn, "family");
        courses::extend(&mut conn, extended, &[courses::NewWord { pl: "moje własne słowo".into(), en: "my own word".into(), pos: "phr".into() }], &[], &[], "you").unwrap();
        let edited = course(&mut conn, "food");
        let edited_block = practice_blocks::for_topic(&mut conn, edited).unwrap()[0].id;
        practice_blocks::update(&mut conn, edited_block, practice_blocks::BlockChanges { title: Some("My dialogue".into()), ..Default::default() }).unwrap();
        let shared = course(&mut conn, "shopping");
        let shared_block = practice_blocks::for_topic(&mut conn, shared).unwrap()[0].id;
        let own = courses::create_empty(&mut conn, "My practice", "Made by me").unwrap();
        courses::add_block(&mut conn, own, shared_block).unwrap();
        let changed_word = course(&mut conn, "travel");
        let word = schema::course_words::table.filter(schema::course_words::topic_id.eq(changed_word)).select(schema::course_words::id).first::<i64>(&mut conn).unwrap();
        diesel::update(words::table.find(word)).set(words::en.eq("my edited translation")).execute(&mut conn).unwrap();
        let scheduled = course(&mut conn, "home");
        let scheduled_block = practice_blocks::for_topic(&mut conn, scheduled).unwrap()[0].id;
        practice_blocks::update(&mut conn, scheduled_block, practice_blocks::BlockChanges { period_days: Some(19), ..Default::default() }).unwrap();
        run(&mut conn).unwrap();
        let retained: Vec<i64> = topics::table.select(topics::id).load(&mut conn).unwrap();
        for id in [renamed, extended, edited, shared, changed_word, scheduled, own] {
            assert!(retained.contains(&id), "customized course {id} was lost");
        }
        assert_eq!(retained.len(), 7);
        assert_eq!(course_blocks::table.filter(course_blocks::block_id.eq(shared_block)).count().get_result::<i64>(&mut conn).unwrap(), 2);
        assert_eq!(db::foreign_key_violations(&mut conn).unwrap(), 0);
        // Once retained, restoring a title later must not trigger another sweep.
        diesel::update(topics::table.find(renamed)).set(topics::title_en.eq("Meeting for the first time")).execute(&mut conn).unwrap();
        run(&mut conn).unwrap();
        assert_eq!(topics::table.count().get_result::<i64>(&mut conn).unwrap(), 7);
        drop(conn);
        let mut conn = db::open(&path("custom")).unwrap();
        assert_eq!(topics::table.count().get_result::<i64>(&mut conn).unwrap(), 8);
        assert_eq!(topics::table.filter(topics::slug.eq(crate::demo_course::SLUG)).count().get_result::<i64>(&mut conn).unwrap(), 1);
    }
}
