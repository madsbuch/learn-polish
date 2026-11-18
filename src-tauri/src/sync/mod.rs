//! Sync between the learner's devices (docs/SYNC.md): Tally's engine, ported
//! to Diesel and fitted to this schema.
//!
//! - [`triggers`]: the triggers that record every local write to a synced
//!   table, made from the schema itself.
//! - [`engine`]: what one device sends another and how it takes in what it
//!   is sent. No networking: the transport (a later step) only carries its
//!   [`engine::Batch`]es.
//!
//! The tables that never sync are [`triggers::LOCAL_TABLES`]; everything else
//! does, and a row there means the same on every device.

use diesel::prelude::*;

use crate::db::Conn;
use crate::error::Result;
use crate::ids::DeviceId;
use crate::schema::{sync_rows, sync_state};

// Called by the transport and the screens, which come next; the tests use
// all of it now.
#[allow(dead_code)]
pub mod engine;
pub mod triggers;
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
