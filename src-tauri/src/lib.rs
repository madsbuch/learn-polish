mod agent;
mod ai;
mod blocks;
mod cast;
mod commands;
mod content;
mod costs;
mod courses;
mod db;
mod error;
mod examiner;
mod http;
mod memories;
mod practice;
mod models;
mod playlists;
mod session;
mod songs;
mod speech;
mod srs;
mod stats;
mod text;
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
            practice::submit_answer,
            blocks::commands::blocks_overview,
            blocks::commands::blocks_for_topic,
            blocks::commands::block_detail,
            practice::block_session,
            practice::block_finish,
            blocks::commands::block_set_period,
            blocks::commands::block_delete,
            blocks::commands::block_for_song,
            practice::checkin_summary,
            examiner::checkin_start,
            examiner::checkin_send,            commands::get_stats,
            commands::get_topic_progress,
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
            courses::commands::course_delete,
            courses::commands::block_add_to_course,
            courses::commands::block_remove_from_course,
            courses::commands::course_rename,
            commands::export_database,
            costs::commands::costs_summary,
            memories::commands::memories_overview,
            memories::commands::memories_set_enabled,
            memories::commands::memory_add,
            memories::commands::memory_update,
            memories::commands::memory_delete,
            memories::commands::memories_clear,
            speech::list_voices,
            speech::preview_voice,
            speech::speak,
            speech::cache_audio,
            speech::topic_audio_lines,
            speech::audio_cache_stats,
            speech::clear_audio_cache,
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

    fn grammar_course(conn: &Connection) -> i64 {
        conn.query_row("SELECT id FROM topics WHERE slug = 'grammar-essentials'", [], |r| r.get(0))
            .expect("the starter lessons have a course")
    }

    fn starter_lesson(conn: &Connection) -> blocks::Block {
        blocks::for_topic(conn, grammar_course(conn)).unwrap().remove(0)
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
        // Ten topics and the course holding the grammar lessons.
        assert_eq!(count("SELECT COUNT(*) FROM topics"), 11);
        assert!(count("SELECT COUNT(*) FROM words") >= 2000);
        assert_eq!(count("SELECT COUNT(*) FROM sentences"), 200);
        assert!(count("SELECT COUNT(*) FROM conversation_lines") >= 100);

        // Opening again must not duplicate anything — and a starter course the
        // learner deleted stays deleted: it is theirs now, never seeded again.
        let basics = db::require_topic_id(&conn, 1).unwrap();
        crate::courses::delete(&conn, basics).unwrap();
        drop(conn);
        let conn = scratch_db_keep("seed");
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

        // The starter lessons are there, as study blocks in their own course.
        let lessons = blocks::for_topic(&conn, grammar_course(&conn)).unwrap();
        assert!(lessons.len() >= 7);
        assert!(lessons.iter().all(|b| b.kind == "study" && b.origin == "starter"));
        let overview = blocks::overview(&conn).unwrap();
        assert!(overview.due.is_empty(), "nothing is due on a fresh install");
        assert_eq!(overview.next[0].topic_id, Some(basics), "the course starts at the beginning");
    }

    #[test]
    fn a_version_3_database_moves_words_into_blocks() {
        // The shape before words belonged to blocks: words, sentences and
        // lines in topics, drills pointing at them, one topic per block.
        let path = std::env::temp_dir()
            .join("learn-polish-tests")
            .join(format!("v3-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        {
            let old = Connection::open(&path).unwrap();
            old.execute_batch(
                "CREATE TABLE topics (id INTEGER PRIMARY KEY, slug TEXT NOT NULL UNIQUE, ord INTEGER NOT NULL,
                     kind TEXT NOT NULL DEFAULT 'course', title_pl TEXT NOT NULL, title_en TEXT NOT NULL,
                     summary_en TEXT NOT NULL, created_at INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE words (id INTEGER PRIMARY KEY, topic_id INTEGER NOT NULL, ord INTEGER NOT NULL,
                     pl TEXT NOT NULL, en TEXT NOT NULL, pos TEXT NOT NULL, UNIQUE (topic_id, pl));
                 CREATE TABLE sentences (id INTEGER PRIMARY KEY, topic_id INTEGER NOT NULL, ord INTEGER NOT NULL,
                     pl TEXT NOT NULL, en TEXT NOT NULL, UNIQUE (topic_id, pl));
                 CREATE TABLE conversation_lines (id INTEGER PRIMARY KEY, topic_id INTEGER NOT NULL,
                     ord INTEGER NOT NULL, speaker TEXT NOT NULL, gender TEXT NOT NULL DEFAULT 'f',
                     pl TEXT NOT NULL, en TEXT NOT NULL);
                 CREATE TABLE reviews (kind TEXT NOT NULL, item_id INTEGER NOT NULL, due_day INTEGER NOT NULL,
                     interval INTEGER NOT NULL, ease REAL NOT NULL, reps INTEGER NOT NULL, lapses INTEGER NOT NULL,
                     last_day INTEGER NOT NULL, PRIMARY KEY (kind, item_id));
                 CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE blocks (id INTEGER PRIMARY KEY, slug TEXT UNIQUE, kind TEXT NOT NULL,
                     format TEXT NOT NULL DEFAULT '', topic_id INTEGER, ord INTEGER NOT NULL DEFAULT 0,
                     title TEXT NOT NULL, summary TEXT NOT NULL DEFAULT '', complexity TEXT NOT NULL,
                     lesson TEXT NOT NULL DEFAULT '', objectives TEXT NOT NULL DEFAULT '[]',
                     period_days INTEGER NOT NULL, interval_days INTEGER NOT NULL DEFAULT 0, due_day INTEGER,
                     last_day INTEGER, last_result TEXT, reps INTEGER NOT NULL DEFAULT 0,
                     origin TEXT NOT NULL DEFAULT 'course', created_at INTEGER NOT NULL, song_id INTEGER);
                 CREATE TABLE block_items (block_id INTEGER NOT NULL, kind TEXT NOT NULL, item_id INTEGER NOT NULL,
                     ord INTEGER NOT NULL, PRIMARY KEY (block_id, kind, item_id));
                 CREATE TABLE checkins (id INTEGER PRIMARY KEY, block_id INTEGER NOT NULL, started_at INTEGER NOT NULL,
                     finished_at INTEGER, result TEXT, feedback TEXT);
                 CREATE TABLE checkin_messages (id INTEGER PRIMARY KEY, checkin_id INTEGER NOT NULL,
                     role TEXT NOT NULL, content TEXT NOT NULL, created_at INTEGER NOT NULL);

                 INSERT INTO topics VALUES (1, 'basics', 1, 'course', 'Pierwsze spotkanie', 'Meeting', '', 0),
                                           (2, 'programme-1-1', 1, 'programme', 'Kolory', 'Colours', '', 50);
                 INSERT INTO words VALUES (10, 1, 0, 'cześć', 'hi', 'interj'),
                                          (20, 2, 0, 'czerwony', 'red', 'adj'),
                                          (21, 2, 1, 'biały', 'white', 'adj'),
                                          (22, 2, 2, 'czarny', 'black', 'adj');
                 INSERT INTO sentences VALUES (30, 2, 0, 'Niebo jest niebieskie.', 'The sky is blue.');
                 INSERT INTO conversation_lines VALUES (40, 1, 0, 'Anna', 'f', 'Dzień dobry!', 'Good morning!');
                 INSERT INTO reviews VALUES ('word', 10, 100, 3, 2.5, 2, 0, 97), ('word', 20, 100, 3, 2.5, 4, 0, 97);
                 -- A starter topic, cut into a dialogue and a word drill.
                 INSERT INTO blocks (id, slug, kind, format, topic_id, ord, title, complexity, period_days, created_at, due_day, reps)
                     VALUES (100, 'topic/1/dialogue', 'drill', 'dialogue', 1, 1, 'Conversation', 'low', 2, 0, NULL, 0),
                            (101, 'topic/1/words-1', 'drill', 'words', 1, 2, 'Words 1', 'low', 2, 0, 105, 3);
                 INSERT INTO block_items VALUES (101, 'word', 10, 0);
                 -- A programme: the tutor's memory drill over two words, which the
                 -- programme's own word drill also picked up (the duplicate).
                 INSERT INTO blocks (id, slug, kind, format, topic_id, ord, title, complexity, period_days, origin, created_at)
                     VALUES (200, NULL, 'drill', 'memory', 2, 1, 'Colours — memory', 'low', 2, 'tutor', 60),
                            (201, 'topic/2/words-1', 'drill', 'words', 2, 2, 'Words 1', 'low', 2, 'tutor', 50),
                            (202, 'topic/2/sentences-1', 'drill', 'sentences', 2, 3, 'Sentences 1', 'medium', 4, 'tutor', 50),
                            (203, 'topic/2/words-2', 'drill', 'words', 2, 4, 'Words 2', 'low', 2, 'tutor', 50);
                 INSERT INTO block_items VALUES (200, 'word', 20, 0), (200, 'word', 21, 1),
                                                (201, 'word', 20, 0), (201, 'word', 21, 1), (201, 'word', 22, 2),
                                                (202, 'sentence', 30, 0),
                                                (203, 'word', 21, 0);
                 -- A lesson with no topic, with a finished check-in.
                 INSERT INTO blocks (id, slug, kind, format, topic_id, title, complexity, lesson, period_days, origin, created_at)
                     VALUES (300, NULL, 'study', '', NULL, 'Iść', 'low', 'idę', 5, 'tutor', 70);
                 INSERT INTO checkins VALUES (1, 300, 0, 1, 'pass', 'Good.');
                 INSERT INTO settings VALUES ('content_version', '4');
                 PRAGMA user_version = 3;",
            )
            .unwrap();
        }

        let conn = db::open(&path).expect("upgrade");
        let owner = |table: &str, id: i64| -> i64 {
            conn.query_row(&format!("SELECT block_id FROM {table} WHERE id = ?1"), [id], |r| r.get(0)).unwrap()
        };
        // Ids — and so review history — survive.
        assert_eq!(owner("words", 10), 101);
        assert_eq!(srs::load(&conn, "word", 10).unwrap().unwrap().reps, 2);
        assert_eq!(srs::load(&conn, "word", 20).unwrap().unwrap().reps, 4);
        // A shared word goes to the tutor's drill, not the automatic one.
        assert_eq!(owner("words", 20), 200);
        assert_eq!(owner("words", 21), 200);
        assert_eq!(owner("words", 22), 201);
        assert_eq!(owner("sentences", 30), 202);
        assert_eq!(owner("conversation_lines", 40), 100);
        // An automatic drill left with nothing goes.
        assert!(blocks::load(&conn, 203).is_err());
        assert_eq!(blocks::load(&conn, 101).unwrap().due_day, Some(105), "schedules survive");

        // Every block is listed in a course; programmes are courses now.
        let unlisted: i64 = conn
            .query_row("SELECT COUNT(*) FROM blocks WHERE id NOT IN (SELECT block_id FROM course_blocks)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(unlisted, 0);
        assert_eq!(blocks::load(&conn, 300).unwrap().topic_title.as_deref(), Some("Lessons"));
        assert_eq!(blocks::detail(&conn, 300).unwrap().last_feedback.as_deref(), Some("Good."));
        let kinds: i64 = conn.query_row("SELECT COUNT(*) FROM topics WHERE kind != 'course'", [], |r| r.get(0)).unwrap();
        assert_eq!(kinds, 0);

        // The starter content is filled in around what was there, without
        // repeating a word the course already teaches.
        let basics: i64 = conn.query_row("SELECT id FROM topics WHERE slug = 'basics'", [], |r| r.get(0)).unwrap();
        let czesc: i64 = conn
            .query_row("SELECT COUNT(*) FROM course_words WHERE topic_id = ?1 AND pl = 'cześć'", [basics], |r| r.get(0))
            .unwrap();
        assert_eq!(czesc, 1);
        let dialogues = blocks::for_topic(&conn, basics).unwrap().iter().filter(|b| b.format == "dialogue").count();
        assert_eq!(dialogues, 1, "the existing conversation is kept, not doubled");
        assert!(blocks::for_topic(&conn, grammar_course(&conn)).unwrap().len() >= 7);

        // A word taken out goes with its history.
        crate::courses::remove_items(&conn, 2, &["czerwony".into()], &[]).unwrap();
        assert!(srs::load(&conn, "word", 20).unwrap().is_none());
        drop(conn);
        db::open(&path).expect("a second open is a no-op");
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
        let lesson = starter_lesson(&conn);
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
            topic_id: None,
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
    fn the_tutor_builds_a_course_that_can_be_practised() {
        let conn = scratch_db("course");

        // "Can you build me a programme for learning the colours?" — a course.
        let result = tutor::run_tool_for_test(
            &conn,
            "create_course",
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
        assert_eq!(result["_action"]["kind"], "courseCreated");
        let topic_id = created["topic_id"].as_i64().unwrap();

        // It is cut into drills, and they are offered first.
        let list = blocks::for_topic(&conn, topic_id).unwrap();
        let formats: Vec<&str> = list.iter().map(|b| b.format.as_str()).collect();
        assert_eq!(formats, vec!["dialogue", "words", "sentences"]);
        assert_eq!(blocks::overview(&conn).unwrap().next[0].topic_id, Some(topic_id));
        let plan = session::build_block(&conn, list[1].id).unwrap();
        assert_eq!(plan.topic.kind, "course");
        assert_eq!(plan.new_words, 4);

        // Adding words makes a new drill; the existing ones keep theirs, and a
        // word the course already teaches is not added twice.
        let added = tutor::run_tool_for_test(
            &conn,
            "add_to_course",
            serde_json::json!({ "course_id": topic_id, "words": [
                { "pl": "żółty", "en": "yellow", "pos": "adj" },
                { "pl": "czerwony", "en": "red", "pos": "adj" }
            ] }),
        );
        assert_eq!(added["updated"]["skipped_duplicates"], 1, "{added}");
        assert_eq!(blocks::load(&conn, list[1].id).unwrap().items, 4);
        let words_2 = blocks::for_topic(&conn, topic_id).unwrap().into_iter().last().unwrap();
        assert_eq!((words_2.title.as_str(), words_2.items), ("Words 2", 1));

        // Starter courses are ordinary courses: they can be extended too.
        let course = db::require_topic_id(&conn, 1).unwrap();
        let before = blocks::for_topic(&conn, course).unwrap().len();
        let grown = tutor::run_tool_for_test(
            &conn,
            "add_to_course",
            serde_json::json!({ "course_id": course, "words": [{ "pl": "sąsiedztwo", "en": "neighbourhood", "pos": "n-n" }] }),
        );
        assert!(grown.get("error").is_none(), "{grown}");
        assert!(blocks::for_topic(&conn, course).unwrap().len() >= before);

        // Any course can be deleted with its blocks.
        let deleted = tutor::run_tool_for_test(&conn, "delete_course", serde_json::json!({ "course_id": topic_id }));
        assert_eq!(deleted["deleted"], "Colours");
        assert!(blocks::for_topic(&conn, topic_id).unwrap().is_empty());
    }

    #[test]
    fn blocks_move_and_are_shared_between_courses() {
        let conn = scratch_db("membership");
        let basics = db::require_topic_id(&conn, 1).unwrap();
        let family = db::require_topic_id(&conn, 2).unwrap();
        let block = blocks::for_topic(&conn, basics).unwrap().into_iter().find(|b| b.format == "words").unwrap();
        let words = blocks::detail(&conn, block.id).unwrap().words;

        // Shared: one block, listed in both, with one schedule.
        let shared = tutor::run_tool_for_test(
            &conn,
            "add_block_to_course",
            serde_json::json!({ "block_id": block.id, "course_id": family }),
        );
        assert!(shared.get("error").is_none(), "{shared}");
        assert!(blocks::for_topic(&conn, family).unwrap().iter().any(|b| b.id == block.id));
        assert_eq!(blocks::load(&conn, block.id).unwrap().course_ids, vec![basics, family]);
        blocks::record(&conn, block.id, "pass").unwrap();
        assert!(blocks::for_topic(&conn, family).unwrap().iter().any(|b| b.id == block.id && b.reps == 1));
        // The family course now reaches the block's words too.
        let reach: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM course_words WHERE topic_id = ?1 AND block_id = ?2",
                [family, block.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(reach as usize, words.len());

        // Out of one course, it stays; out of the last, it goes with its words.
        let out = tutor::run_tool_for_test(
            &conn,
            "remove_block_from_course",
            serde_json::json!({ "block_id": block.id, "course_id": basics }),
        );
        assert_eq!(out["block_deleted"], false);
        assert_eq!(blocks::load(&conn, block.id).unwrap().topic_id, Some(family));
        let word_id: i64 = conn
            .query_row("SELECT id FROM words WHERE block_id = ?1 LIMIT 1", [block.id], |r| r.get(0))
            .unwrap();
        let gone = tutor::run_tool_for_test(
            &conn,
            "remove_block_from_course",
            serde_json::json!({ "block_id": block.id, "course_id": family }),
        );
        assert_eq!(gone["block_deleted"], true);
        assert!(blocks::load(&conn, block.id).is_err());
        let left: i64 = conn.query_row("SELECT COUNT(*) FROM words WHERE id = ?1", [word_id], |r| r.get(0)).unwrap();
        assert_eq!(left, 0, "its words went with it");

        // Moving takes it out of one course and into another, at the end.
        let lesson = starter_lesson(&conn);
        let grammar = grammar_course(&conn);
        tutor::run_tool_for_test(
            &conn,
            "move_block",
            serde_json::json!({ "block_id": lesson.id, "from_course_id": grammar, "to_course_id": basics }),
        );
        assert_eq!(blocks::load(&conn, lesson.id).unwrap().course_ids, vec![basics]);
        assert_eq!(blocks::for_topic(&conn, basics).unwrap().last().unwrap().id, lesson.id);

        // Reordering puts it first.
        tutor::run_tool_for_test(
            &conn,
            "order_course_blocks",
            serde_json::json!({ "course_id": basics, "block_ids": [lesson.id] }),
        );
        assert_eq!(blocks::for_topic(&conn, basics).unwrap()[0].id, lesson.id);

        // Deleting a course keeps blocks another course still lists.
        let kept = blocks::for_topic(&conn, family).unwrap()[0].id;
        crate::courses::add_block(&conn, basics, kept).unwrap();
        crate::courses::delete(&conn, family).unwrap();
        assert!(blocks::load(&conn, kept).is_ok());
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
        // Without a course it is filed in "Lessons".
        assert_eq!(block.topic_title.as_deref(), Some("Lessons"));

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

        // Starter blocks are ordinary blocks: they can be edited and deleted.
        let starter = starter_lesson(&conn).id;
        let edit = tutor::run_tool_for_test(&conn, "update_block", serde_json::json!({ "block_id": starter, "title": "Mine now" }));
        assert!(edit.get("error").is_none(), "{edit}");
        assert_eq!(blocks::load(&conn, starter).unwrap().title, "Mine now");
        let gone = tutor::run_tool_for_test(&conn, "delete_block", serde_json::json!({ "block_id": starter }));
        assert_eq!(gone["_action"]["kind"], "blockDeleted");

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
    fn the_tutor_manages_playlists_by_song() {
        let conn = scratch_db("tutor-playlists");
        conn.execute_batch(
            "INSERT INTO songs (id, title, chorus_pl, chorus_en, style, sections, lyrics, suno_model, status, created_at)
             VALUES (1, 'Kawa', 'Poproszę kawę', 'A coffee, please', 'pop', '[]', '', 'V6', 'ready', 0),
                    (2, 'Kolory', 'Niebo', 'Sky', 'pop', '[]', '', 'V6', 'ready', 0),
                    (3, 'Na dworcu', 'x', 'x', 'pop', '[]', '', 'V6', 'generating', 0);
             INSERT INTO song_tracks (id, song_id, ord, remote_id, audio_path, play_count) VALUES
                 (10, 1, 0, 'a', '/tmp/a.mp3', 1), (11, 1, 1, 'b', '/tmp/b.mp3', 5),
                 (20, 2, 0, 'c', '/tmp/c.mp3', 0);",
        )
        .unwrap();

        let made = tutor::run_tool_for_test(
            &conn,
            "create_playlist",
            serde_json::json!({ "name": "Morning", "song_ids": [1] }),
        );
        assert_eq!(made["_action"]["kind"], "playlistChanged", "{made}");
        let id = made["playlist_id"].as_i64().unwrap();
        // The most-played take goes in.
        assert_eq!(made["songs"][0]["take"], 2);
        assert_eq!(playlists::list(&conn).unwrap()[0].entries[0].track_id, 11);

        // "Add Kolory to my morning playlist" — and Kawa again is skipped.
        let added = tutor::run_tool_for_test(
            &conn,
            "add_to_playlist",
            serde_json::json!({ "playlist_id": id, "song_ids": [2, 1] }),
        );
        assert_eq!(added["songs"].as_array().unwrap().len(), 2, "{added}");
        assert_eq!(added["already_in_it"], serde_json::json!(["Kawa"]));

        // A song still being made cannot go in, and nothing changes.
        let refused = tutor::run_tool_for_test(
            &conn,
            "add_to_playlist",
            serde_json::json!({ "playlist_id": id, "song_ids": [3] }),
        );
        assert!(refused["error"].as_str().unwrap().contains("no audio yet"));

        let listed = tutor::run_tool_for_test(&conn, "list_playlists", serde_json::json!({}));
        assert_eq!(listed[0]["songs"][1]["title"], "Kolory");

        tutor::run_tool_for_test(&conn, "remove_from_playlist", serde_json::json!({ "playlist_id": id, "song_ids": [1] }));
        tutor::run_tool_for_test(&conn, "rename_playlist", serde_json::json!({ "playlist_id": id, "name": "Rano" }));
        let list = playlists::list(&conn).unwrap();
        assert_eq!((list[0].name.as_str(), list[0].entries.len()), ("Rano", 1));

        let gone = tutor::run_tool_for_test(&conn, "delete_playlist", serde_json::json!({ "playlist_id": id }));
        assert_eq!(gone["deleted"], "Rano");
        assert!(playlists::list(&conn).unwrap().is_empty());
        assert_eq!(songs::list_songs(&conn).unwrap().len(), 3, "the songs stay");
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
        assert_eq!(blocks::create_listening(&conn, 1, None, None, None, "tutor").unwrap().id, id);
        // A song without a course files it in "Listening"; it goes when the song goes.
        assert_eq!(block.topic_title.as_deref(), Some("Listening"));

        // A song written from a course's words files it in that course.
        let food = db::require_topic_id(&conn, 3).unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO songs (id, title, chorus_pl, chorus_en, style, sections, lyrics, suno_model, status, created_at, topic_id)
             VALUES (2, 'Obiad', 'Smacznego', 'Enjoy', 'pop', '[]', '', 'V6', 'ready', 0, {food});"
        ))
        .unwrap();
        let made = tutor::run_tool_for_test(&conn, "create_listening_block", serde_json::json!({ "song_id": 2 }));
        let filed = blocks::load(&conn, made["_action"]["blockId"].as_i64().unwrap()).unwrap();
        assert_eq!(filed.course_ids, vec![food]);
        // Or in the course asked for.
        conn.execute_batch(
            "INSERT INTO songs (id, title, chorus_pl, chorus_en, style, sections, lyrics, suno_model, status, created_at)
             VALUES (3, 'Dom', 'Dom', 'Home', 'pop', '[]', '', 'V6', 'ready', 0);",
        )
        .unwrap();
        let basics = db::require_topic_id(&conn, 1).unwrap();
        let made = tutor::run_tool_for_test(
            &conn,
            "create_listening_block",
            serde_json::json!({ "song_id": 3, "course_id": basics }),
        );
        assert_eq!(blocks::load(&conn, made["_action"]["blockId"].as_i64().unwrap()).unwrap().course_ids, vec![basics]);

        // The tutor reads a song's lyrics and files a course-less one itself.
        conn.execute_batch(
            r#"INSERT INTO songs (id, title, chorus_pl, chorus_en, style, sections, lyrics, suno_model, status, created_at)
               VALUES (4, 'Pociąg', 'Gdzie jest dworzec?', 'Where is the station?', 'pop',
                       '[{"kind":"chorus","lines":[{"en":"Where is the station?","pl":"Gdzie jest dworzec?"}]}]',
                       '', 'V6', 'ready', 0);"#,
        )
        .unwrap();
        let block = blocks::create_listening(&conn, 4, None, None, None, "you").unwrap();
        assert_eq!(block.topic_title.as_deref(), Some("Listening"));
        let song = tutor::run_tool_for_test(&conn, "get_song", serde_json::json!({ "song_id": 4 }));
        assert_eq!(song["lyrics"][0]["lines"][0]["pl"], "Gdzie jest dworzec?");
        assert_eq!(song["filed"]["listening_block_in"][0]["title"], "Listening");
        assert!(song["filed"]["written_from_course"].is_null());
        let travel = db::require_topic_id(&conn, 5).unwrap();
        let listening = block.topic_id.unwrap();
        tutor::run_tool_for_test(
            &conn,
            "move_block",
            serde_json::json!({ "block_id": block.id, "from_course_id": listening, "to_course_id": travel }),
        );
        let song = tutor::run_tool_for_test(&conn, "get_song", serde_json::json!({ "song_id": 4 }));
        assert_eq!(song["filed"]["written_from_course"]["course_id"], travel, "the song now knows its course");
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
        let topics = tutor::run_tool_for_test(&conn, "list_courses", serde_json::json!({}));
        assert_eq!(topics.as_array().unwrap().len(), 11);
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
        assert_eq!(topics, 11);
        let basics_words: i64 = conn
            .query_row("SELECT COUNT(*) FROM words WHERE pl = 'cześć'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(basics_words, 1);

        // Opening again is a no-op.
        drop(conn);
        db::open(&path).expect("second open");
    }
}
