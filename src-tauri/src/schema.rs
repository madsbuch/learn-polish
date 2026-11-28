//! The database schema as Rust types, for Diesel: every table and view the
//! app reads or writes. Queries are checked against this at compile time —
//! a misspelt column or a wrong type does not build.
//!
//! It mirrors `db::SCHEMA`; `schema_matches_the_database` (a test in db.rs)
//! fails if the two drift apart. All integers are 64-bit, as SQLite stores
//! them.

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    xp_log (id) {
        id -> XpEntryIdSql,
        day -> BigInt,
        at -> BigInt,
        run -> Text,
        lesson_id -> Nullable<LessonIdSql>,
        label -> Text,
        xp -> BigInt,
        note -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    courses (id) {
        id -> CourseIdSql,
        slug -> Text,
        ord -> BigInt,
        kind -> Text,
        title_pl -> Text,
        title_en -> Text,
        summary_en -> Text,
        created_at -> BigInt,
        image_file_id -> Nullable<FileIdSql>,
        completed_at -> Nullable<BigInt>,
        mastered_at -> Nullable<BigInt>,
        closed_at -> Nullable<BigInt>,
        level -> Nullable<Text>,
        level_checked_at -> Nullable<BigInt>,
        picture_wanted_at -> Nullable<BigInt>,
        offline_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    words (id) {
        id -> WordIdSql,
        lesson_id -> LessonIdSql,
        ord -> BigInt,
        pl -> Text,
        en -> Text,
        pos -> Text,
        image_file_id -> Nullable<FileIdSql>,
        near_pl -> Text,
        near_en -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    sentences (id) {
        id -> SentenceIdSql,
        lesson_id -> LessonIdSql,
        ord -> BigInt,
        pl -> Text,
        en -> Text,
        endings -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    conversation_lines (id) {
        id -> LineIdSql,
        lesson_id -> LessonIdSql,
        ord -> BigInt,
        speaker -> Text,
        gender -> Text,
        pl -> Text,
        en -> Text,
    }
}

diesel::table! {
    reviews (kind, item_id) {
        kind -> Text,
        item_id -> Text,
        due_day -> BigInt,
        interval -> BigInt,
        ease -> Double,
        reps -> BigInt,
        lapses -> BigInt,
        last_day -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    day_log (day, device) {
        day -> BigInt,
        device -> DeviceIdSql,
        xp -> BigInt,
        answered -> BigInt,
        correct -> BigInt,
        course_id -> Nullable<CourseIdSql>,
        completed -> BigInt,
        frozen -> BigInt,
        minutes -> BigInt,
        early_minutes -> BigInt,
    }
}

diesel::table! {
    /// A day's numbers summed over the devices that practised it (a view
    /// over day_log).
    day_totals (day) {
        day -> BigInt,
        xp -> BigInt,
        answered -> BigInt,
        correct -> BigInt,
        completed -> BigInt,
        frozen -> BigInt,
        minutes -> BigInt,
        early_minutes -> BigInt,
    }
}

diesel::table! {
    settings (key) {
        key -> Text,
        value -> Text,
    }
}

diesel::table! {
    achievements (code) {
        code -> Text,
        unlocked_day -> BigInt,
        unlocked_at -> BigInt,
    }
}

diesel::table! {
    /// Every file the app keeps (files.rs).
    use diesel::sql_types::*;
    use crate::ids::*;

    files (id) {
        id -> FileIdSql,
        kind -> Text,
        name -> Text,
        mime -> Text,
        size -> BigInt,
        sha256 -> Text,
        bytes -> Binary,
        created_at -> BigInt,
    }
}

diesel::table! {
    /// Where this device wrote a file out (local, files.rs).
    use diesel::sql_types::*;
    use crate::ids::*;

    file_paths (file_id) {
        file_id -> FileIdSql,
        path -> Text,
    }
}

diesel::table! {
    /// Settings about this device, not the learner (local, db.rs).
    device_settings (key) {
        key -> Text,
        value -> Text,
    }
}

diesel::table! {
    /// This device, for sync (local, sync/).
    use diesel::sql_types::*;
    use crate::ids::*;

    sync_state (id) {
        id -> BigInt,
        device_id -> DeviceIdSql,
        hlc_ms -> BigInt,
        hlc_counter -> BigInt,
        seq -> BigInt,
        applying -> BigInt,
        vault_id -> Nullable<Text>,
        secret_key -> Nullable<Binary>,
        public_key -> Nullable<Binary>,
    }
}

diesel::table! {
    /// Every synced row's current version (local, sync/).
    sync_rows (tbl, pk) {
        tbl -> Text,
        pk -> Text,
        hlc -> Text,
        origin -> Text,
        deleted -> BigInt,
        seq -> BigInt,
        from_peer -> Nullable<Text>,
    }
}

diesel::table! {
    /// Every synced row's version vector (local, sync/).
    sync_vv (tbl, pk, device) {
        tbl -> Text,
        pk -> Text,
        device -> Text,
        counter -> BigInt,
    }
}

diesel::table! {
    /// What this device has pulled from each peer (local, sync/).
    sync_peers (device_id) {
        device_id -> Text,
        last_pulled_seq -> BigInt,
        last_addr -> Nullable<Text>,
        last_sync_at -> Nullable<Text>,
        last_complete_at -> Nullable<Text>,
        complete -> BigInt,
        last_epoch -> Nullable<Text>,
    }
}

diesel::table! {
    /// Marks in this device's history that peers remember (local, sync/).
    sync_epochs (epoch) {
        epoch -> Text,
        seq -> BigInt,
        created_at -> Text,
    }
}

diesel::table! {
    /// The devices of the vault (synced).
    vault_devices (id) {
        id -> Text,
        name -> Text,
        public_key -> Text,
        platform -> Text,
        added_at -> Text,
    }
}

diesel::table! {
    /// Keys no longer accepted (synced).
    vault_revocations (public_key) {
        public_key -> Text,
        device_id -> Text,
        revoked_at -> Text,
    }
}

diesel::table! {
    /// Wikipedia articles a study lesson or a podcast is built on (wiki.rs).
    use diesel::sql_types::*;
    use crate::ids::*;

    wiki_sources (id) {
        id -> SourceIdSql,
        lesson_id -> Nullable<LessonIdSql>,
        song_id -> Nullable<SongIdSql>,
        ord -> BigInt,
        lang -> Text,
        title -> Text,
        url -> Text,
        pageid -> BigInt,
        revid -> BigInt,
        summary -> Text,
        content -> Text,
        image_file_id -> Nullable<FileIdSql>,
        pl_title -> Nullable<Text>,
        pl_url -> Nullable<Text>,
        pl_content -> Nullable<Text>,
        html -> Nullable<Text>,
        pl_html -> Nullable<Text>,
        fetched_at -> BigInt,
        checked_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    speech_clips (cache_key) {
        cache_key -> Text,
        model -> Text,
        voice -> Text,
        text -> Text,
        file_id -> FileIdSql,
        created_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    lessons (id) {
        id -> LessonIdSql,
        slug -> Nullable<Text>,
        lesson_type -> Text,
        ord -> BigInt,
        title -> Text,
        summary -> Text,
        complexity -> Text,
        content -> Text,
        objectives -> Text,
        period_days -> BigInt,
        interval_days -> BigInt,
        due_day -> Nullable<BigInt>,
        last_day -> Nullable<BigInt>,
        last_result -> Nullable<Text>,
        reps -> BigInt,
        origin -> Text,
        created_at -> BigInt,
        song_id -> Nullable<SongIdSql>,
        fact_note -> Nullable<Text>,
        fact_checked_at -> Nullable<BigInt>,
        ease -> Double,
        support_level -> BigInt,
        scaffold -> Nullable<Text>,
        challenge -> BigInt,
        passed -> BigInt,
        mastery -> BigInt,
        level -> Nullable<Text>,
        mastered_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    course_lessons (course_id, lesson_id) {
        /// SQLite's implicit row id: the order a lesson was listed in its
        /// courses, so its first course is the one it was made in.
        rowid -> BigInt,
        course_id -> CourseIdSql,
        lesson_id -> LessonIdSql,
        ord -> BigInt,
    }
}

diesel::table! {
    /// A course's words: whatever its lessons own (a view).
    use diesel::sql_types::*;
    use crate::ids::*;

    course_words (course_id, id) {
        course_id -> CourseIdSql,
        id -> WordIdSql,
        lesson_id -> LessonIdSql,
        ord -> BigInt,
        pl -> Text,
        en -> Text,
        pos -> Text,
    }
}

diesel::table! {
    /// A course's sentences (a view).
    use diesel::sql_types::*;
    use crate::ids::*;

    course_sentences (course_id, id) {
        course_id -> CourseIdSql,
        id -> SentenceIdSql,
        lesson_id -> LessonIdSql,
        ord -> BigInt,
        pl -> Text,
        en -> Text,
    }
}

diesel::table! {
    /// A course's dialogue lines (a view).
    use diesel::sql_types::*;
    use crate::ids::*;

    course_lines (course_id, id) {
        course_id -> CourseIdSql,
        id -> LineIdSql,
        lesson_id -> LessonIdSql,
        ord -> BigInt,
        speaker -> Text,
        gender -> Text,
        pl -> Text,
        en -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    checkins (id) {
        id -> CheckinIdSql,
        lesson_id -> LessonIdSql,
        started_at -> BigInt,
        finished_at -> Nullable<BigInt>,
        result -> Nullable<Text>,
        feedback -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    checkin_messages (id) {
        id -> CheckinMessageIdSql,
        checkin_id -> CheckinIdSql,
        role -> Text,
        content -> Text,
        created_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    tutor_conversations (id) {
        id -> ConversationIdSql,
        title -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
        job_started_at -> Nullable<BigInt>,
        job_error -> Nullable<Text>,
        tag -> Nullable<Text>,
        about -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    tutor_messages (id) {
        id -> MessageIdSql,
        role -> Text,
        content -> Text,
        actions -> Nullable<Text>,
        created_at -> BigInt,
        conversation_id -> Nullable<ConversationIdSql>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    songs (id) {
        id -> SongIdSql,
        title -> Text,
        chorus_pl -> Text,
        chorus_en -> Text,
        style -> Text,
        vocal_gender -> Text,
        sections -> Text,
        lyrics -> Text,
        suno_model -> Text,
        task_id -> Nullable<Text>,
        status -> Text,
        error -> Nullable<Text>,
        created_at -> BigInt,
        course_id -> Nullable<CourseIdSql>,
        /// JSON: how the song came about (songs::SongOrigin).
        origin -> Text,
        /// "song" (sung by Suno) or "podcast" (spoken, sentence by sentence).
        kind -> Text,
        /// JSON: how a podcast's audio is put together (podcasts::PodcastAudio).
        podcast -> Text,
    }
}

diesel::table! {
    /// The radio's spoken bridges (radio.rs).
    use diesel::sql_types::*;
    use crate::ids::*;

    bridges (id) {
        id -> BridgeIdSql,
        song_id -> Nullable<SongIdSql>,
        role -> Text,
        /// JSON: the lines said, [{pl, en}].
        lines -> Text,
        format -> Text,
        voice -> Text,
        audio_file_id -> FileIdSql,
        duration -> Double,
        /// JSON: the lines with when each is said.
        timed_lines -> Text,
        play_count -> BigInt,
        last_played_at -> Nullable<BigInt>,
        created_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    song_tracks (id) {
        id -> TrackIdSql,
        song_id -> SongIdSql,
        ord -> BigInt,
        remote_id -> Text,
        duration -> Double,
        play_count -> BigInt,
        last_played_at -> Nullable<BigInt>,
        aligned_words -> Nullable<Text>,
        audio_file_id -> Nullable<FileIdSql>,
        image_file_id -> Nullable<FileIdSql>,
        /// JSON: lines with exact times, for audio the app put together itself.
        timed_lines -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    playlists (id) {
        id -> PlaylistIdSql,
        name -> Text,
        created_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    playlist_tracks (id) {
        id -> EntryIdSql,
        playlist_id -> PlaylistIdSql,
        track_id -> TrackIdSql,
        ord -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    ai_costs (id) {
        id -> CostIdSql,
        day -> BigInt,
        feature -> Text,
        provider -> Text,
        model -> Text,
        cost_usd -> Nullable<Double>,
        detail -> Text,
        #[sql_name = "ref"]
        reference -> Nullable<Text>,
        created_at -> BigInt,
        tokens_in -> BigInt,
        tokens_cached -> BigInt,
        tokens_out -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    memories (id) {
        id -> MemoryIdSql,
        content -> Text,
        category -> Text,
        source -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    word_examples (word_id) {
        word_id -> WordIdSql,
        examples -> Text,
        created_at -> BigInt,
    }
}

diesel::joinable!(word_examples -> words (word_id));
diesel::joinable!(attempt_items -> attempts (attempt_id));
diesel::joinable!(words -> lessons (lesson_id));
diesel::joinable!(sentences -> lessons (lesson_id));
diesel::joinable!(conversation_lines -> lessons (lesson_id));
diesel::joinable!(course_lessons -> lessons (lesson_id));
diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    bench_runs (id) {
        id -> BenchRunIdSql,
        model -> Text,
        created_at -> BigInt,
        score -> BigInt,
        polish -> BigInt,
        tools -> BigInt,
        rules -> BigInt,
        seconds -> Double,
        cost_usd -> Nullable<Double>,
        items -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    scenes (lesson_id) {
        lesson_id -> LessonIdSql,
        description -> Text,
        statements -> Text,
        prompt -> Text,
        image_file_id -> Nullable<FileIdSql>,
        width -> BigInt,
        height -> BigInt,
        status -> Text,
        error -> Text,
        image_model -> Text,
        vision_model -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    scene_spots (id) {
        id -> SpotIdSql,
        lesson_id -> LessonIdSql,
        word_id -> WordIdSql,
        x -> Double,
        y -> Double,
        w -> Double,
        h -> Double,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    stories (lesson_id) {
        lesson_id -> LessonIdSql,
        theme -> Text,
        setting -> Text,
        characters -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    story_panels (id) {
        id -> PanelIdSql,
        lesson_id -> LessonIdSql,
        ord -> BigInt,
        picture -> Text,
        lines -> Text,
        question -> Text,
        prompt -> Text,
        image_file_id -> Nullable<FileIdSql>,
        width -> BigInt,
        height -> BigInt,
        status -> Text,
        error -> Text,
        image_model -> Text,
        updated_at -> BigInt,
    }
}

diesel::joinable!(course_lessons -> courses (course_id));
diesel::joinable!(scene_spots -> words (word_id));
diesel::joinable!(course_words -> courses (course_id));
diesel::joinable!(course_words -> lessons (lesson_id));
diesel::joinable!(course_sentences -> lessons (lesson_id));
diesel::joinable!(course_sentences -> courses (course_id));
diesel::joinable!(course_lines -> courses (course_id));
diesel::joinable!(checkins -> lessons (lesson_id));
diesel::joinable!(checkin_messages -> checkins (checkin_id));
diesel::joinable!(tutor_messages -> tutor_conversations (conversation_id));
diesel::joinable!(song_tracks -> songs (song_id));
diesel::joinable!(speech_clips -> files (file_id));
diesel::joinable!(wiki_sources -> lessons (lesson_id));
diesel::joinable!(lessons -> songs (song_id));
diesel::joinable!(songs -> courses (course_id));
diesel::joinable!(playlist_tracks -> playlists (playlist_id));
diesel::joinable!(playlist_tracks -> song_tracks (track_id));

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    attempts (id) {
        id -> AttemptIdSql,
        at -> BigInt,
        day -> BigInt,
        device -> DeviceIdSql,
        item_kind -> Text,
        item_id -> Text,
        lesson_id -> Nullable<LessonIdSql>,
        exercise -> Text,
        run -> Text,
        correct -> BigInt,
        given -> Text,
        expected -> Text,
        mistake -> Text,
        detail -> Text,
        contrast -> Text,
        ms_first -> Nullable<BigInt>,
        ms_answer -> Nullable<BigInt>,
        tries -> BigInt,
        hints -> BigInt,
        plays -> BigInt,
        slow_plays -> BigInt,
        reveals -> BigInt,
        changed -> BigInt,
        details -> Text,
        course_id -> Nullable<CourseIdSql>,
        source -> Text,
        card_key -> Text,
        direction -> Text,
        prompt -> Text,
        position -> Nullable<BigInt>,
        checkin_id -> Nullable<CheckinIdSql>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    learning_events (id) {
        id -> LearningEventIdSql,
        at -> BigInt,
        day -> BigInt,
        device -> DeviceIdSql,
        kind -> Text,
        item_kind -> Text,
        item_id -> Text,
        lesson_id -> Nullable<LessonIdSql>,
        run -> Text,
        details -> Text,
        course_id -> Nullable<CourseIdSql>,
        source -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    mastery_log (id) {
        id -> MasteryEntryIdSql,
        at -> BigInt,
        day -> BigInt,
        device -> DeviceIdSql,
        lesson_id -> LessonIdSql,
        delta -> BigInt,
        points_before -> BigInt,
        points_after -> BigInt,
        grade -> Text,
        correct -> Nullable<BigInt>,
        almost -> Nullable<BigInt>,
        total -> Nullable<BigInt>,
        days_since -> Nullable<BigInt>,
        extra -> BigInt,
        via -> Text,
        run -> Text,
        reason -> Text,
        details -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    attempt_items (attempt_id, item_id, role, position) {
        attempt_id -> AttemptIdSql,
        item_id -> Text,
        item_kind -> Text,
        role -> Text,
        form -> Text,
        position -> BigInt,
        correct -> Nullable<BigInt>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    sentence_tokens (sentence_id, position) {
        sentence_id -> SentenceIdSql,
        position -> BigInt,
        form -> Text,
        lemma -> Text,
        word_id -> Nullable<WordIdSql>,
        pos -> Text,
        gram_case -> Text,
        number -> Text,
        gender -> Text,
        person -> Text,
        tense -> Text,
        aspect -> Text,
        mood -> Text,
        reason -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    word_topics (word_id, topic) {
        word_id -> WordIdSql,
        topic -> TopicIdSql,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    sentence_topics (sentence_id, topic) {
        sentence_id -> SentenceIdSql,
        topic -> TopicIdSql,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    lesson_topics (lesson_id, topic) {
        lesson_id -> LessonIdSql,
        topic -> TopicIdSql,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    word_grammar (word_id, tag_id) {
        word_id -> WordIdSql,
        tag_id -> GrammarTagIdSql,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::ids::*;

    sentence_grammar (sentence_id, tag_id) {
        sentence_id -> SentenceIdSql,
        tag_id -> GrammarTagIdSql,
    }
}

diesel::joinable!(word_grammar -> words (word_id));
diesel::joinable!(sentence_grammar -> sentences (sentence_id));

diesel::allow_tables_to_appear_in_same_query!(
    bench_runs,
    xp_log,
    day_totals,
    file_paths,
    device_settings,
    sync_state,
    sync_rows,
    sync_vv,
    sync_peers,
    sync_epochs,
    vault_devices,
    vault_revocations,
    courses,
    words,
    sentences,
    conversation_lines,
    reviews,
    day_log,
    settings,
    achievements,
    files,
    speech_clips,
    wiki_sources,
    lessons,
    course_lessons,
    course_words,
    course_sentences,
    course_lines,
    checkins,
    checkin_messages,
    tutor_conversations,
    tutor_messages,
    songs,
    song_tracks,
    bridges,
    playlists,
    playlist_tracks,
    ai_costs,
    memories,
    word_examples,
    scenes,
    scene_spots,
    stories,
    story_panels,
    attempts,
    learning_events,
    mastery_log,
    word_grammar,
    sentence_grammar,
    attempt_items,
    sentence_tokens,
    word_topics,
    sentence_topics,
    lesson_topics,
);
