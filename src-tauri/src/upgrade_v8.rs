//! Schema version 8: sync between devices (docs/SYNC.md). Runs once, on a
//! version 7 database, in one transaction — after the backup every upgrade
//! gets (db.rs) — and does nothing on one already converted.
//!
//! 1. The sync tables, `device_settings` and `file_paths` are made (the
//!    schema), and this device gets its id.
//! 2. Settings about the device (db.rs `DEVICE_SETTINGS`) move from
//!    `settings` to `device_settings`.
//! 3. `day_log` is keyed by day and device; its rows become this device's.
//! 4. `files.path` (where this device wrote a file out) moves to
//!    `file_paths`.
//! 5. Every row already here is recorded as this device's first version of
//!    it, and every synced table gets its triggers (sync/triggers.rs).
//! 6. `foreign_key_check` must come back empty, or the whole upgrade is
//!    undone.
//!
//! It reads the version 7 tables with plain SQL: the typed schema
//! (`schema.rs`) describes version 8.

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::Text;

use crate::db::Conn;
use crate::error::{Error, Result};

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

fn column_exists(conn: &mut Conn, table: &str, column: &str) -> Result<bool> {
    Ok(diesel::sql_query("SELECT name FROM pragma_table_info(?)")
        .bind::<Text, _>(table)
        .load::<Name>(conn)?
        .iter()
        .any(|c| c.name == column))
}

/// Whether this database is a version 7 one, still to be converted (a new
/// database is made as version 8 directly).
pub fn needed(conn: &mut Conn) -> Result<bool> {
    if !table_exists(conn, "courses")? {
        return Ok(false);
    }
    Ok(!table_exists(conn, "sync_state")?
        || (table_exists(conn, "files")? && column_exists(conn, "files", "path")?)
        || (table_exists(conn, "day_log")? && !column_exists(conn, "day_log", "device")?))
}

/// Converts a version 7 database to version 8. Foreign keys are off while
/// tables are rebuilt, and checked before the transaction commits.
pub fn run(conn: &mut Conn, schema: &str) -> Result<()> {
    conn.batch_execute("PRAGMA foreign_keys = OFF")?;
    let result = conn.transaction::<_, Error, _>(|tx| convert(tx, schema));
    conn.batch_execute("PRAGMA foreign_keys = ON")?;
    result
}

fn convert(conn: &mut Conn, schema: &str) -> Result<()> {
    // The view over day_log is made again by the schema, over the new table.
    conn.batch_execute("DROP VIEW IF EXISTS day_totals")?;

    // 1. The new tables, and this device's id.
    conn.batch_execute(schema)?;
    crate::sync::ensure_state(conn)?;
    let device = crate::sync::device_id(conn)?.to_string();

    // 2. Settings about the device.
    let keys = crate::db::DEVICE_SETTINGS.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(", ");
    conn.batch_execute(&format!(
        "INSERT OR IGNORE INTO device_settings (key, value) SELECT key, value FROM settings WHERE key IN ({keys});
         DELETE FROM settings WHERE key IN ({keys});"
    ))?;

    // 3. A day's log per device.
    if !column_exists(conn, "day_log", "device")? {
        conn.batch_execute("DROP VIEW IF EXISTS day_totals; DROP TABLE IF EXISTS day_log_v8;")?;
        let definition = day_log_definition(schema)?.replacen("day_log", "day_log_v8", 1);
        conn.batch_execute(&definition)?;
        diesel::sql_query(
            "INSERT INTO day_log_v8 (day, device, xp, answered, correct, course_id, completed, frozen, minutes, early_minutes)
             SELECT day, ?, xp, answered, correct, course_id, completed, frozen, minutes, early_minutes FROM day_log",
        )
        .bind::<Text, _>(&device)
        .execute(conn)?;
        conn.batch_execute("DROP TABLE day_log; ALTER TABLE day_log_v8 RENAME TO day_log;")?;
    }

    // 4. Where files were written out, kept on this device only. The index
    //    that covered `path` goes with it; the schema makes it again.
    if column_exists(conn, "files", "path")? {
        conn.batch_execute(
            "INSERT OR IGNORE INTO file_paths (file_id, path) SELECT id, path FROM files WHERE path IS NOT NULL;
             DROP INDEX IF EXISTS files_meta;
             ALTER TABLE files DROP COLUMN path;",
        )?;
    }
    conn.batch_execute(schema)?;

    // 5. Every row recorded, every synced table watched.
    crate::sync::triggers::ensure(conn)?;

    let broken = crate::db::foreign_key_violations(conn)?;
    if broken > 0 {
        return Err(Error::msg(format!("The sync upgrade left {broken} broken references; nothing was changed.")));
    }
    crate::diagnostics::note("database", "Upgraded for sync between devices (schema version 8).");
    Ok(())
}

/// `CREATE TABLE ... day_log (...);` as the schema has it.
fn day_log_definition(schema: &str) -> Result<String> {
    let start = schema.find("CREATE TABLE IF NOT EXISTS day_log (").ok_or_else(|| Error::msg("the schema has no day_log"))?;
    let rest = &schema[start..];
    let end = rest.find(");").ok_or_else(|| Error::msg("the schema's day_log does not end"))?;
    Ok(rest[..end + 2].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::schema::{day_log, day_totals, device_settings, file_paths, files, settings, sync_rows};

    fn path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join("learn-polish-tests").join(format!("v8-{name}-{}.db", std::process::id()))
    }

    /// A version 7 database as the app left it: the v7 schema, the demo
    /// course and a practised history, a file written out, device settings.
    fn version_7(name: &str) -> std::path::PathBuf {
        let file = path(name);
        let _ = std::fs::remove_file(&file);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut conn = Conn::establish(&file.to_string_lossy()).unwrap();
        conn.batch_execute(include_str!("schema_v7.sql")).unwrap();
        let course = crate::ids::CourseId::derived("course", "v7-course");
        let lesson = crate::ids::LessonId::derived("lesson", "v7-lesson");
        let word = crate::ids::WordId::derived("word", "v7-word");
        let file_id = crate::ids::FileId::derived("test", "v7-file");
        conn.batch_execute(&format!(
            "INSERT INTO files (id, kind, name, mime, size, sha256, bytes, path, created_at)
                 VALUES ('{file_id}', 'song_image', 'cover.jpg', 'image/jpeg', 3, '', x'010203', '/tmp/cover.jpg', 5);
             INSERT INTO courses (id, slug, ord, title_pl, title_en, summary_en, image_file_id)
                 VALUES ('{course}', 'v7', 1, 'Kurs', 'Course', '', '{file_id}');
             INSERT INTO lessons (id, title, complexity, period_days, created_at) VALUES ('{lesson}', 'Lesson', 'low', 7, 1);
             INSERT INTO course_lessons (course_id, lesson_id, ord) VALUES ('{course}', '{lesson}', 1);
             INSERT INTO words (id, lesson_id, ord, pl, en, pos) VALUES ('{word}', '{lesson}', 1, 'kot', 'cat', 'n');
             INSERT INTO reviews VALUES ('word', '{word}', 20000, 3, 2.5, 2, 0, 19990);
             INSERT INTO day_log (day, xp, answered, correct, completed, minutes) VALUES (19990, 40, 8, 7, 1, 5), (19991, 12, 3, 2, 0, 0);
             INSERT INTO settings (key, value) VALUES ('theme', 'dark'), ('reminder_hour', '7'), ('tz_offset_minutes', '60'),
                 ('openrouter_api_key', 'sk-1'), ('daily_goal', '250'), ('starter_seeded', '1'),
                 ('demo_first_steps_seeded_v1', '1'), ('bundled_courses_retired_v1', '1');
             PRAGMA user_version = 7;"
        ))
        .unwrap();
        file
    }

    #[test]
    fn a_version_7_database_is_upgraded_for_sync() {
        let file = version_7("upgrade");
        let before: Vec<(i64, i64)> = {
            let mut conn = Conn::establish(&file.to_string_lossy()).unwrap();
            diesel::sql_query("SELECT day AS n, xp AS m FROM day_log ORDER BY day")
                .load::<Pair>(&mut conn)
                .unwrap()
                .into_iter()
                .map(|p| (p.n, p.m))
                .collect()
        };
        let conn = &mut db::open(&file).unwrap();
        let me = crate::sync::device_id(conn).unwrap();

        // Rows kept.
        assert_eq!(crate::schema::words::table.count().get_result::<i64>(conn).unwrap(), 1);
        assert_eq!(crate::schema::reviews::table.count().get_result::<i64>(conn).unwrap(), 1);
        assert_eq!(files::table.select(files::bytes).first::<Vec<u8>>(conn).unwrap(), vec![1, 2, 3]);

        // Device settings moved, the learner's stayed.
        let device: Vec<String> = device_settings::table.order(device_settings::key).select(device_settings::key).load(conn).unwrap();
        assert_eq!(
            device,
            [
                "bundled_courses_retired_v1",
                "demo_first_steps_seeded_v1",
                "reminder_hour",
                "starter_seeded",
                // Where this copy of the database lives (sync/instance.rs).
                "sync_instance",
                "theme",
                "tz_offset_minutes",
            ]
        );
        assert_eq!(db::get_setting(conn, "theme").unwrap().as_deref(), Some("dark"));
        assert_eq!(db::get_setting_i64(conn, "tz_offset_minutes", 0), 60);
        assert!(settings::table.find("theme").count().get_result::<i64>(conn).unwrap() == 0);
        assert_eq!(db::get_setting(conn, "openrouter_api_key").unwrap().as_deref(), Some("sk-1"));

        // The day log is this device's, and its sums are unchanged.
        let devices: Vec<crate::ids::DeviceId> = day_log::table.select(day_log::device).load(conn).unwrap();
        assert!(devices.len() == 2 && devices.iter().all(|d| *d == me));
        let after: Vec<(i64, i64)> = day_totals::table.order(day_totals::day).select((day_totals::day, day_totals::xp)).load(conn).unwrap();
        assert_eq!(after, before);
        let completed: i64 = day_totals::table.find(19990).select(day_totals::completed).first(conn).unwrap();
        assert_eq!(completed, 1);

        // The file's path is this device's alone.
        let paths: Vec<String> = file_paths::table.select(file_paths::path).load(conn).unwrap();
        assert_eq!(paths, ["/tmp/cover.jpg"]);
        assert!(!column_exists(conn, "files", "path").unwrap());

        // Every synced row is recorded, as this device's.
        for table in crate::sync::triggers::synced_table_names(conn).unwrap() {
            let shape = crate::sync::triggers::shape(conn, &table).unwrap();
            let rows = diesel::sql_query(format!("SELECT count(*) AS n, 0 AS m FROM \"{table}\"")).get_result::<Pair>(conn).unwrap().n;
            let tracked: i64 = sync_rows::table
                .filter(sync_rows::tbl.eq(&table))
                .filter(sync_rows::deleted.eq(0))
                .count()
                .get_result(conn)
                .unwrap();
            assert_eq!(rows, tracked, "{table}: every row has a version");
            let untracked = diesel::sql_query(format!(
                "SELECT count(*) AS n, 0 AS m FROM \"{table}\" t WHERE NOT EXISTS
                     (SELECT 1 FROM sync_rows r WHERE r.tbl = '{table}' AND r.pk = {})",
                crate::sync::triggers::key_sql(&shape.key, "t")
            ))
            .get_result::<Pair>(conn)
            .unwrap()
            .n;
            assert_eq!(untracked, 0, "{table}");
        }
        assert!(crate::sync::written_here(conn, "words", &crate::ids::WordId::derived("word", "v7-word").to_string()));
        assert_eq!(db::foreign_key_violations(conn).unwrap(), 0);

        // Opening again changes nothing: the upgrade ran once.
        let seq: i64 = crate::schema::sync_state::table.select(crate::schema::sync_state::seq).first(conn).unwrap();
        let conn = &mut db::open(&file).unwrap();
        assert!(!needed(conn).unwrap());
        assert_eq!(crate::sync::device_id(conn).unwrap(), me);
        let again: i64 = crate::schema::sync_state::table.select(crate::schema::sync_state::seq).first(conn).unwrap();
        assert_eq!(again, seq, "nothing recorded twice");
    }

    #[test]
    fn a_failed_upgrade_changes_nothing_and_can_run_again() {
        let file = version_7("failed");
        let mut conn = Conn::establish(&file.to_string_lossy()).unwrap();
        // A schema the upgrade cannot apply: it stops part-way.
        let broken = format!("{}\nCREATE TABLE nonsense (", db::SCHEMA);
        assert!(run(&mut conn, &broken).is_err());
        assert!(needed(&mut conn).unwrap());
        assert!(column_exists(&mut conn, "files", "path").unwrap());
        assert!(!table_exists(&mut conn, "sync_state").unwrap());
        drop(conn);
        let conn = &mut db::open(&file).unwrap();
        assert!(!needed(conn).unwrap());
        assert_eq!(db::get_setting(conn, "theme").unwrap().as_deref(), Some("dark"));
    }

    #[derive(QueryableByName)]
    struct Pair {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        m: i64,
    }
}
