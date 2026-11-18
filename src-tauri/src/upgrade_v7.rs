//! Schema version 7: every integer id becomes a tagged TypeID (DATA.md,
//! docs/ID_MIGRATION.md). Runs once, on a database from before, in one
//! transaction — after the backup every upgrade gets (db.rs).
//!
//! 1. Each table's rows get their new ids, in a mapping (old → new): the
//!    UUIDv7 inside takes the row's own time, so new ids sort as old ones
//!    did. Built-in courses and lessons get derived ids (from their slugs).
//! 2. Old tables are set aside, the version 7 tables made (`schema.sql`),
//!    and every row copied across through the mappings — references too.
//! 3. Ids inside stored JSON (a tutor message's actions, a song's origin,
//!    the day's list) are rewritten by key.
//! 4. `foreign_key_check` must come back empty, or the whole upgrade is
//!    undone.
//!
//! It reads the old tables with plain SQL: the typed schema (`schema.rs`)
//! describes version 7, not what is converted here.

use std::collections::HashMap;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Nullable, Text};

use crate::db::Conn;
use crate::error::{Error, Result};
use crate::ids::*;

/// A table with an integer id: how its new id is made, and what time each
/// row's id takes (an SQL expression over the row `t`, in seconds).
struct Keyed {
    table: &'static str,
    new_id: fn(u64, u128) -> String,
    time: &'static str,
}

macro_rules! maker {
    ($t:ty) => {
        |ms, random| <$t>::at(ms, random).to_string()
    };
}

const KEYED: &[Keyed] = &[
    Keyed { table: "files", new_id: maker!(FileId), time: "t.created_at" },
    Keyed { table: "courses", new_id: maker!(CourseId), time: "t.created_at" },
    Keyed { table: "songs", new_id: maker!(SongId), time: "t.created_at" },
    Keyed { table: "lessons", new_id: maker!(LessonId), time: "t.created_at" },
    Keyed { table: "words", new_id: maker!(WordId), time: "(SELECT created_at FROM lessons l WHERE l.id = t.lesson_id)" },
    Keyed { table: "sentences", new_id: maker!(SentenceId), time: "(SELECT created_at FROM lessons l WHERE l.id = t.lesson_id)" },
    Keyed {
        table: "conversation_lines",
        new_id: maker!(LineId),
        time: "(SELECT created_at FROM lessons l WHERE l.id = t.lesson_id)",
    },
    Keyed { table: "song_tracks", new_id: maker!(TrackId), time: "(SELECT created_at FROM songs s WHERE s.id = t.song_id)" },
    Keyed { table: "playlists", new_id: maker!(PlaylistId), time: "t.created_at" },
    Keyed {
        table: "playlist_tracks",
        new_id: maker!(EntryId),
        time: "(SELECT created_at FROM playlists p WHERE p.id = t.playlist_id)",
    },
    Keyed { table: "checkins", new_id: maker!(CheckinId), time: "t.started_at" },
    Keyed { table: "checkin_messages", new_id: maker!(CheckinMessageId), time: "t.created_at" },
    Keyed { table: "tutor_conversations", new_id: maker!(ConversationId), time: "t.created_at" },
    Keyed { table: "tutor_messages", new_id: maker!(MessageId), time: "t.created_at" },
    Keyed { table: "memories", new_id: maker!(MemoryId), time: "t.created_at" },
    Keyed {
        table: "wiki_sources",
        new_id: maker!(SourceId),
        time: "COALESCE((SELECT created_at FROM lessons l WHERE l.id = t.lesson_id), \
               (SELECT created_at FROM songs s WHERE s.id = t.song_id), 0)",
    },
    Keyed { table: "xp_log", new_id: maker!(XpEntryId), time: "t.at" },
    Keyed { table: "ai_costs", new_id: maker!(CostId), time: "t.created_at" },
    Keyed { table: "bench_runs", new_id: maker!(BenchRunId), time: "t.created_at" },
];

/// Columns that hold another table's id: (table, column, the table it names).
const REFERENCES: &[(&str, &str, &str)] = &[
    ("courses", "image_file_id", "files"),
    ("words", "lesson_id", "lessons"),
    ("words", "image_file_id", "files"),
    ("sentences", "lesson_id", "lessons"),
    ("conversation_lines", "lesson_id", "lessons"),
    ("xp_log", "lesson_id", "lessons"),
    ("day_log", "course_id", "courses"),
    ("wiki_sources", "lesson_id", "lessons"),
    ("wiki_sources", "song_id", "songs"),
    ("wiki_sources", "image_file_id", "files"),
    ("speech_clips", "file_id", "files"),
    ("lessons", "song_id", "songs"),
    ("course_lessons", "course_id", "courses"),
    ("course_lessons", "lesson_id", "lessons"),
    ("checkins", "lesson_id", "lessons"),
    ("checkin_messages", "checkin_id", "checkins"),
    ("tutor_messages", "conversation_id", "tutor_conversations"),
    ("songs", "course_id", "courses"),
    ("song_tracks", "song_id", "songs"),
    ("song_tracks", "audio_file_id", "files"),
    ("song_tracks", "image_file_id", "files"),
    ("playlist_tracks", "playlist_id", "playlists"),
    ("playlist_tracks", "track_id", "song_tracks"),
    ("word_examples", "word_id", "words"),
];

/// Tables kept by their natural keys, copied with their references mapped.
const NATURAL: &[&str] = &["reviews", "day_log", "settings", "achievements", "speech_clips", "course_lessons", "word_examples"];

#[derive(QueryableByName)]
struct OldRow {
    #[diesel(sql_type = BigInt)]
    id: i64,
    #[diesel(sql_type = Nullable<BigInt>)]
    time: Option<i64>,
    #[diesel(sql_type = Nullable<Text>)]
    slug: Option<String>,
}

#[derive(QueryableByName)]
struct Name {
    #[diesel(sql_type = Text)]
    name: String,
}

fn table_exists(conn: &mut Conn, table: &str) -> Result<bool> {
    Ok(!diesel::sql_query("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?")
        .bind::<Text, _>(table)
        .load::<Name>(conn)?
        .is_empty())
}

fn columns(conn: &mut Conn, table: &str) -> Result<Vec<String>> {
    Ok(diesel::sql_query(format!("SELECT name FROM pragma_table_info('{table}')"))
        .load::<Name>(conn)?
        .into_iter()
        .map(|n| n.name)
        .collect())
}

/// Whether this database still has integer ids (from before version 7).
pub fn needed(conn: &mut Conn) -> Result<bool> {
    #[derive(QueryableByName)]
    struct Kind {
        #[diesel(sql_type = Text)]
        #[diesel(column_name = "type")]
        kind: String,
    }
    // Any table that has always had an id tells: a very old database has
    // topics and words but no courses yet.
    for table in ["courses", "topics", "blocks", "words", "songs", "tutor_messages"] {
        if !table_exists(conn, table)? {
            continue;
        }
        let id = diesel::sql_query(format!("SELECT type FROM pragma_table_info('{table}') WHERE name = 'id'")).load::<Kind>(conn)?;
        return Ok(id.first().is_some_and(|c| c.kind.eq_ignore_ascii_case("INTEGER")));
    }
    Ok(false)
}

/// Converts the database to TypeIDs. Foreign keys are off while tables are
/// rebuilt, and checked before the transaction commits.
pub fn run(conn: &mut Conn, schema: &str) -> Result<()> {
    conn.batch_execute("PRAGMA foreign_keys = OFF; PRAGMA legacy_alter_table = ON;")?;
    let result = conn.transaction::<_, Error, _>(|tx| convert(tx, schema));
    conn.batch_execute("PRAGMA legacy_alter_table = OFF; PRAGMA foreign_keys = ON;")?;
    result
}

fn convert(conn: &mut Conn, schema: &str) -> Result<()> {
    // 1. New ids for every row, by table.
    let mut maps: HashMap<&'static str, HashMap<i64, String>> = HashMap::new();
    for keyed in KEYED {
        if !table_exists(conn, keyed.table)? {
            continue;
        }
        let has_slug = columns(conn, keyed.table)?.iter().any(|c| c == "slug");
        let slug = if has_slug { "t.slug" } else { "NULL" };
        let rows: Vec<OldRow> = diesel::sql_query(format!(
            "SELECT t.id AS id, {} AS time, {slug} AS slug FROM {} t ORDER BY t.id",
            keyed.time, keyed.table
        ))
        .load(conn)?;
        let mut last_ms: u64 = 0;
        let mut map = HashMap::new();
        for row in rows {
            let ms = crate::convert::unsigned(row.time.unwrap_or(0)).saturating_mul(1000).max(last_ms + 1);
            last_ms = ms;
            let id = match (keyed.table, row.slug.as_deref()) {
                // Built-in content: the same id on every device.
                ("courses", Some(slug)) if !slug.starts_with("course-") => CourseId::derived("course", slug).to_string(),
                ("lessons", Some(slug)) => LessonId::derived("lesson", slug).to_string(),
                _ => (keyed.new_id)(ms, rand::random()),
            };
            map.insert(row.id, id);
        }
        maps.insert(keyed.table, map);
    }
    for (table, map) in &maps {
        conn.batch_execute(&format!("CREATE TEMP TABLE map_{table} (old INTEGER PRIMARY KEY, new TEXT NOT NULL);"))?;
        for chunk in map.iter().collect::<Vec<_>>().chunks(400) {
            let values = chunk.iter().map(|(old, new)| format!("({old}, '{new}')")).collect::<Vec<_>>().join(",");
            conn.batch_execute(&format!("INSERT INTO map_{table} (old, new) VALUES {values};"))?;
        }
    }

    // 2. Set the old tables aside; drop what hangs on them (views, triggers,
    //    indexes) — the schema makes them again over the new tables.
    let mut tables: Vec<&str> = KEYED.iter().map(|k| k.table).chain(NATURAL.iter().copied()).collect();
    tables.retain(|t| maps.contains_key(t) || NATURAL.contains(t));
    let mut present = Vec::new();
    for table in tables {
        if table_exists(conn, table)? {
            present.push(table);
        }
    }
    for (kind, name) in [("view", "course_words"), ("view", "course_sentences"), ("view", "course_lines")] {
        conn.batch_execute(&format!("DROP {} IF EXISTS {name};", kind.to_uppercase()))?;
    }
    let hanging: Vec<(String, String)> = {
        #[derive(QueryableByName)]
        struct Item {
            #[diesel(sql_type = Text)]
            #[diesel(column_name = "type")]
            kind: String,
            #[diesel(sql_type = Text)]
            name: String,
        }
        diesel::sql_query(
            "SELECT type, name FROM sqlite_master WHERE type IN ('trigger', 'index') AND sql IS NOT NULL",
        )
        .load::<Item>(conn)?
        .into_iter()
        .map(|i| (i.kind, i.name))
        .collect()
    };
    for (kind, name) in hanging {
        conn.batch_execute(&format!("DROP {} IF EXISTS \"{name}\";", kind.to_uppercase()))?;
    }
    for table in &present {
        conn.batch_execute(&format!("ALTER TABLE {table} RENAME TO {table}__old;"))?;
    }
    conn.batch_execute(schema)?;

    // 3. Every row across, through the mappings.
    for table in &present {
        let new_cols = columns(conn, table)?;
        let old_cols = columns(conn, &format!("{table}__old"))?;
        let mut targets = Vec::new();
        let mut sources = Vec::new();
        for col in &new_cols {
            let source = if col == "id" && maps.contains_key(table) {
                Some(format!("(SELECT new FROM map_{table} m WHERE m.old = o.id)"))
            } else if let Some((_, _, to)) = REFERENCES.iter().find(|(t, c, _)| t == table && c == col) {
                old_cols
                    .contains(col)
                    .then(|| format!("(SELECT new FROM map_{to} m WHERE m.old = o.{col})"))
            } else if *table == "reviews" && col == "item_id" {
                Some(
                    "CASE o.kind WHEN 'word' THEN (SELECT new FROM map_words m WHERE m.old = o.item_id) \
                     WHEN 'sentence' THEN (SELECT new FROM map_sentences m WHERE m.old = o.item_id) END"
                        .to_string(),
                )
            } else {
                old_cols.contains(col).then(|| format!("o.{col}"))
            };
            if let Some(source) = source {
                targets.push(col.clone());
                sources.push(source);
            }
        }
        // A row whose required reference points nowhere (it could not have,
        // with foreign keys on) is left behind rather than failing it all.
        conn.batch_execute(&format!(
            "INSERT OR IGNORE INTO {table} ({}) SELECT {} FROM {table}__old o;",
            targets.join(", "),
            sources.join(", ")
        ))?;
    }

    // 4. Ids inside stored JSON.
    rewrite_json(conn, "tutor_messages", "actions", &maps)?;
    rewrite_json(conn, "songs", "origin", &maps)?;
    rewrite_json(conn, "settings", "value", &maps)?;

    // Say what was left behind: rows that pointed at something long gone.
    #[derive(QueryableByName)]
    struct Left {
        #[diesel(sql_type = BigInt)]
        n: i64,
    }
    let mut left_behind = Vec::new();
    for table in &present {
        let n = diesel::sql_query(format!("SELECT (SELECT COUNT(*) FROM {table}__old) - (SELECT COUNT(*) FROM {table}) AS n"))
            .get_result::<Left>(conn)?
            .n;
        if n > 0 {
            left_behind.push(format!("{n} in {table}"));
        }
    }
    for table in &present {
        conn.batch_execute(&format!("DROP TABLE {table}__old;"))?;
    }
    for table in maps.keys() {
        conn.batch_execute(&format!("DROP TABLE temp.map_{table};"))?;
    }
    let broken = crate::db::foreign_key_violations(conn)?;
    if broken > 0 {
        return Err(Error::msg(format!("The id upgrade left {broken} broken references; nothing was changed.")));
    }
    crate::diagnostics::note("database", "Upgraded to tagged ids (schema version 7).");
    if !left_behind.is_empty() {
        crate::diagnostics::note(
            "database",
            format!(
                "Left behind in the id upgrade, as they pointed at nothing (still in the backup): {}.",
                left_behind.join(", ")
            ),
        );
    }
    Ok(())
}

/// Which mapping a JSON key's integer belongs to.
fn table_for_key(key: &str) -> Option<&'static str> {
    Some(match key {
        "courseId" | "course_id" => "courses",
        "lessonId" | "lesson_id" => "lessons",
        "songId" | "song_id" | "basedOn" | "based_on" | "podcastId" | "podcast_id" => "songs",
        "wordId" | "word_id" => "words",
        "playlistId" | "playlist_id" => "playlists",
        "trackId" | "track_id" => "song_tracks",
        "fileId" | "file_id" | "imageFileId" | "image_file_id" => "files",
        _ => return None,
    })
}

/// Rewrites integer ids in one JSON column, by key, throughout. A one-off
/// walk over JSON the app wrote itself: kept untyped because it reaches into
/// every shape that ever held an id. The day's list (`daily_ids`, a bare
/// list of lesson ids) is mapped as lessons.
fn rewrite_json(conn: &mut Conn, table: &str, column: &str, maps: &HashMap<&'static str, HashMap<i64, String>>) -> Result<()> {
    #[derive(QueryableByName)]
    struct Cell {
        #[diesel(sql_type = Text)]
        key: String,
        #[diesel(sql_type = Nullable<Text>)]
        value: Option<String>,
    }
    let key = if table == "settings" { "key" } else { "id" };
    let cells: Vec<Cell> =
        diesel::sql_query(format!("SELECT {key} AS key, {column} AS value FROM {table}")).load(conn)?;
    for cell in cells {
        let Some(text) = cell.value else { continue };
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let changed = if table == "settings" {
            cell.key == "daily_ids" && map_list(&mut value, maps.get("lessons"))
        } else {
            map_keys(&mut value, maps)
        };
        if changed {
            diesel::sql_query(format!("UPDATE {table} SET {column} = ? WHERE {key} = ?"))
                .bind::<Text, _>(value.to_string())
                .bind::<Text, _>(&cell.key)
                .execute(conn)?;
        }
    }
    Ok(())
}

fn map_list(value: &mut serde_json::Value, map: Option<&HashMap<i64, String>>) -> bool {
    let (Some(list), Some(map)) = (value.as_array_mut(), map) else { return false };
    let mapped: Vec<serde_json::Value> =
        list.iter().filter_map(|v| v.as_i64().and_then(|old| map.get(&old)).map(|new| new.clone().into())).collect();
    *list = mapped;
    true
}

fn map_keys(value: &mut serde_json::Value, maps: &HashMap<&'static str, HashMap<i64, String>>) -> bool {
    let mut changed = false;
    match value {
        serde_json::Value::Object(object) => {
            for (key, inner) in object.iter_mut() {
                if let (Some(table), Some(old)) = (table_for_key(key), inner.as_i64()) {
                    *inner = maps.get(table).and_then(|m| m.get(&old)).map_or(serde_json::Value::Null, |new| new.clone().into());
                    changed = true;
                } else {
                    changed |= map_keys(inner, maps);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                changed |= map_keys(item, maps);
            }
        }
        _ => {}
    }
    changed
}
