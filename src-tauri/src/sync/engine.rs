//! The sync engine: what one device sends another, and how a device takes in
//! what it is sent (docs/SYNC.md; ported from Tally's engine). No networking
//! here: two databases in one process sync through these functions in the
//! tests, and the transport only carries [`Batch`]es between them.
//!
//! The model:
//! - Every row of every synced table has a current version in `sync_rows`
//!   (hybrid-logical-clock time `hlc`, writing device `origin`, tombstone
//!   flag) and a version vector in `sync_vv` (per device, the clock value of
//!   its latest change to the row). Triggers keep both current for every
//!   local write (triggers.rs).
//! - A row is named by its key: its primary-key columns' values as text,
//!   joined by U+001F when there are several (`day_log`'s day and device).
//! - A device pulls "everything you have recorded since seq N" from a peer.
//! - An incoming version whose vector has seen everything the local one has
//!   replaces it; one the local version has already seen is ignored.
//! - Two versions made without either seeing the other (concurrent): the one
//!   with the later clock wins (ties by device id), and both devices end up
//!   with the same row and the merged vector.
//! - A row that collides with a different row on a unique column (the same
//!   word added to a lesson on two devices) keeps the lower key; the other is
//!   deleted everywhere.
//!
//! Every decision depends only on the two versions involved, so every device
//! reaches the same result whatever order changes arrive in.
//!
//! Unlike Tally, a tutor conversation continued on two devices needs no
//! splitting: its turns are rows of their own (`tutor_messages`), so both
//! sides' turns are kept, and only the conversation's own row (title, when
//! it was last updated) is settled by its clock.
//!
//! Rows are read and written column by column as SQLite holds them
//! ([`Value`]): an integer stays an integer, a real a real (bit for bit),
//! text text and a blob a blob.

use std::collections::{BTreeMap, HashMap};

use diesel::connection::SimpleConnection;
use diesel::deserialize::FromSql;
use diesel::prelude::*;
use diesel::query_builder::{BoxedSqlQuery, SqlQuery};
use diesel::row::{Field, NamedRow, Row};
use diesel::sql_types::{BigInt, Binary, Double, Nullable, Text};
use diesel::sqlite::{Sqlite, SqliteType};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::triggers::{key_sql, quote, synced_table_names, tick_sql};
use super::KEY_SEPARATOR;
use crate::db::Conn;
use crate::error::{Error, Result};
use crate::ids::DeviceId;
use crate::schema::{sync_peers, sync_rows, sync_state, sync_vv};

/// One column value, as SQLite stores it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    /// Bytes on the wire as bytes (MessagePack bin), not a list of numbers.
    Blob(#[serde(with = "serde_bytes")] Vec<u8>),
}

/// A row's version as one device holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub tbl: String,
    /// The row's key (see the module comment).
    pub pk: String,
    pub hlc: String,
    pub origin: String,
    pub deleted: bool,
    pub vv: BTreeMap<String, i64>,
    /// Column → value; `None` for a tombstone.
    pub data: Option<Vec<(String, Value)>>,
}

/// A slice of a device's changes, in the order it recorded them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    pub changes: Vec<Change>,
    /// Pull from here next time: every change up to this seq is covered.
    pub upto: i64,
    /// Nothing after `upto` yet.
    pub done: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ApplyStats {
    /// Versions taken over as they came.
    pub applied: usize,
    /// Versions this device already had, or had already moved past.
    pub skipped: usize,
    /// Versions made concurrently with this device's own, settled by clock.
    pub conflicts: usize,
    /// Rows dropped for colliding with another on a unique column.
    pub unique_merges: usize,
}

impl ApplyStats {
    fn add(&mut self, o: ApplyStats) {
        self.applied += o.applied;
        self.skipped += o.skipped;
        self.conflicts += o.conflicts;
        self.unique_merges += o.unique_merges;
    }
}

/// How a synced table is shaped.
#[derive(Debug, Clone)]
pub struct TableInfo {
    pub name: String,
    /// The primary-key columns, in key order.
    pub key: Vec<String>,
    /// Which of them hold integers (declared INTEGER): a key's text is read
    /// back as a number for those, so lookups use the index.
    pub integer_key: Vec<bool>,
    pub columns: Vec<String>,
    /// Columns declared BLOB (file bytes): the digest reads their length,
    /// not their bytes.
    pub blob_columns: Vec<String>,
    /// Column sets of the unique indexes other than the primary key.
    pub uniques: Vec<Vec<String>>,
    /// (column, parent table, parent column) of foreign keys that cascade on delete.
    pub cascades: Vec<(String, String, String)>,
    /// The same, for foreign keys set to NULL when their parent goes.
    pub set_nulls: Vec<(String, String, String)>,
}

#[derive(QueryableByName)]
struct ColumnInfo {
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = BigInt)]
    pk: i64,
    #[diesel(sql_type = Text)]
    #[diesel(column_name = "type")]
    decl: String,
}

#[derive(QueryableByName)]
struct IndexInfo {
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Text)]
    origin: String,
}

#[derive(QueryableByName)]
struct IndexColumn {
    #[diesel(sql_type = Nullable<Text>)]
    name: Option<String>,
}

#[derive(QueryableByName)]
struct ForeignKey {
    #[diesel(sql_type = Text)]
    from: String,
    #[diesel(sql_type = Text)]
    #[diesel(column_name = "table")]
    parent: String,
    #[diesel(sql_type = Nullable<Text>)]
    to: Option<String>,
    #[diesel(sql_type = Text)]
    on_delete: String,
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct KeyText {
    #[diesel(sql_type = Text)]
    k: String,
}

#[derive(QueryableByName)]
struct SchemaText {
    #[diesel(sql_type = Nullable<Text>)]
    sql: Option<String>,
}

/// The last schema read, by a hash of its text: reading the tables' shapes
/// takes a few hundred pragma queries, and every batch needs them.
static SHAPES: std::sync::Mutex<Option<Shapes>> = std::sync::Mutex::new(None);

/// The tables' shapes, and the hash of the schema they were read from.
type Shapes = ([u8; 32], BTreeMap<String, TableInfo>);

/// The synced tables, read from the database itself: a table is synced when
/// it carries the sync insert trigger (triggers.rs).
pub fn synced_tables(conn: &mut Conn) -> Result<BTreeMap<String, TableInfo>> {
    let schema = diesel::sql_query("SELECT group_concat(type || ' ' || name || ' ' || coalesce(sql, ''), char(30)) AS sql FROM sqlite_master")
        .get_result::<SchemaText>(conn)?
        .sql
        .unwrap_or_default();
    let fingerprint: [u8; 32] = Sha256::digest(schema.as_bytes()).into();
    let mut cache = SHAPES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((seen, tables)) = cache.as_ref() {
        if *seen == fingerprint {
            return Ok(tables.clone());
        }
    }
    let tables = read_tables(conn)?;
    *cache = Some((fingerprint, tables.clone()));
    Ok(tables)
}

fn read_tables(conn: &mut Conn) -> Result<BTreeMap<String, TableInfo>> {
    let mut out = BTreeMap::new();
    for name in synced_table_names(conn)? {
        if !super::triggers::existing_triggers(conn, &name)?.contains_key(&format!("sync_{name}_ai")) {
            continue;
        }
        let cols: Vec<ColumnInfo> = diesel::sql_query("SELECT name, pk, upper(type) AS type FROM pragma_table_info(?) ORDER BY cid")
            .bind::<Text, _>(&name)
            .load(conn)?;
        let mut key: Vec<(i64, String, bool)> =
            cols.iter().filter(|c| c.pk > 0).map(|c| (c.pk, c.name.clone(), c.decl.contains("INT"))).collect();
        key.sort();
        if key.is_empty() {
            return Err(Error::msg(format!("{name}: no primary key")));
        }
        let key_columns: Vec<String> = key.iter().map(|(_, c, _)| c.clone()).collect();
        let blob_columns = cols.iter().filter(|c| c.decl == "BLOB").map(|c| c.name.clone()).collect();
        let indexes: Vec<IndexInfo> = diesel::sql_query("SELECT name, origin FROM pragma_index_list(?) WHERE \"unique\" = 1")
            .bind::<Text, _>(&name)
            .load(conn)?;
        let mut uniques = Vec::new();
        for index in indexes {
            if index.origin == "pk" {
                continue;
            }
            let columns: Vec<String> = diesel::sql_query("SELECT name FROM pragma_index_info(?) ORDER BY seqno")
                .bind::<Text, _>(&index.name)
                .load::<IndexColumn>(conn)?
                .into_iter()
                .filter_map(|c| c.name)
                .collect();
            if columns == key_columns || columns.is_empty() {
                continue;
            }
            uniques.push(columns);
        }
        let fks: Vec<ForeignKey> =
            diesel::sql_query("SELECT \"from\", \"table\", \"to\", on_delete FROM pragma_foreign_key_list(?)")
                .bind::<Text, _>(&name)
                .load(conn)?;
        let mut cascades = Vec::new();
        let mut set_nulls = Vec::new();
        for fk in fks {
            // A reference without a column names its parent's key.
            let to = match fk.to {
                Some(to) => to,
                None => super::triggers::shape(conn, &fk.parent)?.key.into_iter().next().unwrap_or_default(),
            };
            if fk.on_delete.eq_ignore_ascii_case("cascade") {
                cascades.push((fk.from, fk.parent, to));
            } else if fk.on_delete.eq_ignore_ascii_case("set null") {
                set_nulls.push((fk.from, fk.parent, to));
            }
        }
        out.insert(
            name.clone(),
            TableInfo {
                name,
                key: key_columns,
                integer_key: key.iter().map(|(_, _, int)| *int).collect(),
                columns: cols.into_iter().map(|c| c.name).collect(),
                blob_columns,
                uniques,
                cascades,
                set_nulls,
            },
        );
    }
    Ok(out)
}

/// The schema version: two devices only sync on the same one.
pub fn schema_version() -> i64 {
    crate::db::SCHEMA_VERSION
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalState {
    pub device_id: String,
    pub vault_id: Option<String>,
    pub seq: i64,
}

pub fn local_state(conn: &mut Conn) -> Result<LocalState> {
    let (device_id, vault_id, seq): (DeviceId, Option<String>, i64) = sync_state::table
        .find(1)
        .select((sync_state::device_id, sync_state::vault_id, sync_state::seq))
        .first(conn)?;
    Ok(LocalState { device_id: device_id.to_string(), vault_id, seq })
}

// ---------------------------------------------------------------------------
// Reading and writing rows, value by value
// ---------------------------------------------------------------------------

/// A row of any table, its columns as SQLite holds them.
struct AnyRow(Vec<(String, Value)>);

impl QueryableByName<Sqlite> for AnyRow {
    fn build<'a>(row: &impl NamedRow<'a, Sqlite>) -> diesel::deserialize::Result<Self> {
        let mut out = Vec::with_capacity(row.field_count());
        for i in 0..row.field_count() {
            let field = Row::get(row, i).ok_or("a column of the row went missing")?;
            let name = field.field_name().unwrap_or_default().to_string();
            let value = match field.value() {
                None => Value::Null,
                Some(raw) => match raw.value_type() {
                    None => Value::Null,
                    Some(SqliteType::Long) => Value::Int(<i64 as FromSql<BigInt, Sqlite>>::from_sql(raw)?),
                    Some(SqliteType::Double) => Value::Real(<f64 as FromSql<Double, Sqlite>>::from_sql(raw)?),
                    Some(SqliteType::Binary) => Value::Blob(<Vec<u8> as FromSql<Binary, Sqlite>>::from_sql(raw)?),
                    Some(_) => Value::Text(<String as FromSql<Text, Sqlite>>::from_sql(raw)?),
                },
            };
            out.push((name, value));
        }
        Ok(AnyRow(out))
    }
}

type Query<'f> = BoxedSqlQuery<'f, Sqlite, SqlQuery>;

fn bind<'f>(query: Query<'f>, value: &'f Value) -> Query<'f> {
    match value {
        Value::Null => query.bind::<Nullable<Text>, _>(None::<&str>),
        Value::Int(i) => query.bind::<BigInt, _>(*i),
        Value::Real(f) => query.bind::<Double, _>(*f),
        Value::Text(s) => query.bind::<Text, _>(s.as_str()),
        Value::Blob(b) => query.bind::<Binary, _>(b.as_slice()),
    }
}

fn query<'f>(sql: String) -> Query<'f> {
    diesel::sql_query(sql).into_boxed()
}

/// The key's columns as values: the integer ones read back as numbers.
fn key_values(t: &TableInfo, key: &str) -> Result<Vec<Value>> {
    let parts: Vec<&str> = key.split(KEY_SEPARATOR).collect();
    if parts.len() != t.key.len() {
        return Err(Error::msg(format!("{}: key {key:?} doesn't have {} parts", t.name, t.key.len())));
    }
    Ok(parts
        .iter()
        .zip(&t.integer_key)
        .map(|(part, int)| match part.parse::<i64>() {
            Ok(n) if *int => Value::Int(n),
            _ => Value::Text(part.to_string()),
        })
        .collect())
}

/// A row's key, from its values (as the triggers make it).
fn key_of(t: &TableInfo, data: &[(String, Value)]) -> Result<String> {
    let mut parts = Vec::with_capacity(t.key.len());
    for column in &t.key {
        parts.push(match value_of(data, column) {
            Some(Value::Int(n)) => n.to_string(),
            Some(Value::Text(s)) => s.clone(),
            other => return Err(Error::msg(format!("{}: key column {column} holds {other:?}", t.name))),
        });
    }
    Ok(parts.join(&KEY_SEPARATOR.to_string()))
}

/// `"a" = ? AND "b" = ?`: the row with a key, by its key columns.
fn where_key(t: &TableInfo) -> String {
    t.key.iter().map(|c| format!("{} = ?", quote(c))).collect::<Vec<_>>().join(" AND ")
}

/// Every row of a table, in key order, as SQLite holds them.
pub(crate) fn rows_of(conn: &mut Conn, t: &TableInfo) -> Result<Vec<Vec<(String, Value)>>> {
    let order = t.key.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
    Ok(diesel::sql_query(format!("SELECT * FROM {} ORDER BY {order}", quote(&t.name)))
        .load::<AnyRow>(conn)?
        .into_iter()
        .map(|r| r.0)
        .collect())
}

fn read_row(conn: &mut Conn, t: &TableInfo, key: &str) -> Result<Option<Vec<(String, Value)>>> {
    let values = key_values(t, key)?;
    let mut q = query(format!("SELECT * FROM {} WHERE {}", quote(&t.name), where_key(t)));
    for v in &values {
        q = bind(q, v);
    }
    Ok(q.load::<AnyRow>(conn)?.into_iter().next().map(|r| r.0))
}

fn vv_of(conn: &mut Conn, tbl: &str, pk: &str) -> Result<BTreeMap<String, i64>> {
    Ok(sync_vv::table
        .filter(sync_vv::tbl.eq(tbl))
        .filter(sync_vv::pk.eq(pk))
        .select((sync_vv::device, sync_vv::counter))
        .load::<(String, i64)>(conn)?
        .into_iter()
        .collect())
}

fn data_size(data: &Option<Vec<(String, Value)>>) -> usize {
    data.as_ref().map_or(16, |d| {
        d.iter()
            .map(|(c, v)| {
                c.len()
                    + match v {
                        Value::Text(s) => s.len(),
                        Value::Blob(b) => b.len(),
                        _ => 9,
                    }
            })
            .sum()
    })
}

fn value_of<'a>(data: &'a [(String, Value)], col: &str) -> Option<&'a Value> {
    data.iter().find(|(c, _)| c == col).map(|(_, v)| v)
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// The changes recorded after `since`, for `peer`: at most `max_rows`, and
/// stopping once about `max_bytes` of row data is in (always at least one).
/// Versions that came from `peer` itself, unchanged since, aren't sent back.
pub fn export(conn: &mut Conn, since: i64, peer: &str, max_rows: i64, max_bytes: usize) -> Result<Batch> {
    // One read transaction, so the rows and their versions agree.
    conn.transaction(|tx| export_in(tx, since, peer, max_rows, max_bytes))
}

type VersionRow = (String, String, String, String, i64, i64, Option<String>);

fn export_in(conn: &mut Conn, since: i64, peer: &str, max_rows: i64, max_bytes: usize) -> Result<Batch> {
    let tables = synced_tables(conn)?;
    let rows: Vec<VersionRow> = sync_rows::table
        .filter(sync_rows::seq.gt(since))
        .order(sync_rows::seq)
        .limit(max_rows.max(1) + 1)
        .select((
            sync_rows::tbl,
            sync_rows::pk,
            sync_rows::hlc,
            sync_rows::origin,
            sync_rows::deleted,
            sync_rows::seq,
            sync_rows::from_peer,
        ))
        .load(conn)?;
    let more = crate::convert::count(rows.len()) > max_rows.max(1);
    let mut changes = Vec::new();
    let mut upto = since;
    let mut bytes = 0usize;
    let mut stopped_early = false;
    for (tbl, pk, hlc, origin, deleted, seq, from_peer) in rows.into_iter().take(crate::convert::index(max_rows.max(1))) {
        if bytes >= max_bytes && !changes.is_empty() {
            stopped_early = true;
            break;
        }
        upto = seq;
        if from_peer.as_deref() == Some(peer) {
            continue;
        }
        let Some(t) = tables.get(&tbl) else {
            // A table no longer synced on this side: nothing to send.
            continue;
        };
        let data = if deleted != 0 {
            None
        } else {
            match read_row(conn, t, &pk)? {
                Some(d) => Some(d),
                // Recorded as live but gone: not ours to invent a deletion for.
                None => continue,
            }
        };
        bytes += data_size(&data);
        let vv = vv_of(conn, &tbl, &pk)?;
        changes.push(Change { tbl, pk, hlc, origin, deleted: deleted != 0, vv, data });
    }
    Ok(Batch { changes, upto, done: !more && !stopped_early })
}

// ---------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Order {
    Equal,
    /// The incoming version has seen everything the local one has, and more.
    Newer,
    /// The local version has seen everything the incoming one has.
    Older,
    Concurrent,
}

pub(crate) fn compare_vv(incoming: &BTreeMap<String, i64>, local: &BTreeMap<String, i64>) -> Order {
    let mut greater = false;
    let mut less = false;
    for k in incoming.keys().chain(local.keys()) {
        let a = incoming.get(k).copied().unwrap_or(0);
        let b = local.get(k).copied().unwrap_or(0);
        if a > b {
            greater = true;
        }
        if a < b {
            less = true;
        }
    }
    match (greater, less) {
        (false, false) => Order::Equal,
        (true, false) => Order::Newer,
        (false, true) => Order::Older,
        (true, true) => Order::Concurrent,
    }
}

pub(crate) fn merge_vv(a: &BTreeMap<String, i64>, b: &BTreeMap<String, i64>) -> BTreeMap<String, i64> {
    let mut out = a.clone();
    for (k, v) in b {
        let e = out.entry(k.clone()).or_insert(0);
        *e = (*e).max(*v);
    }
    out
}

struct LocalVersion {
    hlc: String,
    origin: String,
    deleted: bool,
    vv: BTreeMap<String, i64>,
}

fn local_version(conn: &mut Conn, tbl: &str, pk: &str) -> Result<Option<LocalVersion>> {
    let row: Option<(String, String, i64)> = sync_rows::table
        .find((tbl, pk))
        .select((sync_rows::hlc, sync_rows::origin, sync_rows::deleted))
        .first(conn)
        .optional()?;
    match row {
        None => Ok(None),
        Some((hlc, origin, deleted)) => {
            let vv = vv_of(conn, tbl, pk)?;
            Ok(Some(LocalVersion { hlc, origin, deleted: deleted != 0, vv }))
        }
    }
}

/// Takes the next local seq.
fn next_seq(conn: &mut Conn) -> Result<i64> {
    Ok(diesel::update(sync_state::table.find(1))
        .set(sync_state::seq.eq(sync_state::seq + 1))
        .returning(sync_state::seq)
        .get_result(conn)?)
}

/// How far ahead of this device's own clock a peer's may pull it along: a
/// peer with a clock years ahead must not pin every device's clock there.
pub const MAX_CLOCK_LEAD_MS: i64 = 24 * 3600 * 1000;

/// Moves this device's clock past a time it has seen, so its next change is
/// ordered after it (the hybrid logical clock's receive rule) — but never
/// more than [`MAX_CLOCK_LEAD_MS`] ahead of its own wall clock.
fn observe_hlc(conn: &mut Conn, hlc: &str) -> Result<()> {
    let (ms, counter) = parse_hlc(hlc)?;
    let limit = wall_ms() + MAX_CLOCK_LEAD_MS;
    let (ms, counter) = if ms > limit { (limit, 0) } else { (ms, counter) };
    diesel::sql_query(
        "UPDATE sync_state SET
           hlc_counter = CASE WHEN ?1 > hlc_ms THEN ?2 WHEN ?1 = hlc_ms THEN max(hlc_counter, ?2) ELSE hlc_counter END,
           hlc_ms = max(hlc_ms, ?1)
         WHERE id = 1",
    )
    .bind::<BigInt, _>(ms)
    .bind::<BigInt, _>(counter)
    .execute(conn)?;
    Ok(())
}

/// "<13 digits>.<6 digits>": the only shape that sorts correctly as text.
pub(crate) fn parse_hlc(hlc: &str) -> Result<(i64, i64)> {
    let bad = || Error::msg(format!("bad clock value {hlc:?}"));
    let (ms, counter) = hlc.split_once('.').ok_or_else(bad)?;
    if ms.len() != 13 || counter.len() != 6 || !ms.bytes().chain(counter.bytes()).all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    Ok((ms.parse().map_err(|_| bad())?, counter.parse().map_err(|_| bad())?))
}

fn wall_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[derive(QueryableByName)]
struct Hlc {
    #[diesel(sql_type = Text)]
    hlc: String,
}

/// A fresh local clock value, as the triggers make them.
fn tick(conn: &mut Conn) -> Result<String> {
    Ok(diesel::sql_query(format!(
        "UPDATE sync_state SET {} WHERE id = 1 RETURNING printf('%013d.%06d', hlc_ms, hlc_counter) AS hlc",
        tick_sql()
    ))
    .get_result::<Hlc>(conn)?
    .hlc)
}

fn device_id(conn: &mut Conn) -> Result<String> {
    Ok(super::device_id(conn)?.to_string())
}

fn set_vv(conn: &mut Conn, tbl: &str, pk: &str, vv: &BTreeMap<String, i64>) -> Result<()> {
    diesel::delete(sync_vv::table.filter(sync_vv::tbl.eq(tbl)).filter(sync_vv::pk.eq(pk))).execute(conn)?;
    for (device, counter) in vv {
        diesel::insert_into(sync_vv::table)
            .values((sync_vv::tbl.eq(tbl), sync_vv::pk.eq(pk), sync_vv::device.eq(device), sync_vv::counter.eq(counter)))
            .execute(conn)?;
    }
    Ok(())
}

/// Records a version as this device's current one for a row.
#[allow(clippy::too_many_arguments)]
fn record(
    conn: &mut Conn,
    tbl: &str,
    pk: &str,
    hlc: &str,
    origin: &str,
    deleted: bool,
    vv: &BTreeMap<String, i64>,
    from_peer: Option<&str>,
) -> Result<()> {
    let seq = next_seq(conn)?;
    diesel::insert_into(sync_rows::table)
        .values((
            sync_rows::tbl.eq(tbl),
            sync_rows::pk.eq(pk),
            sync_rows::hlc.eq(hlc),
            sync_rows::origin.eq(origin),
            sync_rows::deleted.eq(i64::from(deleted)),
            sync_rows::seq.eq(seq),
            sync_rows::from_peer.eq(from_peer),
        ))
        .on_conflict((sync_rows::tbl, sync_rows::pk))
        .do_update()
        .set((
            sync_rows::hlc.eq(hlc),
            sync_rows::origin.eq(origin),
            sync_rows::deleted.eq(i64::from(deleted)),
            sync_rows::seq.eq(seq),
            sync_rows::from_peer.eq(from_peer),
        ))
        .execute(conn)?;
    set_vv(conn, tbl, pk, vv)
}

fn set_applying(conn: &mut Conn, applying: bool) -> Result<()> {
    diesel::update(sync_state::table.find(1)).set(sync_state::applying.eq(i64::from(applying))).execute(conn)?;
    Ok(())
}

/// Deletes a row while a batch is being applied, with the triggers on: what
/// the app's own triggers take along with it (a word's review history)
/// is then recorded as this device's deletions, which travel to the peers
/// like any other. The row's own version is whatever the triggers wrote
/// unless the caller records another. True when there was a row.
fn delete_tracked(conn: &mut Conn, t: &TableInfo, key: &str) -> Result<bool> {
    let values = key_values(t, key)?;
    set_applying(conn, false)?;
    let mut q = query(format!("DELETE FROM {} WHERE {}", quote(&t.name), where_key(t)));
    for v in &values {
        q = bind(q, v);
    }
    let deleted = q.execute(conn);
    set_applying(conn, true)?;
    Ok(deleted? > 0)
}

/// Deletes a row on this device's own authority (a unique-column merge),
/// with a version that supersedes `seen`, so the deletion travels to the
/// peers and wins there.
fn delete_locally(conn: &mut Conn, t: &TableInfo, key: &str, seen: &BTreeMap<String, i64>) -> Result<()> {
    // The row's vector first takes in `seen`: the triggers' deletion below
    // then counts as having seen it.
    let local = vv_of(conn, &t.name, key)?;
    set_vv(conn, &t.name, key, &merge_vv(&local, seen))?;
    if delete_tracked(conn, t, key)? {
        return Ok(());
    }
    // Not here at all (an incoming row that lost before it was written):
    // its tombstone is recorded by hand, as the triggers would.
    let me = device_id(conn)?;
    let hlc = tick(conn)?;
    let (ms, counter) = parse_hlc(&hlc)?;
    let mut vv = merge_vv(&local, seen);
    let e = vv.entry(me.clone()).or_insert(0);
    *e = (*e + 1).max(ms * 1_000_000 + counter);
    record(conn, &t.name, key, &hlc, &me, true, &vv, None)
}

fn write_row(conn: &mut Conn, t: &TableInfo, data: &[(String, Value)]) -> Result<()> {
    let cols: Vec<String> = data.iter().map(|(c, _)| quote(c)).collect();
    let updates: Vec<String> =
        data.iter().filter(|(c, _)| !t.key.contains(c)).map(|(c, _)| format!("{0} = excluded.{0}", quote(c))).collect();
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO {}",
        quote(&t.name),
        cols.join(", "),
        vec!["?"; cols.len()].join(", "),
        t.key.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", "),
        if updates.is_empty() { "NOTHING".to_string() } else { format!("UPDATE SET {}", updates.join(", ")) },
    );
    let mut q = query(sql);
    for (_, v) in data {
        q = bind(q, v);
    }
    q.execute(conn)?;
    Ok(())
}

/// Another row holding the same values as `data` in one of the table's
/// unique indexes, if there is one: its key.
fn unique_clash(conn: &mut Conn, t: &TableInfo, key: &str, data: &[(String, Value)]) -> Result<Option<String>> {
    let own_key = key_values(t, key)?;
    for cols in &t.uniques {
        let values: Vec<&Value> = cols.iter().filter_map(|c| value_of(data, c)).collect();
        // A NULL never clashes in a unique index.
        if values.len() != cols.len() || values.iter().any(|v| matches!(v, Value::Null)) {
            continue;
        }
        let cond: Vec<String> = cols.iter().map(|c| format!("{} = ?", quote(c))).collect();
        let sql = format!(
            "SELECT {} AS k FROM {} WHERE {} AND NOT ({}) LIMIT 1",
            key_sql(&t.key, ""),
            quote(&t.name),
            cond.join(" AND "),
            where_key(t),
        );
        let mut q = query(sql);
        for v in &values {
            q = bind(q, v);
        }
        for v in &own_key {
            q = bind(q, v);
        }
        if let Some(other) = q.load::<KeyText>(conn)?.into_iter().next() {
            return Ok(Some(other.k));
        }
    }
    Ok(None)
}

/// Writes `data` as row `key`, settling a clash on a unique column by keeping
/// the lower key. False when the incoming row lost and wasn't written.
fn write_settling_clashes(
    conn: &mut Conn,
    t: &TableInfo,
    key: &str,
    data: &[(String, Value)],
    incoming_vv: &BTreeMap<String, i64>,
    stats: &mut ApplyStats,
) -> Result<bool> {
    while let Some(other) = unique_clash(conn, t, key, data)? {
        stats.unique_merges += 1;
        if other.as_str() < key {
            // The existing row keeps its place; the incoming one is deleted
            // everywhere, by a version that has seen it.
            delete_locally(conn, t, key, incoming_vv)?;
            return Ok(false);
        }
        let seen = vv_of(conn, &t.name, &other)?;
        delete_locally(conn, t, &other, &seen)?;
    }
    write_row(conn, t, data)?;
    Ok(true)
}

fn apply_one(conn: &mut Conn, tables: &BTreeMap<String, TableInfo>, from_peer: &str, c: &Change) -> Result<ApplyStats> {
    let mut stats = ApplyStats::default();
    let t = tables.get(&c.tbl).ok_or_else(|| Error::msg(format!("not a synced table: {:?}", c.tbl)))?;
    parse_hlc(&c.hlc)?;
    // Revocations only ever grow: a deletion of one — which no device of
    // this app makes — is ignored, however new it claims to be.
    if t.name == "vault_revocations" && c.deleted {
        return Ok(ApplyStats { skipped: 1, ..Default::default() });
    }
    if let Some(data) = &c.data {
        // Only this table's own columns, all of them, and the key must match.
        let mut got: Vec<&str> = data.iter().map(|(k, _)| k.as_str()).collect();
        got.sort_unstable();
        let mut want: Vec<&str> = t.columns.iter().map(String::as_str).collect();
        want.sort_unstable();
        if got != want {
            return Err(Error::msg(format!("{}: columns {:?} don't match this schema {:?}", t.name, got, want)));
        }
        if key_of(t, data)? != c.pk {
            return Err(Error::msg(format!("{}: row key doesn't match its version's", t.name)));
        }
    } else if !c.deleted {
        return Err(Error::msg(format!("{} {}: a live version without data", t.name, c.pk)));
    }
    observe_hlc(conn, &c.hlc)?;

    let local = local_version(conn, &c.tbl, &c.pk)?;
    let order = local.as_ref().map_or(Order::Newer, |l| compare_vv(&c.vv, &l.vv));
    match (order, local) {
        (Order::Equal | Order::Older, _) => {
            stats.skipped += 1;
        }
        (Order::Newer, _) | (Order::Concurrent, None) => {
            if c.deleted {
                delete_tracked(conn, t, &c.pk)?;
            } else if !write_settling_clashes(conn, t, &c.pk, c.data.as_deref().unwrap_or(&[]), &c.vv, &mut stats)? {
                return Ok(stats);
            }
            record(conn, &c.tbl, &c.pk, &c.hlc, &c.origin, c.deleted, &c.vv, Some(from_peer))?;
            stats.applied += 1;
        }
        (Order::Concurrent, Some(l)) => {
            stats.conflicts += 1;
            let incoming_wins = (c.hlc.as_str(), c.origin.as_str()) > (l.hlc.as_str(), l.origin.as_str());
            let merged = merge_vv(&c.vv, &l.vv);
            let (hlc, origin, deleted) = if incoming_wins {
                if c.deleted {
                    delete_tracked(conn, t, &c.pk)?;
                } else if !write_settling_clashes(conn, t, &c.pk, c.data.as_deref().unwrap_or(&[]), &merged, &mut stats)? {
                    return Ok(stats);
                }
                (c.hlc.as_str(), c.origin.as_str(), c.deleted)
            } else {
                (l.hlc.as_str(), l.origin.as_str(), l.deleted)
            };
            // The merged vector is a state neither side had: send it on, to
            // the peer it came from as well.
            record(conn, &c.tbl, &c.pk, hlc, origin, deleted, &merged, None)?;
        }
    }
    Ok(stats)
}

fn foreign_keys_on(conn: &mut Conn) -> Result<bool> {
    #[derive(QueryableByName)]
    struct ForeignKeys {
        #[diesel(sql_type = BigInt)]
        foreign_keys: i64,
    }
    Ok(diesel::sql_query("PRAGMA foreign_keys").get_result::<ForeignKeys>(conn)?.foreign_keys == 1)
}

/// Runs `f` with foreign keys off (a child may arrive before its parent's
/// newest version), turning them back on afterwards if they were. SQLite
/// ignores the pragma inside a transaction, so this must not be called in one.
fn without_foreign_keys<T>(conn: &mut Conn, f: impl FnOnce(&mut Conn) -> Result<T>) -> Result<T> {
    let was_on = foreign_keys_on(conn)?;
    conn.batch_execute("PRAGMA foreign_keys = OFF")?;
    let result = f(conn);
    if was_on {
        conn.batch_execute("PRAGMA foreign_keys = ON")?;
    }
    result
}

/// Takes in a batch pulled from `peer` in one transaction, and records that
/// everything up to `batch.upto` has been pulled — both or neither. Foreign
/// keys are off and the triggers stand aside (`applying`) while it runs;
/// both are as they were afterwards, whatever happens.
pub fn apply(conn: &mut Conn, peer: &str, batch: &Batch) -> Result<ApplyStats> {
    apply_part(conn, peer, &batch.changes, Some(batch.upto))
}

/// Takes in part of a batch in one transaction, and with the last part
/// (`upto`) records how far it has been pulled. A big batch is taken in
/// parts, each in a transaction of its own, so the database is free for the
/// screens between them (session.rs): a pull cut off between parts records
/// nothing as pulled, and the parts already in are found equal next time.
pub fn apply_part(conn: &mut Conn, peer: &str, changes: &[Change], upto: Option<i64>) -> Result<ApplyStats> {
    without_foreign_keys(conn, |conn| conn.immediate_transaction(|tx| apply_in(tx, peer, changes, upto)))
}

fn apply_in(conn: &mut Conn, peer: &str, changes: &[Change], upto: Option<i64>) -> Result<ApplyStats> {
    set_applying(conn, true)?;
    let tables = synced_tables(conn)?;
    let mut stats = ApplyStats::default();
    for c in changes {
        stats.add(apply_one(conn, &tables, peer, c).map_err(|e| Error::msg(format!("{} {}: {e}", c.tbl, c.pk)))?);
    }
    if let Some(upto) = upto {
        diesel::insert_into(sync_peers::table)
            .values((sync_peers::device_id.eq(peer), sync_peers::last_pulled_seq.eq(upto)))
            .on_conflict(sync_peers::device_id)
            .do_update()
            .set(sync_peers::last_pulled_seq.eq(diesel::dsl::sql::<BigInt>("max(last_pulled_seq, excluded.last_pulled_seq)")))
            .execute(conn)?;
    }
    set_applying(conn, false)?;
    Ok(stats)
}

/// Splits a batch's changes into parts of at most `rows` changes and about
/// `bytes` of data (a part holds at least one change), for [`apply_part`].
pub fn parts(changes: Vec<Change>, rows: usize, bytes: usize) -> Vec<Vec<Change>> {
    let mut out: Vec<Vec<Change>> = Vec::new();
    let mut size = 0;
    for change in changes {
        let this = data_size(&change.data);
        match out.last_mut() {
            Some(part) if part.len() < rows && size + this <= bytes => {
                size += this;
                part.push(change);
            }
            _ => {
                size = this;
                out.push(vec![change]);
            }
        }
    }
    out
}

/// How far this device has pulled from `peer`.
pub fn pulled_from(conn: &mut Conn, peer: &str) -> Result<i64> {
    Ok(sync_peers::table.find(peer).select(sync_peers::last_pulled_seq).first(conn).optional()?.unwrap_or(0))
}

/// Pulls everything `from` has that `into` hasn't, batch by batch. Used by
/// the tests directly and, later, by the transport over the wire.
pub fn pull_all(into: &mut Conn, from: &mut Conn, batch_rows: i64, batch_bytes: usize) -> Result<ApplyStats> {
    let into_id = local_state(into)?.device_id;
    let from_id = local_state(from)?.device_id;
    let mut stats = ApplyStats::default();
    loop {
        let since = pulled_from(into, &from_id)?;
        let batch = export(from, since, &into_id, batch_rows, batch_bytes)?;
        stats.add(apply(into, &from_id, &batch)?);
        if batch.done {
            return Ok(stats);
        }
    }
}

// ---------------------------------------------------------------------------
// After a sync
// ---------------------------------------------------------------------------

/// Settles rows whose parent was deleted on another device while they were
/// being added or changed here, as the parent's deletion would have
/// (`ON DELETE CASCADE` removes them, `SET NULL` clears the reference).
/// Done as ordinary local writes, so they sync. Returns how many rows.
///
/// Only a parent known to be deleted (a tombstone here) counts. A parent
/// that simply hasn't arrived yet — a pull cut off between a child and its
/// parent's newest version — is not a deletion, and treating it as one
/// would delete the child on every device.
pub fn remove_orphans(conn: &mut Conn) -> Result<usize> {
    let tables = synced_tables(conn)?;
    let mut total = 0;
    loop {
        let mut removed = 0;
        for t in tables.values() {
            let orphaned = |col: &str, parent: &str, parent_col: &str| {
                format!(
                    "{col} IS NOT NULL
                       AND NOT EXISTS (SELECT 1 FROM {p} WHERE {p}.{pc} = {t}.{col})
                       AND EXISTS (SELECT 1 FROM sync_rows r WHERE r.tbl = ? AND r.pk = {t}.{col} AND r.deleted = 1)",
                    t = quote(&t.name),
                    col = quote(col),
                    p = quote(parent),
                    pc = quote(parent_col),
                )
            };
            for (col, parent, parent_col) in &t.cascades {
                removed += diesel::sql_query(format!("DELETE FROM {} WHERE {}", quote(&t.name), orphaned(col, parent, parent_col)))
                    .bind::<Text, _>(parent)
                    .execute(conn)?;
            }
            for (col, parent, parent_col) in &t.set_nulls {
                removed += diesel::sql_query(format!(
                    "UPDATE {} SET {} = NULL WHERE {}",
                    quote(&t.name),
                    quote(col),
                    orphaned(col, parent, parent_col)
                ))
                .bind::<Text, _>(parent)
                .execute(conn)?;
            }
        }
        total += removed;
        // Removing a row can orphan its own children: until nothing moves.
        if removed == 0 {
            return Ok(total);
        }
    }
}

/// A fingerprint of everything a device holds, table by table. Two devices
/// whose digests are equal hold the same rows with the same values and the
/// same versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableDigest {
    /// Rows in the table.
    pub rows: i64,
    /// Live versions recorded for it; equal to `rows` unless tracking broke.
    pub tracked: i64,
    /// SHA-256 over every row's values and version, in key order.
    pub hash: String,
}

pub type Digest = BTreeMap<String, TableDigest>;

pub fn digest(conn: &mut Conn) -> Result<Digest> {
    conn.transaction(|conn| {
        let mut out = BTreeMap::new();
        for t in synced_tables(conn)?.values() {
            out.insert(t.name.clone(), table_digest_in(conn, t)?);
        }
        Ok(out)
    })
}

/// The synced tables' names, for a digest taken a table at a time.
pub fn digest_tables(conn: &mut Conn) -> Result<Vec<String>> {
    Ok(synced_tables(conn)?.into_keys().collect())
}

/// One table's part of [`digest`], in a read transaction of its own: a
/// sync takes the digest a table at a time, so the screens are not held up
/// for the whole database (each table is compared on its own anyway).
/// `None` if the table is no longer synced.
pub fn table_digest(conn: &mut Conn, table: &str) -> Result<Option<TableDigest>> {
    conn.transaction(|conn| match synced_tables(conn)?.get(table) {
        Some(t) => Ok(Some(table_digest_in(conn, t)?)),
        None => Ok(None),
    })
}

fn hash_value(h: &mut Sha256, v: &Value) {
    match v {
        Value::Null => h.update([0]),
        Value::Int(n) => {
            h.update([1]);
            h.update(n.to_be_bytes());
        }
        Value::Real(f) => {
            h.update([2]);
            h.update(f.to_bits().to_be_bytes());
        }
        Value::Text(s) => {
            h.update([3]);
            h.update(crate::convert::bytes(s.len()).to_be_bytes());
            h.update(s.as_bytes());
        }
        Value::Blob(b) => {
            h.update([4]);
            h.update(crate::convert::bytes(b.len()).to_be_bytes());
            h.update(b);
        }
    }
}

fn table_digest_in(conn: &mut Conn, t: &TableInfo) -> Result<TableDigest> {
    {
        let versions: HashMap<String, (String, String)> = sync_rows::table
            .filter(sync_rows::tbl.eq(&t.name))
            .filter(sync_rows::deleted.eq(0))
            .select((sync_rows::pk, sync_rows::hlc, sync_rows::origin))
            .load::<(String, String, String)>(conn)?
            .into_iter()
            .map(|(pk, h, o)| (pk, (h, o)))
            .collect();
        let tombstones: Vec<(String, String, String)> = sync_rows::table
            .filter(sync_rows::tbl.eq(&t.name))
            .filter(sync_rows::deleted.eq(1))
            .order(sync_rows::pk)
            .select((sync_rows::pk, sync_rows::hlc, sync_rows::origin))
            .load(conn)?;
        let mut cols: Vec<&String> = t.columns.iter().collect();
        cols.sort();
        // A blob is represented by its length: hashing every song on every
        // sync would read the whole library each time. Files are never
        // changed in place (a new picture is a new file), and the rows
        // around them are compared by their versions.
        let select: Vec<String> = cols
            .iter()
            .map(|c| if t.blob_columns.contains(c) { format!("length({0}) AS {0}", quote(c)) } else { quote(c) })
            .collect();
        let sql = format!(
            "SELECT {} FROM {} ORDER BY {}",
            select.join(", "),
            quote(&t.name),
            t.key.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ")
        );
        let rows = diesel::sql_query(sql).load::<AnyRow>(conn)?;
        let mut h = Sha256::new();
        let mut count = 0i64;
        for row in rows {
            count += 1;
            let key = key_of(t, &row.0)?;
            h.update(key.as_bytes());
            if let Some((hlc, origin)) = versions.get(&key) {
                h.update(hlc.as_bytes());
                h.update(origin.as_bytes());
            }
            for (column, value) in &row.0 {
                h.update(column.as_bytes());
                hash_value(&mut h, value);
            }
        }
        for (pk, hlc, origin) in &tombstones {
            h.update(b"\x00deleted");
            h.update(pk.as_bytes());
            h.update(hlc.as_bytes());
            h.update(origin.as_bytes());
        }
        let hash = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        Ok(TableDigest { rows: count, tracked: crate::convert::count(versions.len()), hash })
    }
}

/// The tables where two digests differ, and any where tracking is off.
pub fn digest_differences(a: &Digest, b: &Digest) -> Vec<String> {
    let mut out = Vec::new();
    for name in a.keys().chain(b.keys()) {
        if out.contains(name) {
            continue;
        }
        match (a.get(name), b.get(name)) {
            (Some(x), Some(y)) if x == y && x.rows == x.tracked => {}
            _ => out.push(name.clone()),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Joining a vault
// ---------------------------------------------------------------------------

/// What a fresh install holds that isn't the learner's, so joining a vault
/// may replace it without asking: (table, the SQL condition for such rows).
///
/// The rule: settings and the vault's own lists never count (a fresh
/// install has preferences, and API keys are re-shared by the vault), and
/// neither does the demo course as it was installed — its course row, its
/// built-in lessons, and their words, sentences, lines, scene, story and
/// course links, and its episode while unrecorded.
/// Everything else does: any course, lesson or word the learner or the
/// tutor made, and any practice at all — of the demo too, since that lives
/// in reviews, the XP log and the day log, which always count.
fn join_exempt(table: &str) -> Option<String> {
    let slug = crate::demo_course::SLUG.replace('\'', "''");
    let course = format!("SELECT id FROM courses WHERE slug = '{slug}'");
    let lessons = format!(
        "SELECT l.id FROM lessons l JOIN course_lessons cl ON cl.lesson_id = l.id
         WHERE l.origin = 'starter' AND cl.course_id IN ({course})"
    );
    Some(match table {
        "settings" | "vault_devices" | "vault_revocations" => "1".to_string(),
        "courses" => format!("id IN ({course})"),
        "lessons" => format!("id IN ({lessons})"),
        "words" | "sentences" | "conversation_lines" => format!("lesson_id IN ({lessons})"),
        "course_lessons" => format!("course_id IN ({course}) AND lesson_id IN ({lessons})"),
        "scenes" | "scene_spots" | "stories" | "story_panels" => format!("lesson_id IN ({lessons})"),
        // Its episode, while it is still the unrecorded script it came with.
        "songs" => format!("status = 'draft' AND id IN (SELECT song_id FROM lessons WHERE id IN ({lessons}))"),
        _ => return None,
    })
}

/// Whether this device has nothing of its own to lose by joining a vault —
/// the condition for joining without asking. Returns the tables holding
/// data if not.
pub fn data_tables(conn: &mut Conn) -> Result<Vec<String>> {
    Ok(held_data(conn)?.into_iter().map(|(t, _)| t).collect())
}

/// The tables holding the learner's data here, and how many rows each.
pub fn held_data(conn: &mut Conn) -> Result<Vec<(String, i64)>> {
    let tables = synced_tables(conn)?;
    let mut with_data = Vec::new();
    for t in tables.keys() {
        let filter = join_exempt(t).map_or_else(String::new, |exempt| format!(" WHERE NOT ({exempt})"));
        let n = diesel::sql_query(format!("SELECT count(*) AS n FROM {}{filter}", quote(t))).get_result::<Count>(conn)?.n;
        if n > 0 {
            with_data.push((t.clone(), n));
        }
    }
    Ok(with_data)
}

/// Empties every synced table and the sync bookkeeping, and gives this device
/// the id `new_id` and the vault's — ready to receive the vault's data.
/// Refuses if any of the learner's data is here, checked in the same
/// transaction that empties it, so nothing written meanwhile can be lost —
/// unless `replace`, for which the caller has saved a copy first.
pub fn reset_for_join(
    conn: &mut Conn,
    vault_id: &str,
    new_id: DeviceId,
    keys: Option<(&[u8], &[u8])>,
    replace: bool,
) -> Result<()> {
    without_foreign_keys(conn, |conn| {
        conn.immediate_transaction(|conn| {
            let held = data_tables(conn)?;
            if !held.is_empty() && !replace {
                return Err(Error::msg(format!(
                    "this device already holds data ({}); joining needs an empty one",
                    held.join(", ")
                )));
            }
            set_applying(conn, true)?;
            for t in synced_tables(conn)?.keys() {
                conn.batch_execute(&format!("DELETE FROM {}", quote(t)))?;
            }
            // The epochs too: they mark this device's old history, which a
            // new id in a new vault never had.
            conn.batch_execute("DELETE FROM sync_rows; DELETE FROM sync_vv; DELETE FROM sync_peers; DELETE FROM sync_epochs;")?;
            diesel::update(sync_state::table.find(1))
                .set((
                    sync_state::device_id.eq(new_id),
                    sync_state::vault_id.eq(vault_id),
                    sync_state::seq.eq(0),
                    sync_state::applying.eq(0),
                ))
                .execute(conn)?;
            if let Some((secret, public)) = keys {
                diesel::update(sync_state::table.find(1))
                    .set((sync_state::secret_key.eq(secret), sync_state::public_key.eq(public)))
                    .execute(conn)?;
            }
            Ok(())
        })
    })
}
