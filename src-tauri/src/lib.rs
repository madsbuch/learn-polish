mod ai;
mod cast;
mod commands;
mod content;
mod db;
mod error;
mod models;
mod playlists;
mod programmes;
mod session;
mod songs;
mod srs;
mod stats;
mod tutor;

use std::sync::Mutex;

use tauri::Manager;

use db::Db;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_songplayer::init())
        .setup(|app| {
            let path = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("no writable app data directory: {e}"))?
                .join("learn-polish.db");

            let connection = db::open(&path).map_err(|e| e.to_string())?;
            app.manage(Db(Mutex::new(connection)));
            app.manage(cast::CastSessions::default());
            songs::resume_pending(app.handle());
            songs::sync_media(app.handle());
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
            commands::browse_catalogue,
            commands::reset_progress,
            commands::ai_set_key,
            commands::ai_clear_key,
            commands::ai_test_key,
            commands::ai_list_models,
            commands::ai_explain_stream,
            tutor::tutor_send,
            tutor::tutor_history,
            tutor::tutor_clear,
            commands::programme_delete,
            commands::set_focus,
            commands::export_database,
            commands::list_voices,
            commands::preview_voice,
            commands::speak,
            commands::audio_is_cached,
            commands::cache_audio,
            commands::topic_audio_lines,
            commands::audio_cache_stats,
            commands::clear_audio_cache,
            commands::kie_set_key,
            commands::kie_clear_key,
            songs::kie_credits,
            songs::songs_list,
            songs::song_draft,
            songs::song_create,
            songs::song_retry,
            songs::song_delete,
            songs::song_track_delete,
            songs::song_track_played,
            songs::song_track_lyrics,
            playlists::playlists_list,
            playlists::playlist_create,
            playlists::playlist_rename,
            playlists::playlist_delete,
            playlists::playlist_add,
            playlists::playlist_remove,
            playlists::playlist_move,
            playlists::song_track_save,
            playlists::song_track_share_file,
            cast::cast_discover,
            cast::cast_start,
            cast::cast_command,
            cast::cast_set_repeat,
            cast::cast_state,
            cast::cast_control_info,
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

        assert!(stats::complete_day(&conn, today, plan.topic.id).unwrap(), "first completion");
        assert!(!stats::complete_day(&conn, today, plan.topic.id).unwrap(), "not counted twice");

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
    fn the_chat_model_is_never_guessed() {
        let conn = scratch_db("model");
        // Model ids go stale, so nothing is assumed on a fresh install.
        assert_eq!(ai::stored_model(&conn, "ai_model"), "");
        let error = ai::require_model(&conn).unwrap_err().to_string();
        assert!(error.contains("Settings"), "unhelpful message: {error}");

        db::set_setting(&conn, "ai_model", "anthropic/claude-haiku-4.5").unwrap();
        assert_eq!(ai::require_model(&conn).unwrap(), "anthropic/claude-haiku-4.5");
    }

    #[test]
    fn voices_are_chosen_by_speaker_gender() {
        let conn = scratch_db("voices");
        // Speech is pinned, so there are always working voices out of the box.
        assert_eq!(ai::voice_for(&conn, "f"), ai::DEFAULT_FEMALE_VOICE);
        assert_eq!(ai::voice_for(&conn, "m"), ai::DEFAULT_MALE_VOICE);
        // Anything that is not a dialogue line reads in the female voice.
        assert_eq!(ai::voice_for(&conn, ""), ai::DEFAULT_FEMALE_VOICE);

        db::set_setting(&conn, "tts_voice_female", "Leda").unwrap();
        db::set_setting(&conn, "tts_voice_male", "Charon").unwrap();
        assert_eq!(ai::voice_for(&conn, "f"), "Leda");
        assert_eq!(ai::voice_for(&conn, "m"), "Charon");

        // A voice of the wrong gender, or one Gemini does not have, is ignored
        // rather than sent to the API to be rejected.
        db::set_setting(&conn, "tts_voice_female", "Charon").unwrap();
        assert_eq!(ai::voice_for(&conn, "f"), ai::DEFAULT_FEMALE_VOICE);
        db::set_setting(&conn, "tts_voice_male", "NotAVoice").unwrap();
        assert_eq!(ai::voice_for(&conn, "m"), ai::DEFAULT_MALE_VOICE);
    }

    #[test]
    fn dialogue_lines_carry_the_speaker_gender() {
        let conn = scratch_db("gender");
        let topic = db::require_topic_id(&conn, 4).unwrap(); // the clothes shop
        let lines = session::dialogue_lines(&conn, topic).unwrap();
        assert!(!lines.is_empty());
        for line in &lines {
            assert!(line.gender == "f" || line.gender == "m", "{:?}", line);
        }
        // Sprzedawczyni is a saleswoman, Mads is a man: two voices, not one.
        let genders: std::collections::HashSet<_> =
            lines.iter().map(|l| l.gender.clone()).collect();
        assert_eq!(genders.len(), 2, "the shop dialogue should use both voices");
    }

    #[test]
    fn audio_cache_round_trips_and_is_keyed_by_voice() {
        let conn = scratch_db("audio");
        let insert = |voice: &str, bytes: &[u8]| {
            conn.execute(
                "INSERT INTO audio_cache (cache_key, model, voice, text, mime, bytes, created_at)
                 VALUES (?1, 'm', ?2, 'Cześć', 'audio/mpeg', ?3, 0)",
                rusqlite::params![format!("m\u{1}{voice}\u{1}Cześć"), voice, bytes],
            )
            .unwrap();
        };
        insert("Kore", &[1, 2, 3]);
        insert("Puck", &[9, 9]);

        let stored: Vec<u8> = conn
            .query_row(
                "SELECT bytes FROM audio_cache WHERE voice = 'Kore'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, vec![1, 2, 3], "blobs must survive the round trip");

        let total: i64 = conn
            .query_row("SELECT COALESCE(SUM(LENGTH(bytes)), 0) FROM audio_cache", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(total, 5, "two voices of the same line are stored separately");
    }

    #[test]
    fn songs_round_trip_with_their_lyrics_and_takes() {
        let conn = scratch_db("songs");
        let draft = songs::normalize_draft(songs::SongDraft {
            title: "Kawa".into(),
            chorus_pl: "Poproszę kawę".into(),
            chorus_en: "A coffee, please".into(),
            style: String::new(),
            vocal_gender: "f".into(),
            sections: vec![songs::SongSection {
                kind: "chorus".into(),
                lines: vec![songs::SongLine { en: "A coffee, please".into(), pl: "Poproszę kawę".into() }],
            }],
        })
        .unwrap();
        assert!(!draft.style.is_empty(), "a default style is filled in");

        assert_eq!(songs::suno_model(&conn), songs::DEFAULT_SUNO_MODEL);
        let id = songs::insert_song(&conn, &draft, "V6").unwrap();
        conn.execute(
            "INSERT INTO song_tracks (song_id, ord, remote_id, audio_path, duration)
             VALUES (?1, 0, 'r1', '/tmp/a.mp3', 120.5)",
            [id],
        )
        .unwrap();

        let listed = songs::list_songs(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].status, "generating");
        assert_eq!(listed[0].sections, draft.sections, "lyrics survive storage");
        assert_eq!(listed[0].tracks.len(), 1);
        assert_eq!(listed[0].tracks[0].duration, 120.5);

        // The songwriter falls back to the tutor model until it has its own.
        db::set_setting(&conn, "ai_model", "tutor/model").unwrap();
        assert_eq!(songs::songwriter_model(&conn).unwrap(), "tutor/model");
        db::set_setting(&conn, "songwriter_model", "writer/model").unwrap();
        assert_eq!(songs::songwriter_model(&conn).unwrap(), "writer/model");
    }

    #[test]
    fn the_tutor_builds_a_programme_that_can_be_practised() {
        let conn = scratch_db("programme");
        let course_topic = session::build(&conn).unwrap().topic.id;

        // "Can you build me a programme for learning the colours?"
        let result = tutor::run_tool_for_test(
            &conn,
            "create_programme",
            serde_json::json!({
                "title": "Colours",
                "title_pl": "Kolory",
                "summary": "The basic colours.",
                "words": [
                    { "pl": "czerwony", "en": "red", "pos": "adj" },
                    { "pl": "niebieski", "en": "blue", "pos": "adj" },
                    { "pl": "zielony", "en": "green", "pos": "Adjective" },
                    { "pl": "kolor", "en": "colour", "pos": "n-m" },
                    // A duplicate is skipped, not an error.
                    { "pl": "czerwony", "en": "red", "pos": "adj" }
                ],
                "sentences": [
                    { "pl": "Mój samochód jest czerwony.", "en": "My car is red." },
                    { "pl": "Niebo jest niebieskie.", "en": "The sky is blue." }
                ],
                "dialogue": [
                    { "speaker": "Ola", "gender": "f", "pl": "Jaki jest twój ulubiony kolor?", "en": "What is your favourite colour?" },
                    { "speaker": "Tomek", "gender": "m", "pl": "Zielony.", "en": "Green." }
                ]
            }),
        );
        assert!(result.get("error").is_none(), "{result}");
        let created = &result["created"];
        assert_eq!(created["words"], 4);
        assert_eq!(created["sentences"], 2);
        assert_eq!(created["dialogue_lines"], 2);
        assert_eq!(created["skipped_duplicates"], 1);
        assert_eq!(result["_action"]["kind"], "programmeCreated");
        let topic_id = created["topic_id"].as_i64().unwrap();

        // A course word can appear in a programme too.
        let shared: i64 = conn
            .query_row("SELECT COUNT(*) FROM words WHERE pl = 'kolor'", [], |r| r.get(0))
            .unwrap();
        assert!(shared >= 1);

        // Practising it on demand works like any topic, and leaves the course alone.
        let plan = session::build_for(&conn, topic_id).unwrap();
        assert_eq!(plan.topic.kind, "programme");
        assert_eq!(plan.new_words, 4);
        assert_eq!(plan.exercises[0].kind, "dialogueRead");
        assert_eq!(session::build(&conn).unwrap().topic.id, course_topic);

        // The daily focus moves the check-in to the programme…
        let focus = tutor::run_tool_for_test(&conn, "set_daily_focus", serde_json::json!({ "topic_id": topic_id }));
        assert_eq!(focus["_action"]["kind"], "focusSet");
        assert_eq!(session::build(&conn).unwrap().topic.id, topic_id);

        // …until every word in it has been met; then the course takes over again.
        let today = db::today(&conn);
        for exercise in &plan.exercises {
            if exercise.item_kind == "word" {
                srs::save(&conn, "word", exercise.item_id, &srs::grade(srs::ReviewState::fresh(today), srs::GOOD, today)).unwrap();
            }
        }
        assert!(stats::complete_day(&conn, today, topic_id).unwrap());
        assert_eq!(session::focus_topic_id(&conn), None);
        assert_eq!(session::build(&conn).unwrap().topic.id, course_topic, "rotation did not move");

        // Course topics are read-only; programmes can be deleted with their history.
        let refused = tutor::run_tool_for_test(&conn, "add_to_programme", serde_json::json!({ "topic_id": course_topic, "words": [] }));
        assert!(refused["error"].as_str().unwrap().contains("built-in"));
        let deleted = tutor::run_tool_for_test(&conn, "delete_programme", serde_json::json!({ "topic_id": topic_id }));
        assert_eq!(deleted["deleted"], "Colours");
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM words WHERE topic_id = ?1", [topic_id], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn playlists_hold_takes_in_order_and_lose_deleted_ones() {
        let conn = scratch_db("playlists");
        conn.execute_batch(
            "INSERT INTO songs (id, title, chorus_pl, chorus_en, style, sections, lyrics, suno_model, status, created_at)
             VALUES (1, 'Kawa', 'Poproszę kawę', 'A coffee, please', 'pop', '[]', '', 'V6', 'ready', 0);
             INSERT INTO song_tracks (id, song_id, ord, remote_id, audio_path) VALUES
                 (10, 1, 0, 'a', '/tmp/a.mp3'), (11, 1, 1, 'b', '/tmp/b.mp3');",
        )
        .unwrap();

        let list = playlists::create(&conn, "  Morning   run ", &[10, 11, 10]).unwrap();
        assert_eq!(list.name, "Morning run");
        let order = |p: &playlists::Playlist| p.entries.iter().map(|e| e.track_id).collect::<Vec<_>>();
        assert_eq!(order(&list), vec![10, 11, 10], "the same take may appear twice");

        let moved = playlists::move_entry(&conn, list.id, list.entries[2].id, -2).unwrap();
        assert_eq!(order(&moved), vec![10, 10, 11]);
        // Moving past either end stays in bounds.
        let moved = playlists::move_entry(&conn, list.id, moved.entries[0].id, -5).unwrap();
        assert_eq!(order(&moved), vec![10, 10, 11]);

        // Deleting a take takes it out of every playlist.
        conn.execute("DELETE FROM song_tracks WHERE id = 10", []).unwrap();
        assert_eq!(order(&playlists::list(&conn).unwrap()[0]), vec![11]);
        assert!(playlists::create(&conn, "   ", &[]).is_err());
    }

    #[test]
    fn the_tutor_reports_progress_and_finds_vocabulary() {
        let conn = scratch_db("tutor-read");
        let progress = tutor::run_tool_for_test(&conn, "get_progress", serde_json::json!({}));
        assert_eq!(progress["words_met"], 0);
        let found = tutor::run_tool_for_test(&conn, "search_vocabulary", serde_json::json!({ "query": "dzień dobry" }));
        assert!(!found["words"].as_array().unwrap().is_empty());
        let topics = tutor::run_tool_for_test(&conn, "list_topics", serde_json::json!({}));
        assert_eq!(topics.as_array().unwrap().len(), 10);
        let unknown = tutor::run_tool_for_test(&conn, "fly_to_the_moon", serde_json::json!({}));
        assert!(unknown["error"].is_string());
    }

    #[test]
    fn a_danish_era_database_is_upgraded_in_place() {
        let path = std::env::temp_dir()
            .join("learn-polish-tests")
            .join(format!("legacy-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        {
            let old = Connection::open(&path).unwrap();
            old.execute_batch(
                "CREATE TABLE topics (id INTEGER PRIMARY KEY, slug TEXT NOT NULL UNIQUE, ord INTEGER NOT NULL,
                     title_pl TEXT NOT NULL, title_en TEXT NOT NULL, title_da TEXT NOT NULL,
                     summary_en TEXT NOT NULL, summary_da TEXT NOT NULL);
                 CREATE TABLE words (id INTEGER PRIMARY KEY, topic_id INTEGER NOT NULL REFERENCES topics(id),
                     ord INTEGER NOT NULL, pl TEXT NOT NULL UNIQUE, en TEXT NOT NULL, da TEXT NOT NULL, pos TEXT NOT NULL);
                 CREATE TABLE sentences (id INTEGER PRIMARY KEY, topic_id INTEGER NOT NULL REFERENCES topics(id),
                     ord INTEGER NOT NULL, pl TEXT NOT NULL UNIQUE, en TEXT NOT NULL, da TEXT NOT NULL);
                 CREATE TABLE conversation_lines (id INTEGER PRIMARY KEY, topic_id INTEGER NOT NULL REFERENCES topics(id),
                     ord INTEGER NOT NULL, speaker TEXT NOT NULL, pl TEXT NOT NULL, en TEXT NOT NULL, da TEXT NOT NULL);
                 CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE reviews (kind TEXT NOT NULL, item_id INTEGER NOT NULL, due_day INTEGER NOT NULL,
                     interval INTEGER NOT NULL, ease REAL NOT NULL, reps INTEGER NOT NULL, lapses INTEGER NOT NULL,
                     last_day INTEGER NOT NULL, PRIMARY KEY (kind, item_id));
                 CREATE TABLE chat_messages (id INTEGER PRIMARY KEY, topic_id INTEGER NOT NULL, role TEXT NOT NULL,
                     content TEXT NOT NULL, created_at INTEGER NOT NULL);
                 INSERT INTO topics VALUES (1, 'basics', 1, 'Pierwsze spotkanie', 'Meeting', 'Møde', 'Hi', 'Hej');
                 INSERT INTO words VALUES (7, 1, 0, 'cześć', 'hi, bye', 'hej', 'interj');
                 INSERT INTO reviews VALUES ('word', 7, 100, 3, 2.5, 2, 0, 97);
                 INSERT INTO settings VALUES ('source_language', 'da'), ('content_version', '2');",
            )
            .unwrap();
        }

        let conn = db::open(&path).expect("upgrade");
        let (id, en): (i64, String) = conn
            .query_row("SELECT id, en FROM words WHERE pl = 'cześć'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(id, 7, "row ids survive, so review history does too");
        assert_eq!(en, "hi, bye");
        assert_eq!(srs::load(&conn, "word", 7).unwrap().unwrap().reps, 2);
        let columns: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('words')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(!columns.contains(&"da".to_string()));
        assert_eq!(db::get_setting(&conn, "source_language").unwrap(), None);
        // The rest of the curriculum seeds on top, without duplicating.
        let topics: i64 = conn.query_row("SELECT COUNT(*) FROM topics", [], |r| r.get(0)).unwrap();
        assert_eq!(topics, 10);
        let basics_words: i64 = conn
            .query_row("SELECT COUNT(*) FROM words WHERE pl = 'cześć'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(basics_words, 1);

        // Opening again is a no-op.
        drop(conn);
        db::open(&path).expect("second open");
    }
}
