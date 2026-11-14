//! Files: every file the app keeps — speech clips, song audio, cover art —
//! is a row in the `files` table, bytes and all, and other tables point at
//! it by id. The database alone is then a complete backup.
//!
//! Some players need a real file (the webview's asset protocol, the native
//! player, a Sonos speaker streaming over the network), so a file can also
//! have a copy on disk, `path`. That copy is only a cache: it is written from
//! the bytes when missing, and deleted with the row.

use std::path::{Path, PathBuf};

use diesel::prelude::*;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
use crate::schema::files;

pub const SPEECH: &str = "speech";
pub const SONG_AUDIO: &str = "song_audio";
pub const SONG_IMAGE: &str = "song_image";

/// A file's details, without its bytes.
#[allow(dead_code)] // every column, though not every caller reads each
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = files, check_for_backend(diesel::sqlite::Sqlite))]
pub struct FileInfo {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub path: Option<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Stores a file and returns its id.
pub fn store(conn: &mut Conn, kind: &str, name: &str, mime: &str, bytes: &[u8]) -> Result<i64> {
    Ok(diesel::insert_into(files::table)
        .values((
            files::kind.eq(kind),
            files::name.eq(name),
            files::mime.eq(mime),
            files::size.eq(bytes.len() as i64),
            files::sha256.eq(sha256_hex(bytes)),
            files::bytes.eq(bytes),
            files::created_at.eq(db::now_secs()),
        ))
        .returning(files::id)
        .get_result(conn)?)
}

pub fn info(conn: &mut Conn, id: i64) -> Result<FileInfo> {
    files::table
        .find(id)
        .select(FileInfo::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg("That file no longer exists."))
}

/// A file's type and contents.
pub fn read(conn: &mut Conn, id: i64) -> Result<(String, Vec<u8>)> {
    files::table
        .find(id)
        .select((files::mime, files::bytes))
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg("That file no longer exists."))
}

/// Where each file's copy on disk is, for those that have one.
pub fn paths(conn: &mut Conn, ids: &[i64]) -> Result<std::collections::HashMap<i64, String>> {
    Ok(files::table
        .filter(files::id.eq_any(ids))
        .filter(files::path.is_not_null())
        .select((files::id, files::path.assume_not_null()))
        .load::<(i64, String)>(conn)?
        .into_iter()
        .collect())
}

/// Deletes files and their copies on disk.
pub fn delete(conn: &mut Conn, ids: &[i64]) -> Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    for (_, path) in paths(conn, ids)? {
        let _ = std::fs::remove_file(path);
    }
    let mut deleted = 0;
    for chunk in ids.chunks(500) {
        deleted += diesel::delete(files::table.filter(files::id.eq_any(chunk))).execute(conn)?;
    }
    Ok(deleted)
}

/// Writes a file's copy into `dir` unless a copy is already there, and
/// returns its path.
pub fn materialize(conn: &mut Conn, dir: &Path, id: i64) -> Result<String> {
    let file = info(conn, id)?;
    if let Some(path) = &file.path {
        if Path::new(path).exists() {
            return Ok(path.clone());
        }
    }
    let target = dir.join(&file.name);
    let (_, bytes) = read(conn, id)?;
    // Write then rename, so a crash never leaves a half file that looks done.
    let partial = target.with_extension("part");
    std::fs::write(&partial, &bytes)?;
    std::fs::rename(&partial, &target)?;
    let path = target.to_string_lossy().to_string();
    diesel::update(files::table.find(id)).set(files::path.eq(&path)).execute(conn)?;
    Ok(path)
}

/// Where the copies of song files live.
pub fn media_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| Error::msg(format!("no app data directory: {e}")))?
        .join("songs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Makes sure every song file has its copy on disk — after a restore, on a
/// new phone, or when the copies were cleared — in the background.
pub fn sync_to_disk(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let dir = media_dir(&app)?;
            let db = app.state::<Db>();
            let ids: Vec<i64> = files::table
                .filter(files::kind.eq_any([SONG_AUDIO, SONG_IMAGE]))
                .select(files::id)
                .load(&mut *db.lock())?;
            for id in ids {
                materialize(&mut db.lock(), &dir, id)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("writing song files to disk failed: {error}");
        }
    });
}

/// The extension a file's name should have, from its MIME type.
pub fn extension_for(mime: &str) -> &'static str {
    match mime {
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/ogg" => "ogg",
        "audio/mp4" | "audio/aac" => "m4a",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_stored_hashed_and_written_out_on_demand() {
        let dir = std::env::temp_dir().join("learn-polish-tests").join(format!("files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = &mut db::open(&dir.join("files.db")).unwrap();

        let id = store(conn, SONG_AUDIO, "song-1-0.mp3", "audio/mpeg", b"abc").unwrap();
        let file = info(conn, id).unwrap();
        assert_eq!((file.size, file.path), (3, None));
        let hash: String = files::table.find(id).select(files::sha256).first(conn).unwrap();
        assert_eq!(hash, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");

        let path = materialize(conn, &dir, id).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        // A copy that went missing is written again.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(materialize(conn, &dir, id).unwrap(), path);
        assert!(Path::new(&path).exists());

        assert_eq!(delete(conn, &[id]).unwrap(), 1);
        assert!(!Path::new(&path).exists(), "the copy goes with the row");
        assert!(info(conn, id).is_err());
    }
}
