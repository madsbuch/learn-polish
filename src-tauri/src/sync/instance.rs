//! Noticing a database that isn't where it was written (as Tally's).
//!
//! Sync knows a device by its id, and trusts that device's history to only
//! move forward: what a peer has pulled from it stays pulled, its clock only
//! grows. A copy of the database put back later — restored from a backup,
//! the file carried to another computer — breaks that: same id, older
//! history. Its new writes would never be pulled, and what it lost would
//! never be sent back.
//!
//! So each copy carries a random marker (the device setting
//! `sync_instance`), mirrored in a file beside the database. When the two
//! disagree, this database was written somewhere else, and it takes a new
//! device id before anything is written: to the vault it is a new device
//! that happens to hold the old one's data, and syncs from scratch. Copies
//! the app makes itself (the backups before an upgrade or a join, exports)
//! are marked as copies, so putting one back is caught too.
//!
//! A copy restored together with the file beside it (a whole folder put
//! back) passes this check; the session's epochs catch it instead, at its
//! first sync (session.rs, `check_not_stale`). Either way the vault still
//! recognises it by its key.

use std::path::{Path, PathBuf};

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::Text;

use crate::db::{self, Conn};
use crate::error::Result;
use crate::ids::DeviceId;
use crate::schema::sync_state;

/// The device setting holding this copy's marker (`sync_` keys stay on the
/// device: db.rs).
pub const MARKER: &str = "sync_instance";

/// The file beside the database that holds the marker too.
pub fn sidecar(db: &Path) -> PathBuf {
    let mut p = db.as_os_str().to_owned();
    p.push(".instance");
    PathBuf::from(p)
}

/// What [`check`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    /// The database is where it was written.
    Same,
    /// First check of this database: marked as this copy.
    Marked,
    /// A copy from elsewhere: it now has a new device id.
    Rotated { old_id: DeviceId, new_id: DeviceId },
}

/// Compares the database's marker with the file beside it, and gives the
/// database a new device id if it was written elsewhere. Run when the
/// database opens, before anything is written to it.
///
/// The file is written before the database each time: a crash between the
/// two then leaves a marker-less database beside a file, which reads as a
/// first check, never as a copy.
pub fn check(conn: &mut Conn, db_path: &Path) -> Result<Check> {
    let marker = db::get_setting(conn, MARKER)?;
    let file = std::fs::read_to_string(sidecar(db_path)).ok().map(|s| s.trim().to_string());
    match (&marker, &file) {
        (Some(m), Some(f)) if m == f => Ok(Check::Same),
        (None, _) => {
            // Never checked: a new database, or one from before sync.
            remark(conn, db_path)?;
            Ok(Check::Marked)
        }
        _ => {
            let old_id = super::device_id(conn)?;
            let new_id = rotate(conn)?;
            remark(conn, db_path)?;
            Ok(Check::Rotated { old_id, new_id })
        }
    }
}

/// A fresh marker, in the file and then in the database.
fn remark(conn: &mut Conn, db_path: &Path) -> Result<()> {
    let marker = super::new_marker();
    std::fs::write(sidecar(db_path), &marker)?;
    db::set_setting(conn, MARKER, &marker)
}

/// Gives this database a new device id, keeping its data and key. The new id
/// is added to the vault's device list as an ordinary change that syncs; the
/// old entry stays — if the original is still in use somewhere (a folder
/// copied to a second computer), it must stay listed. Returns the new id.
///
/// The marker is left alone (unlike Tally, which changes it here too): the
/// file beside the database still matches it, so the next start doesn't
/// take yet another id.
pub fn rotate(conn: &mut Conn) -> Result<DeviceId> {
    conn.immediate_transaction(|conn| {
        let (old_id, vault): (DeviceId, Option<String>) =
            sync_state::table.find(1).select((sync_state::device_id, sync_state::vault_id)).first(conn)?;
        let new_id = DeviceId::new();
        diesel::update(sync_state::table.find(1)).set(sync_state::device_id.eq(new_id)).execute(conn)?;
        if vault.is_some() {
            // Recorded by the triggers under the new id.
            diesel::sql_query(
                "INSERT OR IGNORE INTO vault_devices (id, name, public_key, platform, added_at)
                 SELECT ?, name, public_key, platform, ? FROM vault_devices WHERE id = ?",
            )
            .bind::<Text, _>(new_id.to_string())
            .bind::<Text, _>(super::now_iso())
            .bind::<Text, _>(old_id.to_string())
            .execute(conn)?;
        }
        Ok(new_id)
    })
}

#[derive(QueryableByName)]
struct Found {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    n: i64,
}

/// Marks a copy the app made of its database (a backup, an export) as a
/// copy, so a copy put back in place is noticed by [`check`]. A copy from
/// before sync (no device settings yet) needs nothing: it has no marker,
/// and gets a new id when it is upgraded anyway.
pub fn mark_copy(path: &Path) -> Result<()> {
    let mut copy = Conn::establish(&path.to_string_lossy())?;
    let tracked = diesel::sql_query("SELECT count(*) AS n FROM sqlite_master WHERE type = 'table' AND name = 'device_settings'")
        .get_result::<Found>(&mut copy)?
        .n
        > 0;
    if tracked {
        copy.batch_execute(&format!(
            "INSERT INTO device_settings (key, value) VALUES ('{MARKER}', 'copy:{}')
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            super::new_marker()
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("learn-polish-tests")
            .join(format!("instance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("learn-polish.db")
    }

    /// Opens a database file without the check `db::open` runs (as an old
    /// version of the app would, or a copy nothing looked at).
    fn open_unchecked(path: &Path) -> Conn {
        let mut conn = Conn::establish(&path.to_string_lossy()).unwrap();
        conn.batch_execute("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;").unwrap();
        conn
    }

    fn copy(conn: &mut Conn, to: &Path) {
        let _ = std::fs::remove_file(to);
        diesel::sql_query("VACUUM INTO ?").bind::<Text, _>(to.to_string_lossy().to_string()).execute(conn).unwrap();
    }

    #[test]
    fn a_database_in_place_keeps_its_id() {
        let path = scratch("in-place");
        // Opening checks it once already (db.rs).
        let mut conn = db::open(&path).unwrap();
        let id = super::super::device_id(&mut conn).unwrap();
        assert_eq!(check(&mut conn, &path).unwrap(), Check::Same);
        assert_eq!(check(&mut conn, &path).unwrap(), Check::Same);
        assert_eq!(super::super::device_id(&mut conn).unwrap(), id);
    }

    #[test]
    fn a_copy_elsewhere_takes_a_new_id_and_is_listed_under_it() {
        let path = scratch("original");
        let mut conn = db::open(&path).unwrap();
        conn.batch_execute("UPDATE sync_state SET vault_id = 'v'").unwrap();
        let id = super::super::device_id(&mut conn).unwrap();
        diesel::sql_query("INSERT INTO vault_devices (id, name, public_key, platform, added_at) VALUES (?, 'Mac', 'aa', 'macos', 'now')")
            .bind::<Text, _>(id.to_string())
            .execute(&mut conn)
            .unwrap();
        // Carried to another folder (no file beside it there).
        let other = scratch("carried");
        copy(&mut conn, &other);
        let mut copied = open_unchecked(&other);
        match check(&mut copied, &other).unwrap() {
            Check::Rotated { old_id, new_id } => {
                assert_eq!(old_id, id);
                assert_ne!(new_id, id);
                assert_eq!(super::super::device_id(&mut copied).unwrap(), new_id);
                #[derive(QueryableByName)]
                struct Key {
                    #[diesel(sql_type = Text)]
                    public_key: String,
                }
                let listed = diesel::sql_query("SELECT public_key FROM vault_devices WHERE id = ?")
                    .bind::<Text, _>(new_id.to_string())
                    .get_result::<Key>(&mut copied)
                    .unwrap();
                assert_eq!(listed.public_key, "aa", "the vault knows it by the same key");
            }
            other => panic!("{other:?}"),
        }
        // And from then on it's itself.
        assert_eq!(check(&mut copied, &other).unwrap(), Check::Same);
    }

    #[test]
    fn a_backup_put_back_is_noticed() {
        let path = scratch("put-back");
        let mut conn = db::open(&path).unwrap();
        let backup = path.with_file_name("backup.db");
        copy(&mut conn, &backup);
        mark_copy(&backup).unwrap();
        drop(conn);
        // Put back in place: same folder, same file beside it — but marked a copy.
        std::fs::rename(&backup, &path).unwrap();
        let _ = std::fs::remove_file(path.with_file_name("learn-polish.db-wal"));
        let mut restored = open_unchecked(&path);
        assert!(matches!(check(&mut restored, &path).unwrap(), Check::Rotated { .. }));
    }

    #[test]
    fn a_rotation_leaves_the_marker_matching_the_file() {
        let path = scratch("rotate");
        let mut conn = db::open(&path).unwrap();
        let id = rotate(&mut conn).unwrap();
        assert_eq!(super::super::device_id(&mut conn).unwrap(), id);
        assert_eq!(check(&mut conn, &path).unwrap(), Check::Same);
    }
}
