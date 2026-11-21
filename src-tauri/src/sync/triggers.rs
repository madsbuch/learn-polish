//! The triggers that put a table under sync, made from the table as it is in
//! the database: on every insert, update and delete they record the row's new
//! version in `sync_rows` (a clock value, this device, a tombstone flag, the
//! next local sequence number) and this device's latest change in `sync_vv`.
//!
//! [`ensure`] runs at every open (db.rs). It compares each synced table's
//! triggers with what its current columns call for and re-creates any that
//! differ, so a schema change that adds a column or rebuilds a table only
//! has to bump `SCHEMA_VERSION`: the triggers follow. A table with no
//! triggers yet (a new one, or every table when a database is upgraded to
//! version 8) first has its rows recorded as this device's first version of
//! them: bookkeeping, not content (docs/SYNC.md item 7).
//!
//! The SQL keeps to what older SQLite builds parse (no `unixepoch('subsec')`):
//! a future Android worker may open this file with the system's SQLite.

use std::collections::{BTreeMap, BTreeSet};

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Nullable, Text};

use crate::db::Conn;
use crate::error::{Error, Result};

/// Tables that never sync: the sync machinery itself and what is about this
/// device only.
pub const LOCAL_TABLES: &[&str] =
    &["sync_state", "sync_rows", "sync_vv", "sync_peers", "sync_epochs", "device_settings", "file_paths"];

/// Milliseconds since the epoch, from julianday.
pub(crate) const NOW_MS: &str = "CAST((julianday('now') - 2440587.5) * 86400000.0 AS INTEGER)";

/// The hybrid logical clock's tick, as SET clauses on sync_state (SQLite
/// evaluates them all against the old row): the wall clock if it has moved
/// on, else one more within the same millisecond — rolling into the next
/// millisecond rather than let the 6-digit counter overflow and sort wrong.
pub(crate) fn tick_sql() -> String {
    format!(
        "hlc_counter = CASE WHEN {NOW_MS} > hlc_ms THEN 0 WHEN hlc_counter >= 999999 THEN 0 ELSE hlc_counter + 1 END,
    hlc_ms = CASE WHEN {NOW_MS} > hlc_ms THEN {NOW_MS} WHEN hlc_counter >= 999999 THEN hlc_ms + 1 ELSE hlc_ms END"
    )
}

/// A device's entry in a row's version vector is the clock value of its
/// latest change to it (as one integer), not a count of changes: a copy of
/// the database restored from earlier still writes values above any the
/// original used, so its edits never look identical to ones they never saw.
const VV_TICK: &str = "hlc_ms * 1000000 + hlc_counter";

/// A table's columns and its primary key, in key order.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub table: String,
    pub key: Vec<String>,
    pub columns: Vec<String>,
}

pub(crate) fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

#[derive(QueryableByName)]
struct Name {
    #[diesel(sql_type = Text)]
    name: String,
}

#[derive(QueryableByName)]
struct ColumnRow {
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = BigInt)]
    pk: i64,
}

/// Every synced table, by name: every table but SQLite's own and the local ones.
pub fn synced_table_names(conn: &mut Conn) -> Result<Vec<String>> {
    Ok(diesel::sql_query("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name")
        .load::<Name>(conn)?
        .into_iter()
        .map(|n| n.name)
        .filter(|n| !LOCAL_TABLES.contains(&n.as_str()))
        .collect())
}

pub fn shape(conn: &mut Conn, table: &str) -> Result<Shape> {
    let cols: Vec<ColumnRow> =
        diesel::sql_query("SELECT name, pk FROM pragma_table_info(?) ORDER BY cid").bind::<Text, _>(table).load(conn)?;
    let mut key: Vec<(i64, String)> = cols.iter().filter(|c| c.pk > 0).map(|c| (c.pk, c.name.clone())).collect();
    key.sort();
    if key.is_empty() {
        return Err(Error::msg(format!("{table}: sync needs a primary key")));
    }
    Ok(Shape { table: table.to_string(), key: key.into_iter().map(|(_, k)| k).collect(), columns: cols.into_iter().map(|c| c.name).collect() })
}

/// The SQL for a row's sync key, from its key columns as text, joined by
/// U+001F (`sync::KEY_SEPARATOR`) when there are several. `row` is how the
/// row is named there (`NEW`, `OLD`, a table alias) — or empty, for a bare
/// column.
pub(crate) fn key_sql(key: &[String], row: &str) -> String {
    key.iter()
        .map(|c| if row.is_empty() { format!("CAST({} AS TEXT)", quote(c)) } else { format!("CAST({row}.{} AS TEXT)", quote(c)) })
        .collect::<Vec<_>>()
        .join(" || char(31) || ")
}

/// The names of a table's sync triggers, in the order [`trigger_sql`] makes them.
fn trigger_names(table: &str) -> [String; 4] {
    ["ai", "au", "ak", "ad"].map(|k| format!("sync_{table}_{k}"))
}

/// The triggers that record a table's changes: (name, CREATE statement).
pub fn trigger_sql(shape: &Shape) -> Vec<(String, String)> {
    let table = literal(&shape.table);
    let record = |row: &str, deleted: u8| {
        let key = key_sql(&shape.key, row);
        format!(
            "
  UPDATE sync_state SET
    {tick},
    seq = seq + 1
  WHERE id = 1;
  INSERT INTO sync_rows (tbl, pk, hlc, origin, deleted, seq, from_peer)
    SELECT {table}, {key}, printf('%013d.%06d', hlc_ms, hlc_counter), device_id, {deleted}, seq, NULL
    FROM sync_state WHERE id = 1
    ON CONFLICT (tbl, pk) DO UPDATE SET
      hlc = excluded.hlc, origin = excluded.origin, deleted = excluded.deleted,
      seq = excluded.seq, from_peer = NULL;
  INSERT INTO sync_vv (tbl, pk, device, counter)
    SELECT {table}, {key}, device_id, {VV_TICK} FROM sync_state WHERE id = 1
    ON CONFLICT (tbl, pk, device) DO UPDATE SET counter = max(counter + 1, excluded.counter);",
            tick = tick_sql(),
        )
    };
    let when = "WHEN (SELECT applying FROM sync_state WHERE id = 1) = 0";
    // A row without a whole key can't be named to another device: it isn't
    // recorded (rather than fail the write). The app never makes one; a
    // key column declared without NOT NULL could only hold one by mistake.
    let keyed = |row: &str| format!("({}) IS NOT NULL", key_sql(&shape.key, row));
    // An update that changes nothing (the same values written again) is not
    // a new version: it would otherwise win over a real edit made
    // concurrently elsewhere.
    let changed =
        shape.columns.iter().map(|c| format!("OLD.{0} IS NOT NEW.{0}", quote(c))).collect::<Vec<_>>().join(" OR ");
    // A row whose key changes (the demo course's rows are given derived ids
    // after they are made) is a new row under the new key and a deletion of
    // the old one, or peers would keep the old key forever.
    let rekeyed =
        shape.key.iter().map(|c| format!("OLD.{0} IS NOT NEW.{0}", quote(c))).collect::<Vec<_>>().join(" OR ");
    let t = quote(&shape.table);
    let [ai, au, ak, ad] = trigger_names(&shape.table);
    vec![
        (ai.clone(), format!("CREATE TRIGGER {} AFTER INSERT ON {t} {when} AND {}\nBEGIN{}\nEND", quote(&ai), keyed("NEW"), record("NEW", 0))),
        (au.clone(), format!("CREATE TRIGGER {} AFTER UPDATE ON {t} {when} AND {} AND ({changed})\nBEGIN{}\nEND", quote(&au), keyed("NEW"), record("NEW", 0))),
        (ak.clone(), format!("CREATE TRIGGER {} AFTER UPDATE ON {t} {when} AND {} AND ({rekeyed})\nBEGIN{}\nEND", quote(&ak), keyed("OLD"), record("OLD", 1))),
        (ad.clone(), format!("CREATE TRIGGER {} AFTER DELETE ON {t} {when} AND {}\nBEGIN{}\nEND", quote(&ad), keyed("OLD"), record("OLD", 1))),
    ]
}

/// Records every row already in a table as this device's first version of
/// it. Rows already recorded are left as they are.
pub fn backfill(conn: &mut Conn, shape: &Shape) -> Result<()> {
    let table = literal(&shape.table);
    let key = key_sql(&shape.key, "t");
    // Rows without a whole key can't be synced (see `trigger_sql`); they stay
    // here, untracked, and Diagnostics says how many.
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    let unkeyed = diesel::sql_query(format!("SELECT count(*) AS n FROM {} t WHERE ({key}) IS NULL", quote(&shape.table)))
        .get_result::<Count>(conn)?
        .n;
    if unkeyed > 0 {
        crate::diagnostics::note(
            "sync",
            format!("{unkeyed} rows of {} have no key and won't sync to other devices.", shape.table),
        );
    }
    conn.batch_execute(&format!(
        "INSERT INTO sync_rows (tbl, pk, hlc, origin, deleted, seq, from_peer)
           SELECT {table}, {key}, printf('%013d.%06d', max(s.hlc_ms, {NOW_MS}), 0), s.device_id, 0,
             max(s.seq, (SELECT coalesce(max(seq), 0) FROM sync_rows)) + row_number() OVER (ORDER BY {key}), NULL
           FROM {t} t, sync_state s WHERE s.id = 1 AND ({key}) IS NOT NULL
           ON CONFLICT (tbl, pk) DO NOTHING;
         INSERT INTO sync_vv (tbl, pk, device, counter)
           SELECT {table}, {key}, s.device_id, 1 FROM {t} t, sync_state s WHERE s.id = 1 AND ({key}) IS NOT NULL
           ON CONFLICT (tbl, pk, device) DO NOTHING;
         UPDATE sync_state SET seq = max(seq, (SELECT coalesce(max(seq), 0) FROM sync_rows)),
           hlc_ms = max(hlc_ms, {NOW_MS}) WHERE id = 1;",
        t = quote(&shape.table),
    ))?;
    Ok(())
}

#[derive(QueryableByName)]
struct TriggerRow {
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Nullable<Text>)]
    sql: Option<String>,
}

/// The sync triggers a table has now: name → statement.
pub fn existing_triggers(conn: &mut Conn, table: &str) -> Result<BTreeMap<String, String>> {
    Ok(diesel::sql_query(
        "SELECT name, sql FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ? AND name LIKE 'sync\\_%' ESCAPE '\\'",
    )
    .bind::<Text, _>(table)
    .load::<TriggerRow>(conn)?
    .into_iter()
    .map(|t| (t.name, t.sql.unwrap_or_default()))
    .collect())
}

/// Puts every synced table under sync as it is now: missing or outdated
/// triggers are (re)made, and a table that had none has its rows recorded
/// first, parents before the rows that point at them (so a device that
/// pulls part of the backfill has whole records rather than orphans).
pub fn ensure(conn: &mut Conn) -> Result<()> {
    let tables = synced_table_names(conn)?;
    let mut untracked = Vec::new();
    for table in &tables {
        let shape = shape(conn, table)?;
        let wanted: BTreeMap<String, String> = trigger_sql(&shape).into_iter().collect();
        let existing = existing_triggers(conn, table)?;
        if existing.is_empty() {
            untracked.push(shape.clone());
        }
        if existing != wanted {
            for name in existing.keys() {
                conn.batch_execute(&format!("DROP TRIGGER IF EXISTS {}", quote(name)))?;
            }
            for sql in wanted.values() {
                conn.batch_execute(sql)?;
            }
        }
    }
    let order = parents_first(conn, untracked.iter().map(|s| s.table.clone()).collect())?;
    for table in order {
        if let Some(shape) = untracked.iter().find(|s| s.table == table) {
            backfill(conn, shape)?;
        }
    }
    Ok(())
}

#[derive(QueryableByName)]
struct Parent {
    #[diesel(sql_type = Text)]
    #[diesel(column_name = "table")]
    parent: String,
}

/// Tables ordered so each comes after the tables it references (ties, and
/// any cycle, by name).
fn parents_first(conn: &mut Conn, tables: Vec<String>) -> Result<Vec<String>> {
    let mut parents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for table in &tables {
        let of: BTreeSet<String> = diesel::sql_query("SELECT \"table\" FROM pragma_foreign_key_list(?)")
            .bind::<Text, _>(table)
            .load::<Parent>(conn)?
            .into_iter()
            .map(|p| p.parent)
            .filter(|p| p != table && tables.contains(p))
            .collect();
        parents.insert(table.clone(), of);
    }
    let mut out: Vec<String> = Vec::new();
    while out.len() < tables.len() {
        let ready = parents
            .iter()
            .find(|(t, of)| !out.contains(t) && of.iter().all(|p| out.contains(p)))
            .map(|(t, _)| t.clone());
        // A cycle: take the first left, so the order is still complete.
        let next = ready.or_else(|| parents.keys().find(|t| !out.contains(t)).cloned());
        match next {
            Some(t) => out.push(t),
            None => break,
        }
    }
    Ok(out)
}
