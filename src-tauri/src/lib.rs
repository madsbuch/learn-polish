mod ai;
mod commands;
mod content;
mod db;
mod error;
mod models;
mod session;
mod srs;
mod stats;

use std::sync::Mutex;

use tauri::Manager;

use db::Db;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let path = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("no writable app data directory: {e}"))?
                .join("learn-polish.db");

            let connection = db::open(&path).map_err(|e| e.to_string())?;
            app.manage(Db(Mutex::new(connection)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::init_app,
            commands::get_settings,
            commands::update_settings,
            commands::get_session,
            commands::submit_answer,
            commands::complete_session,
            commands::get_stats,
            commands::get_topic_progress,
            commands::get_dialogue,
            commands::browse_words,
            commands::browse_sentences,
            commands::reset_progress,
            commands::ai_set_key,
            commands::ai_clear_key,
            commands::ai_test_key,
            commands::ai_list_models,
            commands::ai_history,
            commands::ai_clear_chat,
            commands::ai_chat,
            commands::ai_explain,
            commands::ai_feedback,
            commands::ai_drill,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the application");
}

#[cfg(test)]
mod integration {
    //! Drives a real database through a full check-in, without Tauri.

    use super::*;
    use rusqlite::Connection;

    fn scratch_db(name: &str) -> Connection {
        let path = std::env::temp_dir()
            .join("learn-polish-tests")
            .join(format!("{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = db::open(&path).expect("open");
        // Pin the clock to a known day so the assertions do not depend on today.
        db::set_setting(&conn, "tz_offset_minutes", "0").unwrap();
        conn
    }

    #[test]
    fn a_fresh_install_seeds_the_whole_curriculum() {
        let conn = scratch_db("seed");
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT COUNT(*) FROM topics"), 10);
        assert!(count("SELECT COUNT(*) FROM words") >= 2000);
        assert_eq!(count("SELECT COUNT(*) FROM sentences"), 200);
        assert!(count("SELECT COUNT(*) FROM conversation_lines") >= 100);

        // Opening again must not duplicate anything.
        drop(conn);
        let conn = scratch_db("seed");
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM topics", [], |r| r.get(0))
                .unwrap(),
            10
        );
    }

    #[test]
    fn the_daily_session_is_built_around_one_conversation() {
        let conn = scratch_db("session");
        let plan = session::build(&conn).expect("plan");

        assert_eq!(plan.topic.ord, 1, "day one starts at the first conversation");
        assert!(!plan.dialogue.is_empty());
        assert_eq!(
            plan.exercises.first().map(|e| e.kind.as_str()),
            Some("dialogueRead"),
            "the conversation is read before anything else"
        );
        assert!(plan.new_words > 0);
        assert_eq!(plan.review_words, 0, "nothing is due on a fresh install");
        assert!(
            plan.exercises.iter().any(|e| e.kind == "dialogueGap"),
            "the conversation is also drilled"
        );

        // Every multiple-choice exercise offers its own answer.
        for exercise in &plan.exercises {
            if !exercise.options.is_empty() {
                assert!(
                    exercise.options.contains(&exercise.answer),
                    "{} lost its answer",
                    exercise.key
                );
                assert!(exercise.options.len() >= 2);
            }
        }
    }

    #[test]
    fn answering_schedules_reviews_and_advances_the_rotation() {
        let conn = scratch_db("progress");
        let today = db::today(&conn);
        let plan = session::build(&conn).expect("plan");

        let mut graded = 0;
        for exercise in &plan.exercises {
            if exercise.item_kind != "word" {
                continue;
            }
            let state = srs::load(&conn, "word", exercise.item_id)
                .unwrap()
                .unwrap_or_else(|| srs::ReviewState::fresh(today));
            srs::save(
                &conn,
                "word",
                exercise.item_id,
                &srs::grade(state, srs::GOOD, today),
            )
            .unwrap();
            stats::record_answer(&conn, today, 6, true).unwrap();
            graded += 1;
        }
        assert!(graded > 0);

        assert!(stats::complete_day(&conn, today).unwrap(), "first completion");
        assert!(!stats::complete_day(&conn, today).unwrap(), "not counted twice");

        let summary = stats::collect(&conn).unwrap();
        assert_eq!(summary.current_streak, 1);
        assert!(summary.completed_today);
        assert!(summary.total_xp > 0);
        assert!(summary.words_seen > 0);
        assert!(
            summary.achievements.iter().any(|a| a.code == "first_day" && a.unlocked),
            "finishing a day unlocks the first achievement"
        );

        // Tomorrow moves to the next conversation, and today's words are not
        // offered as new again.
        let tomorrow = session::build(&conn).expect("next plan");
        assert_eq!(tomorrow.topic.ord, 2);
        let seen: std::collections::HashSet<i64> = plan
            .exercises
            .iter()
            .filter(|e| e.item_kind == "word")
            .map(|e| e.item_id)
            .collect();
        assert!(
            tomorrow
                .exercises
                .iter()
                .filter(|e| e.item_kind == "word" && e.is_new)
                .all(|e| !seen.contains(&e.item_id)),
            "already-learned words must not come back as new"
        );
    }

    #[test]
    fn switching_source_language_swaps_every_gloss() {
        let conn = scratch_db("lang");
        let english = session::build(&conn).unwrap();

        db::set_setting(&conn, "source_language", "da").unwrap();
        let danish = session::build(&conn).unwrap();

        assert_ne!(english.topic.title, danish.topic.title);
        assert_eq!(english.topic.title_pl, danish.topic.title_pl);
        assert!(!danish.dialogue.is_empty());
        assert_ne!(danish.dialogue[0].meaning, english.dialogue[0].meaning);
    }
}
