mod ai;
mod blocks;
mod cast;
mod commands;
mod content;
mod db;
mod error;
mod examiner;
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
            commands::submit_answer,
            commands::blocks_overview,
            commands::blocks_for_topic,
            commands::block_detail,
            commands::block_session,
            commands::block_finish,
            commands::block_set_period,
            commands::block_due_now,
            commands::block_delete,
            commands::block_for_song,
            commands::checkin_summary,
            examiner::checkin_start,
            examiner::checkin_send,            commands::get_stats,
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
            tutor::tutor_conversations,
            tutor::tutor_history,
            tutor::tutor_rename,
            tutor::tutor_delete,
            commands::programme_delete,
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

    /// Reopens a scratch database without wiping it, to exercise migrations.
    fn scratch_db_keep(name: &str) -> Connection {
        let path = std::env::temp_dir()
            .join("learn-polish-tests")
            .join(format!("{name}-{}.db", std::process::id()));
        db::open(&path).expect("reopen")
    }

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
    fn the_course_is_cut_into_small_blocks() {
        let conn = scratch_db("blocks");
        let basics = db::require_topic_id(&conn, 1).unwrap();
        let list = blocks::for_topic(&conn, basics).unwrap();
        let count = |format: &str| list.iter().filter(|b| b.format == format).count();
        assert_eq!(count("dialogue"), 1);
        assert!(count("words") >= 20, "words come in eights");
        assert_eq!(count("sentences"), 4, "twenty sentences in fives");
        assert!(list.iter().all(|b| b.due_day.is_none()), "nothing is scheduled before it is done");

        // A word drill is small: eight new words, met then recognised, and a memory round.
        let words = list.iter().find(|b| b.format == "words").unwrap();
        assert_eq!(words.complexity, "low");
        let plan = session::build_block(&conn, words.id).unwrap();
        assert_eq!(plan.new_words, 8);
        assert!(plan.exercises.len() <= 17, "{} exercises", plan.exercises.len());
        assert_eq!(plan.exercises[0].kind, "intro");
        assert!(plan.exercises.iter().any(|e| e.kind == "memory"));

        // The dialogue drill reads first, then drills its lines.
        let dialogue = list.iter().find(|b| b.format == "dialogue").unwrap();
        let plan = session::build_block(&conn, dialogue.id).unwrap();
        assert_eq!(plan.exercises[0].kind, "dialogueRead");
        assert!(plan.exercises.iter().any(|e| e.kind == "dialogueGap"));

        // Sentences: build each, then fill gaps from a bank that holds every answer.
        let sentences = list.iter().find(|b| b.format == "sentences").unwrap();
        assert_eq!(sentences.complexity, "medium");
        let plan = session::build_block(&conn, sentences.id).unwrap();
        assert!(plan.exercises.iter().any(|e| e.kind == "wordOrder"));
        let gaps = plan.exercises.iter().find(|e| e.kind == "gapFill").expect("fill the gaps");
        for gap in &gaps.gaps {
            assert!(gaps.options.contains(&gap.answer), "the bank holds every answer");
        }
        for exercise in &plan.exercises {
            if !exercise.options.is_empty() && exercise.kind != "gapFill" {
                assert!(exercise.options.contains(&exercise.answer), "{} lost its answer", exercise.key);
            }
        }

        // The built-in lessons are there, as study blocks without a topic.
        let overview = blocks::overview(&conn).unwrap();
        assert!(overview.lessons.len() >= 7);
        assert!(overview.lessons.iter().all(|b| b.kind == "study"));
        assert!(overview.due.is_empty(), "nothing is due on a fresh install");
        assert_eq!(overview.next[0].topic_id, Some(basics), "the course starts at the beginning");
    }

    #[test]
    fn finishing_a_block_schedules_its_next_check_in() {
        let conn = scratch_db("schedule");
        let today = db::today(&conn);
        let basics = db::require_topic_id(&conn, 1).unwrap();
        let words = blocks::for_topic(&conn, basics)
            .unwrap()
            .into_iter()
            .find(|b| b.format == "words")
            .unwrap();
        let plan = session::build_block(&conn, words.id).unwrap();
        for exercise in plan.exercises.iter().filter(|e| e.item_kind == "word") {
            let state = srs::load(&conn, "word", exercise.item_id)
                .unwrap()
                .unwrap_or_else(|| srs::ReviewState::fresh(today));
            srs::save(&conn, "word", exercise.item_id, &srs::grade(state, srs::GOOD, today)).unwrap();
            stats::record_answer(&conn, today, 6, true).unwrap();
        }

        let done = blocks::record(&conn, words.id, blocks::result_from_score(9, 10)).unwrap();
        assert_eq!(done.due_day, Some(today + words.period_days), "first pass: back after its period");
        assert!(stats::mark_practised(&conn, today).unwrap(), "first block today");
        assert!(!stats::mark_practised(&conn, today).unwrap(), "not counted twice");

        let summary = stats::collect(&conn).unwrap();
        assert_eq!(summary.current_streak, 1);
        assert!(summary.words_seen > 0);
        assert!(summary.achievements.iter().any(|a| a.code == "first_day" && a.unlocked));

        // The next block to start is a new one, not the one just done.
        let overview = blocks::overview(&conn).unwrap();
        assert!(overview.next.iter().all(|b| b.id != words.id));
        assert_eq!(overview.done_today, 1);

        // Failing brings it back tomorrow; passing again spreads it out.
        let failed = blocks::record(&conn, words.id, "fail").unwrap();
        assert_eq!(failed.due_day, Some(today + 1));
        let passed = blocks::record(&conn, words.id, "pass").unwrap();
        assert!(passed.interval_days >= words.period_days);
    }

    #[test]
    fn the_examiner_s_verdict_schedules_the_study_block() {
        let conn = scratch_db("examiner");
        let lesson = blocks::overview(&conn).unwrap().lessons[0].clone();
        conn.execute("INSERT INTO checkins (block_id, started_at) VALUES (?1, 0)", [lesson.id]).unwrap();
        let checkin = conn.last_insert_rowid();
        let (assessment, first) = examiner::record(
            &conn,
            checkin,
            serde_json::from_str(r#"{"result":"partial","summary":"Mostly right.","to_review":["oni są"]}"#).unwrap(),
        )
        .unwrap();
        assert!(first);
        assert!(assessment.xp > 0, "a partial pass still earns something");
        let block = blocks::load(&conn, lesson.id).unwrap();
        assert_eq!(block.last_result.as_deref(), Some("partial"));
        assert_eq!(block.due_day, Some(db::today(&conn) + block.period_days));
        let detail = blocks::detail(&conn, lesson.id).unwrap();
        assert!(detail.last_feedback.unwrap().contains("oni są"), "what to review is kept for next time");
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
        assert_eq!(created["skipped_duplicates"], 1);
        assert_eq!(result["_action"]["kind"], "programmeCreated");
        let topic_id = created["topic_id"].as_i64().unwrap();

        // It is cut into drills, and they are offered first.
        let list = blocks::for_topic(&conn, topic_id).unwrap();
        let formats: Vec<&str> = list.iter().map(|b| b.format.as_str()).collect();
        assert_eq!(formats, vec!["dialogue", "words", "sentences"]);
        assert_eq!(blocks::overview(&conn).unwrap().next[0].topic_id, Some(topic_id));
        let plan = session::build_block(&conn, list[1].id).unwrap();
        assert_eq!(plan.topic.kind, "programme");
        assert_eq!(plan.new_words, 4);

        // Adding words re-cuts the drills without losing the existing ones.
        tutor::run_tool_for_test(
            &conn,
            "add_to_programme",
            serde_json::json!({ "topic_id": topic_id, "words": [{ "pl": "żółty", "en": "yellow", "pos": "adj" }] }),
        );
        let words_block = blocks::load(&conn, list[1].id).unwrap();
        assert_eq!(words_block.items, 5);

        // Course topics are read-only; programmes can be deleted with their blocks.
        let course = db::require_topic_id(&conn, 1).unwrap();
        let refused = tutor::run_tool_for_test(&conn, "add_to_programme", serde_json::json!({ "topic_id": course, "words": [] }));
        assert!(refused["error"].as_str().unwrap().contains("built-in"));
        let deleted = tutor::run_tool_for_test(&conn, "delete_programme", serde_json::json!({ "topic_id": topic_id }));
        assert_eq!(deleted["deleted"], "Colours");
        assert!(blocks::for_topic(&conn, topic_id).unwrap().is_empty());
    }

    #[test]
    fn the_tutor_writes_study_blocks_and_single_format_drills() {
        let conn = scratch_db("tutor-blocks");
        let study = tutor::run_tool_for_test(
            &conn,
            "create_study_block",
            serde_json::json!({
                "title": "Iść — to go on foot",
                "summary": "The present tense of iść.",
                "complexity": "low",
                "lesson": "**Iść**: idę, idziesz, idzie, idziemy, idziecie, idą.",
                "objectives": ["Conjugate iść in the present tense", "Say where you are going"]
            }),
        );
        assert_eq!(study["_action"]["kind"], "blockCreated", "{study}");
        let study_id = study["_action"]["blockId"].as_i64().unwrap();
        let block = blocks::load(&conn, study_id).unwrap();
        assert_eq!((block.kind.as_str(), block.complexity.as_str(), block.period_days), ("study", "low", 5));
        assert!(blocks::overview(&conn).unwrap().lessons.iter().any(|b| b.id == study_id));

        // A study block without objectives is refused: the examiner needs them.
        let refused = tutor::run_tool_for_test(
            &conn,
            "create_study_block",
            serde_json::json!({ "title": "x", "summary": "x", "complexity": "low", "lesson": "x", "objectives": [] }),
        );
        assert!(refused["error"].is_string());

        // "I meant memory cards": one drill, memory format, and nothing else made.
        let drill = tutor::run_tool_for_test(
            &conn,
            "create_drill_block",
            serde_json::json!({
                "title": "Colours — memory",
                "format": "memory",
                "complexity": "low",
                "words": [
                    { "pl": "czerwony", "en": "red", "pos": "adj" },
                    { "pl": "biały", "en": "white", "pos": "adj" },
                    { "pl": "czarny", "en": "black", "pos": "adj" },
                    { "pl": "zielony", "en": "green", "pos": "adj" }
                ]
            }),
        );
        let drill_id = drill["_action"]["blockId"].as_i64().unwrap();
        let block = blocks::load(&conn, drill_id).unwrap();
        assert_eq!((block.format.as_str(), block.items), ("memory", 4));
        assert_eq!(blocks::for_topic(&conn, block.topic_id.unwrap()).unwrap().len(), 1);
        let plan = session::build_block(&conn, drill_id).unwrap();
        assert!(plan.exercises.iter().all(|e| e.kind == "memory"));

        // Built-in blocks keep their content; only the period may change.
        let builtin = blocks::overview(&conn).unwrap().lessons[0].id;
        let edit = tutor::run_tool_for_test(&conn, "update_block", serde_json::json!({ "block_id": builtin, "title": "Mine now" }));
        assert!(edit["error"].is_string());
        let period = tutor::run_tool_for_test(&conn, "update_block", serde_json::json!({ "block_id": builtin, "period_days": 3 }));
        assert!(period.get("error").is_none(), "{period}");
        assert!(tutor::run_tool_for_test(&conn, "delete_block", serde_json::json!({ "block_id": builtin }))["error"].is_string());

        // Scheduling makes it due now.
        tutor::run_tool_for_test(&conn, "schedule_block", serde_json::json!({ "block_id": study_id }));
        assert!(blocks::overview(&conn).unwrap().due.iter().any(|b| b.id == study_id));
        let gone = tutor::run_tool_for_test(&conn, "delete_block", serde_json::json!({ "block_id": study_id }));
        assert_eq!(gone["_action"]["kind"], "blockDeleted");
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
    fn tutor_conversations_keep_their_own_history() {
        let conn = scratch_db("conversations");
        // A message from before conversations existed is adopted, not lost.
        conn.execute(
            "INSERT INTO tutor_messages (role, content, created_at) VALUES ('user', 'old question', 5)",
            [],
        )
        .unwrap();
        drop(conn);
        let conn = scratch_db_keep("conversations");
        let listed = tutor::list_conversations(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "Earlier conversation");
        assert_eq!(tutor::load_history(&conn, listed[0].id).unwrap().len(), 1);

        let id = tutor::create_conversation(&conn, "How do numbers work?").unwrap();
        assert_eq!(tutor::load_history(&conn, id).unwrap().len(), 0, "a new one starts empty");
        assert_eq!(tutor::list_conversations(&conn).unwrap().len(), 2);
    }

    #[test]
    fn a_song_gets_a_listening_block_that_goes_with_it() {
        let conn = scratch_db("listening");
        conn.execute_batch(
            r#"INSERT INTO songs (id, title, chorus_pl, chorus_en, style, sections, lyrics, suno_model, status, created_at)
               VALUES (1, 'Kawa', 'Poproszę kawę', 'A coffee, please', 'pop',
                       '[{"kind":"chorus","lines":[{"en":"A coffee, please","pl":"Poproszę kawę"}]}]',
                       '', 'V6', 'ready', 0);"#,
        )
        .unwrap();
        let made = tutor::run_tool_for_test(&conn, "create_listening_block", serde_json::json!({ "song_id": 1, "complexity": "high" }));
        let id = made["_action"]["blockId"].as_i64().expect("made");
        let block = blocks::load(&conn, id).unwrap();
        assert_eq!((block.kind.as_str(), block.format.as_str(), block.song_id), ("study", "song", Some(1)));
        assert!(blocks::detail(&conn, id).unwrap().objectives.len() >= 6, "high asks about grammar too");
        // One per song: asking again returns the same block.
        assert_eq!(blocks::create_listening(&conn, 1, None, None, "tutor").unwrap().id, id);
        // It is a lesson on the home screen, and goes when the song goes.
        assert!(blocks::overview(&conn).unwrap().lessons.iter().any(|b| b.id == id));
        songs::delete_song(&conn, 1).unwrap();
        assert!(blocks::load(&conn, id).is_err());
    }

    #[test]
    fn the_tutor_lists_and_deletes_songs() {
        let conn = scratch_db("tutor-songs");
        conn.execute_batch(
            "INSERT INTO songs (id, title, chorus_pl, chorus_en, style, sections, lyrics, suno_model, status, created_at)
             VALUES (1, 'Kawa', 'Poproszę kawę', 'A coffee, please', 'pop', '[]', '', 'V6', 'ready', 0),
                    (2, 'Kolory', 'Niebo jest niebieskie', 'The sky is blue', 'pop', '[]', '', 'V6', 'failed', 0);
             INSERT INTO song_tracks (id, song_id, ord, remote_id, audio_path, play_count)
                 VALUES (10, 1, 0, 'a', '/nonexistent/a.mp3', 3);",
        )
        .unwrap();
        let listed = tutor::run_tool_for_test(&conn, "list_songs", serde_json::json!({}));
        assert_eq!(listed.as_array().unwrap().len(), 2);
        let kawa = listed.as_array().unwrap().iter().find(|s| s["title"] == "Kawa").unwrap();
        assert_eq!(kawa["plays"], 3);

        let deleted = tutor::run_tool_for_test(&conn, "delete_songs", serde_json::json!({ "song_ids": [2, 99] }));
        assert_eq!(deleted["_action"]["kind"], "songsDeleted");
        assert_eq!(deleted["deleted"], serde_json::json!(["Kolory"]));
        assert_eq!(songs::list_songs(&conn).unwrap().len(), 1);
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
