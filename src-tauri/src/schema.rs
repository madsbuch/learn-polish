//! The database schema as Rust types, for Diesel: every table and view the
//! app reads or writes. Queries are checked against this at compile time —
//! a misspelt column or a wrong type does not build.
//!
//! It mirrors `db::SCHEMA`; `schema_matches_the_database` (a test in db.rs)
//! fails if the two drift apart. All integers are 64-bit, as SQLite stores
//! them.

diesel::table! {
    topics (id) {
        id -> BigInt,
        slug -> Text,
        ord -> BigInt,
        kind -> Text,
        title_pl -> Text,
        title_en -> Text,
        summary_en -> Text,
        created_at -> BigInt,
    }
}

diesel::table! {
    words (id) {
        id -> BigInt,
        block_id -> BigInt,
        ord -> BigInt,
        pl -> Text,
        en -> Text,
        pos -> Text,
    }
}

diesel::table! {
    sentences (id) {
        id -> BigInt,
        block_id -> BigInt,
        ord -> BigInt,
        pl -> Text,
        en -> Text,
    }
}

diesel::table! {
    conversation_lines (id) {
        id -> BigInt,
        block_id -> BigInt,
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
        item_id -> BigInt,
        due_day -> BigInt,
        interval -> BigInt,
        ease -> Double,
        reps -> BigInt,
        lapses -> BigInt,
        last_day -> BigInt,
    }
}

diesel::table! {
    day_log (day) {
        day -> BigInt,
        xp -> BigInt,
        answered -> BigInt,
        correct -> BigInt,
        topic_id -> Nullable<BigInt>,
        completed -> BigInt,
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
    files (id) {
        id -> BigInt,
        kind -> Text,
        name -> Text,
        mime -> Text,
        size -> BigInt,
        sha256 -> Text,
        bytes -> Binary,
        path -> Nullable<Text>,
        created_at -> BigInt,
    }
}

diesel::table! {
    speech_clips (cache_key) {
        cache_key -> Text,
        model -> Text,
        voice -> Text,
        text -> Text,
        file_id -> BigInt,
        created_at -> BigInt,
    }
}

diesel::table! {
    blocks (id) {
        id -> BigInt,
        slug -> Nullable<Text>,
        kind -> Text,
        format -> Text,
        ord -> BigInt,
        title -> Text,
        summary -> Text,
        complexity -> Text,
        lesson -> Text,
        objectives -> Text,
        period_days -> BigInt,
        interval_days -> BigInt,
        due_day -> Nullable<BigInt>,
        last_day -> Nullable<BigInt>,
        last_result -> Nullable<Text>,
        reps -> BigInt,
        origin -> Text,
        created_at -> BigInt,
        song_id -> Nullable<BigInt>,
    }
}

diesel::table! {
    course_blocks (topic_id, block_id) {
        /// SQLite's implicit row id: the order a block was listed in its
        /// courses, so its first course is the one it was made in.
        rowid -> BigInt,
        topic_id -> BigInt,
        block_id -> BigInt,
        ord -> BigInt,
    }
}

diesel::table! {
    /// A course's words: whatever its blocks own (a view).
    course_words (topic_id, id) {
        topic_id -> BigInt,
        id -> BigInt,
        block_id -> BigInt,
        ord -> BigInt,
        pl -> Text,
        en -> Text,
        pos -> Text,
    }
}

diesel::table! {
    /// A course's sentences (a view).
    course_sentences (topic_id, id) {
        topic_id -> BigInt,
        id -> BigInt,
        block_id -> BigInt,
        ord -> BigInt,
        pl -> Text,
        en -> Text,
    }
}

diesel::table! {
    /// A course's dialogue lines (a view).
    course_lines (topic_id, id) {
        topic_id -> BigInt,
        id -> BigInt,
        block_id -> BigInt,
        ord -> BigInt,
        speaker -> Text,
        gender -> Text,
        pl -> Text,
        en -> Text,
    }
}

diesel::table! {
    checkins (id) {
        id -> BigInt,
        block_id -> BigInt,
        started_at -> BigInt,
        finished_at -> Nullable<BigInt>,
        result -> Nullable<Text>,
        feedback -> Nullable<Text>,
    }
}

diesel::table! {
    checkin_messages (id) {
        id -> BigInt,
        checkin_id -> BigInt,
        role -> Text,
        content -> Text,
        created_at -> BigInt,
    }
}

diesel::table! {
    tutor_conversations (id) {
        id -> BigInt,
        title -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    tutor_messages (id) {
        id -> BigInt,
        role -> Text,
        content -> Text,
        actions -> Nullable<Text>,
        created_at -> BigInt,
        conversation_id -> Nullable<BigInt>,
    }
}

diesel::table! {
    songs (id) {
        id -> BigInt,
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
        topic_id -> Nullable<BigInt>,
        /// JSON: how the song came about (songs::SongOrigin).
        origin -> Text,
    }
}

diesel::table! {
    song_tracks (id) {
        id -> BigInt,
        song_id -> BigInt,
        ord -> BigInt,
        remote_id -> Text,
        duration -> Double,
        play_count -> BigInt,
        last_played_at -> Nullable<BigInt>,
        aligned_words -> Nullable<Text>,
        audio_file_id -> Nullable<BigInt>,
        image_file_id -> Nullable<BigInt>,
    }
}

diesel::table! {
    playlists (id) {
        id -> BigInt,
        name -> Text,
        created_at -> BigInt,
    }
}

diesel::table! {
    playlist_tracks (id) {
        id -> BigInt,
        playlist_id -> BigInt,
        track_id -> BigInt,
        ord -> BigInt,
    }
}

diesel::table! {
    ai_costs (id) {
        id -> BigInt,
        day -> BigInt,
        feature -> Text,
        provider -> Text,
        model -> Text,
        cost_usd -> Nullable<Double>,
        detail -> Text,
        #[sql_name = "ref"]
        reference -> Nullable<Text>,
        created_at -> BigInt,
    }
}

diesel::table! {
    memories (id) {
        id -> BigInt,
        content -> Text,
        category -> Text,
        source -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::joinable!(words -> blocks (block_id));
diesel::joinable!(sentences -> blocks (block_id));
diesel::joinable!(conversation_lines -> blocks (block_id));
diesel::joinable!(course_blocks -> blocks (block_id));
diesel::joinable!(course_blocks -> topics (topic_id));
diesel::joinable!(course_words -> topics (topic_id));
diesel::joinable!(course_words -> blocks (block_id));
diesel::joinable!(course_sentences -> blocks (block_id));
diesel::joinable!(course_sentences -> topics (topic_id));
diesel::joinable!(course_lines -> topics (topic_id));
diesel::joinable!(checkins -> blocks (block_id));
diesel::joinable!(checkin_messages -> checkins (checkin_id));
diesel::joinable!(tutor_messages -> tutor_conversations (conversation_id));
diesel::joinable!(song_tracks -> songs (song_id));
diesel::joinable!(speech_clips -> files (file_id));
diesel::joinable!(blocks -> songs (song_id));
diesel::joinable!(songs -> topics (topic_id));
diesel::joinable!(playlist_tracks -> playlists (playlist_id));
diesel::joinable!(playlist_tracks -> song_tracks (track_id));

diesel::allow_tables_to_appear_in_same_query!(
    topics,
    words,
    sentences,
    conversation_lines,
    reviews,
    day_log,
    settings,
    achievements,
    files,
    speech_clips,
    blocks,
    course_blocks,
    course_words,
    course_sentences,
    course_lines,
    checkins,
    checkin_messages,
    tutor_conversations,
    tutor_messages,
    songs,
    song_tracks,
    playlists,
    playlist_tracks,
    ai_costs,
    memories,
);
