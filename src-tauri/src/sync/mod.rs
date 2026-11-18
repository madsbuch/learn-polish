//! Sync between the learner's devices (docs/SYNC.md): Tally's engine, ported
//! to Diesel and fitted to this schema.
//!
//! - [`triggers`]: the triggers that record every local write to a synced
//!   table, made from the schema itself.
//! - [`engine`]: what one device sends another and how it takes in what it
//!   is sent. No networking: the transport only carries its
//!   [`engine::Batch`]es.
//! - [`wire`]: the encrypted channel between two devices (Noise over TCP).
//! - [`discovery`]: finding the vault's other devices on the local network.
//! - [`session`]: one sync between two devices, pairing included.
//! - [`instance`]: noticing a database restored from an older copy.
//! - [`service`]: the app's side — the loop, the commands, the events.
//! - [`background`]: one sync with the app closed (Android's background job).
//! - [`app`]: the names and ports that make this app's sync its own.
//!
//! The tables that never sync are [`triggers::LOCAL_TABLES`]; everything else
//! does, and a row there means the same on every device.

use diesel::prelude::*;

use crate::db::{Conn, Db};
use crate::error::{Error, Result};
use crate::ids::DeviceId;
use crate::schema::{sync_rows, sync_state};

pub mod app;
pub mod background;
pub mod discovery;
// A few of its functions are only for the tests (pull_all, data_tables).
#[allow(dead_code)]
pub mod engine;
pub mod instance;
pub mod service;
pub mod session;
pub mod triggers;
pub mod wire;
#[cfg(test)]
mod tests;

/// Joins the columns of a key made of several (`day_log`'s day and device)
/// into one text key: the unit separator, which no key contains.
pub const KEY_SEPARATOR: char = '\u{1f}';

/// This device's id (the one `day_log` rows and sync versions carry).
pub fn device_id(conn: &mut Conn) -> Result<DeviceId> {
    Ok(sync_state::table.find(1).select(sync_state::device_id).first(conn)?)
}

/// Whether this device wrote the current version of a row — the rule for
/// background work that resumes or polls at startup (a song Suno is making,
/// a podcast's audio, a reply the tutor was writing, missing pictures): only
/// the device that started it carries on, or both would make it and the
/// learner would get two copies.
///
/// A row sync has no record of was written here (nothing else could have
/// made it); so is anything when the bookkeeping cannot be read, which keeps
/// a single device doing exactly what it did before sync existed.
pub fn written_here(conn: &mut Conn, table: &str, key: &str) -> bool {
    let origin: Option<String> = match sync_rows::table.find((table, key)).select(sync_rows::origin).first(conn).optional() {
        Ok(origin) => origin,
        Err(_) => return true,
    };
    match (origin, device_id(conn)) {
        (Some(origin), Ok(me)) => origin == me.to_string(),
        _ => true,
    }
}

/// Makes sure this database has its sync bookkeeping: a device id (made
/// once, at random, the first time) and current triggers on every synced
/// table. Run at every open, after the schema.
pub(crate) fn init(conn: &mut Conn) -> Result<()> {
    ensure_state(conn)?;
    triggers::ensure(conn)
}

/// The single `sync_state` row, with a fresh device id if there is none yet.
pub(crate) fn ensure_state(conn: &mut Conn) -> Result<()> {
    diesel::insert_or_ignore_into(sync_state::table)
        .values((sync_state::id.eq(1), sync_state::device_id.eq(DeviceId::new())))
        .execute(conn)?;
    Ok(())
}

/// Where sync finds the database: the app's own (through its handle) or, in
/// the tests, one per device. Sync holds the database's lock only for one
/// engine call at a time — never across the network — so the screens stay
/// responsive while a sync runs.
pub trait Store: Clone + Send + Sync + 'static {
    fn db(&self) -> &Db;
}

impl Store for std::sync::Arc<Db> {
    fn db(&self) -> &Db {
        self
    }
}

impl<R: tauri::Runtime> Store for tauri::AppHandle<R> {
    fn db(&self) -> &Db {
        use tauri::Manager;
        self.state::<Db>().inner()
    }
}

/// Runs `f` on the database, on a thread that may block: waiting for the
/// lock (a screen's command holding it) must not stall the async runtime
/// the network runs on.
pub(crate) async fn with<S: Store, T: Send + 'static>(
    store: &S,
    f: impl FnOnce(&mut Conn) -> Result<T> + Send + 'static,
) -> Result<T> {
    let store = store.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = store.db().lock();
        f(&mut conn)
    })
    .await
    .map_err(|e| Error::msg(format!("sync stopped: {e}")))?
}

/// A new vault's id: 128 random bits, as hex. Never shown, never typed.
pub fn new_vault_id() -> String {
    hex::encode(rand::random::<[u8; 16]>())
}

/// A random marker (instance.rs, epochs): 128 bits, as hex.
pub fn new_marker() -> String {
    hex::encode(rand::random::<[u8; 16]>())
}

/// Now, as sync writes its timestamps (`vault_devices.added_at`,
/// `sync_peers.last_sync_at`, …): UTC, to the millisecond, ISO 8601 — what
/// the screens read with `Date.parse`.
pub fn now_iso() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    iso(ms)
}

/// Milliseconds since 1970 as "YYYY-MM-DDTHH:MM:SS.mmmZ".
pub fn iso(ms: i64) -> String {
    let (y, m, d) = crate::commands::civil_date(ms.div_euclid(86_400_000));
    let rem = ms.rem_euclid(86_400_000);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        rem % 3_600_000 / 60_000,
        rem % 60_000 / 1000,
        rem % 1000
    )
}
