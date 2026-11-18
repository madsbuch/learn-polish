//! Files: every file the app keeps — speech clips, song audio, cover art —
//! is a row in the `files` table, bytes and all, and other tables point at
//! it by id. The database alone is then a complete backup.
//!
//! The webview loads every file through the `media` protocol, by id
//! (`fileUrl(id)` in the frontend; [`serve`] here): straight from this
//! table, never from a path. Sonos streams from it too (cast.rs).
//!
//! Integrity: every reference to a file is a foreign key (SQLite enforces
//! them), listed in [`referenced`]; a file nothing references is an orphan
//! and is swept at startup ([`collect_orphans`]). Where the bytes live is a
//! detail of this module — callers only ever hold a file id.
//!
//! Only Android's native player needs a real file, so song audio and art also
//! have a copy on disk, its path in `file_paths`. That copy is only a cache:
//! it is written from the bytes when missing, and deleted with the row. The
//! path is this device's alone (`file_paths` does not sync): another device
//! writes its own copy where it keeps them.

use std::path::{Path, PathBuf};

use diesel::prelude::*;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
use crate::ids::*;
use crate::schema::{file_paths, files};

pub const SPEECH: &str = "speech";
pub const SONG_AUDIO: &str = "song_audio";
pub const SONG_IMAGE: &str = "song_image";
pub const COURSE_IMAGE: &str = "course_image";
pub const WIKI_IMAGE: &str = "wiki_image";
/// A word's picture (picture lessons).
pub const WORD_IMAGE: &str = "word_image";

/// A file's details, without its bytes.
#[allow(dead_code)] // every column, though not every caller reads each
#[derive(Debug, Clone, QueryableByName)]
pub struct FileInfo {
    #[diesel(sql_type = FileIdSql)]
    pub id: FileId,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub kind: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub name: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub mime: String,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub size: i64,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    pub path: Option<String>,
}

// A file's details are read through the `files_meta` index, never the row:
// reading a column that comes after `bytes` from the row walks the whole
// file — a song's audio, megabytes — from disk. Read from the index, the
// Music tab's 30 songs took a fraction of a millisecond instead of a second.
const META: &str = "SELECT f.id AS id, f.kind AS kind, f.name AS name, f.mime AS mime, f.size AS size, p.path AS path
    FROM files f INDEXED BY files_meta LEFT JOIN file_paths p ON p.file_id = f.id";

#[derive(QueryableByName)]
struct PathRow {
    #[diesel(sql_type = FileIdSql)]
    id: FileId,
    #[diesel(sql_type = diesel::sql_types::Text)]
    path: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Stores a file and returns its id.
pub fn store(conn: &mut Conn, kind: &str, name: &str, mime: &str, bytes: &[u8]) -> Result<FileId> {
    Ok(diesel::insert_into(files::table)
        .values((
            files::id.eq(FileId::new()),
            files::kind.eq(kind),
            files::name.eq(name),
            files::mime.eq(mime),
            files::size.eq(crate::convert::count(bytes.len())),
            files::sha256.eq(sha256_hex(bytes)),
            files::bytes.eq(bytes),
            files::created_at.eq(db::now_secs()),
        ))
        .returning(files::id)
        .get_result(conn)?)
}

pub fn info(conn: &mut Conn, id: FileId) -> Result<FileInfo> {
    diesel::sql_query(format!("{META} WHERE f.id = ?"))
        .bind::<FileIdSql, _>(id)
        .get_result(conn)
        .optional()?
        .ok_or_else(|| Error::msg("That file no longer exists."))
}

/// A file's type and contents.
pub fn read(conn: &mut Conn, id: FileId) -> Result<(String, Vec<u8>)> {
    files::table
        .find(id)
        .select((files::mime, files::bytes))
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg("That file no longer exists."))
}

/// Where each file's copy on disk is, for those that have one.
pub fn paths(conn: &mut Conn, ids: &[FileId]) -> Result<std::collections::HashMap<FileId, String>> {
    let mut found = std::collections::HashMap::new();
    for chunk in ids.chunks(500) {
        found.extend(
            file_paths::table
                .filter(file_paths::file_id.eq_any(chunk))
                .select((file_paths::file_id, file_paths::path))
                .load::<(FileId, String)>(conn)?,
        );
    }
    Ok(found)
}

/// Deletes files and their copies on disk. A file still referenced where
/// that is not allowed (a take's audio) is refused, and nothing is deleted.
pub fn delete(conn: &mut Conn, ids: &[FileId]) -> Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let copies = paths(conn, ids)?;
    let deleted = conn.transaction::<_, Error, _>(|tx| {
        let mut deleted = 0;
        for chunk in ids.chunks(500) {
            deleted += diesel::delete(files::table.filter(files::id.eq_any(chunk))).execute(tx)?;
            diesel::delete(file_paths::table.filter(file_paths::file_id.eq_any(chunk))).execute(tx)?;
        }
        Ok(deleted)
    })?;
    // Only once the rows are gone: a refused delete keeps its copy.
    for path in copies.into_values() {
        let _ = std::fs::remove_file(path);
    }
    Ok(deleted)
}

/// Every file some row points at. This is the one list of the columns that
/// reference files: a new one belongs here, with a foreign key in the schema.
pub fn referenced(conn: &mut Conn) -> Result<std::collections::HashSet<FileId>> {
    use crate::schema::{song_tracks, speech_clips, courses, wiki_sources, words};
    let mut ids: std::collections::HashSet<FileId> =
        speech_clips::table.select(speech_clips::file_id).load::<FileId>(conn)?.into_iter().collect();
    let takes: Vec<(Option<FileId>, Option<FileId>)> =
        song_tracks::table.select((song_tracks::audio_file_id, song_tracks::image_file_id)).load(conn)?;
    ids.extend(takes.into_iter().flat_map(|(audio, image)| [audio, image]).flatten());
    ids.extend(courses::table.select(courses::image_file_id).load::<Option<FileId>>(conn)?.into_iter().flatten());
    ids.extend(wiki_sources::table.select(wiki_sources::image_file_id).load::<Option<FileId>>(conn)?.into_iter().flatten());
    ids.extend(words::table.select(words::image_file_id).load::<Option<FileId>>(conn)?.into_iter().flatten());
    Ok(ids)
}

/// Deletes files nothing references (and their copies on disk). Returns how
/// many went.
///
/// Only files this device stored: one from another device may simply have
/// arrived before the row that uses it, in a sync that was cut off, and
/// deleting it would delete it on every device. The device that stored it
/// collects it once the row that used it is gone there too.
pub fn collect_orphans(conn: &mut Conn) -> Result<usize> {
    let used = referenced(conn)?;
    let orphans: Vec<FileId> = files::table
        .select(files::id)
        .load::<FileId>(conn)?
        .into_iter()
        .filter(|id| !used.contains(id))
        .filter(|id| crate::sync::written_here(conn, "files", &id.to_string()))
        .collect();
    let removed = delete(conn, &orphans)?;
    // Copies on disk of files that are gone (deleted by another device).
    let stale: Vec<PathRow> = diesel::sql_query(
        "SELECT p.file_id AS id, p.path AS path FROM file_paths p WHERE NOT EXISTS (SELECT 1 FROM files f WHERE f.id = p.file_id)",
    )
    .load(conn)?;
    for row in &stale {
        let _ = std::fs::remove_file(&row.path);
        diesel::delete(file_paths::table.find(row.id)).execute(conn)?;
    }
    Ok(removed)
}

/// Writes a file's copy into `dir` unless a copy is already there, and
/// returns its path.
pub fn materialize(conn: &mut Conn, dir: &Path, id: FileId) -> Result<String> {
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
    diesel::insert_into(file_paths::table)
        .values((file_paths::file_id.eq(id), file_paths::path.eq(&path)))
        .on_conflict(file_paths::file_id)
        .do_update()
        .set(file_paths::path.eq(&path))
        .execute(conn)?;
    Ok(path)
}

/// Where the copies of song files live, for the native player.
pub fn media_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| Error::msg(format!("no app data directory: {e}")))?
        .join("songs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[diesel::declare_sql_function]
extern "SQL" {
    /// SQLite's `substr()` on a blob: `length` bytes from 1-based `start`.
    fn substr(bytes: diesel::sql_types::Binary, start: diesel::sql_types::BigInt, length: diesel::sql_types::BigInt) -> diesel::sql_types::Binary;
}

/// Part of a file's bytes, read in SQLite so a range request does not load
/// a whole song.
pub fn read_range(conn: &mut Conn, id: FileId, start: u64, length: u64) -> Result<Vec<u8>> {
    Ok(files::table
        .find(id)
        .select(substr(files::bytes, crate::convert::signed(start) + 1, crate::convert::signed(length)))
        .first(conn)?)
}

/// Part of a file on disk.
fn read_file_range(path: &str, start: u64, length: u64) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity(crate::convert::offset(length));
    file.take(length).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// What part of a file a request asks for.
#[derive(Debug, PartialEq)]
pub enum Slice {
    Whole,
    /// First and last byte, inclusive.
    Part(u64, u64),
    Unsatisfiable,
}

/// Reads `bytes=start-end` (end optional) from a Range header.
pub fn parse_range(header: &str) -> Option<(u64, Option<u64>)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    Some((start.trim().parse().ok()?, end.trim().parse().ok()))
}

pub fn slice(size: u64, range: Option<(u64, Option<u64>)>) -> Slice {
    match range {
        None => Slice::Whole,
        Some((start, end)) if start < size => Slice::Part(start, end.unwrap_or(size - 1).min(size - 1)),
        Some(_) => Slice::Unsatisfiable,
    }
}

/// Answers a `media://<id>` request from the webview: the file, or the
/// requested range of it (audio seeks this way).
pub fn serve(app: &AppHandle, request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::{header, Response, StatusCode};
    let not_found = || Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()).unwrap();
    let Some(id) = request.uri().path().trim_matches('/').split('/').next().and_then(|s| FileId::parse(s).ok())
    else {
        return not_found();
    };
    let db = app.state::<Db>();
    let Ok(file) = info(&mut db.lock(), id) else { return not_found() };
    let size = crate::convert::unsigned(file.size);
    let range = request.headers().get(header::RANGE).and_then(|v| v.to_str().ok()).and_then(parse_range);
    // Song audio and cover art have a copy on disk: read it from there, not
    // from the database, so a big file never holds every other screen up.
    let on_disk = file.path.clone().filter(|path| Path::new(path).exists());
    let response = Response::builder()
        .header(header::CONTENT_TYPE, &file.mime)
        .header(header::ACCEPT_RANGES, "bytes")
        // A file's bytes never change: a new picture or clip is a new id.
        .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    if let Some(path) = on_disk {
        let result = match slice(size, range) {
            Slice::Whole => std::fs::read(&path).ok().map(|bytes| response.status(StatusCode::OK).body(bytes)),
            Slice::Part(start, end) => read_file_range(&path, start, end - start + 1).ok().map(|bytes| {
                response
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"))
                    .body(bytes)
            }),
            Slice::Unsatisfiable => Some(
                response
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                    .body(Vec::new()),
            ),
        };
        return result.and_then(|r| r.ok()).unwrap_or_else(not_found);
    }
    let mut guard = db.lock();
    let conn = &mut *guard;
    let result = match slice(size, range) {
        Slice::Whole => read(conn, id).map(|(_, bytes)| response.status(StatusCode::OK).body(bytes)),
        Slice::Part(start, end) => read_range(conn, id, start, end - start + 1).map(|bytes| {
            response
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"))
                .body(bytes)
        }),
        Slice::Unsatisfiable => Ok(response
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{size}"))
            .body(Vec::new())),
    };
    result.ok().and_then(|r| r.ok()).unwrap_or_else(not_found)
}

/// Makes sure every song file has its copy on disk for the native player —
/// after a restore, on a new phone, or when the copies were cleared — in the
/// background.
pub fn sync_to_disk(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let dir = media_dir(&app)?;
            let db = app.state::<Db>();
            // Course pictures had copies on disk before everything went
            // through the media protocol; they are not needed.
            if let Ok(data) = app.path().app_data_dir() {
                let _ = std::fs::remove_dir_all(data.join("pictures"));
            }
            // Only song files keep a copy: the paths of any others go.
            diesel::sql_query(format!(
                "DELETE FROM file_paths WHERE file_id IN \
                 (SELECT id FROM files INDEXED BY files_meta WHERE kind NOT IN ('{SONG_AUDIO}', '{SONG_IMAGE}'))"
            ))
            .execute(&mut *db.lock())?;
            let ids: Vec<FileId> = files::table
                .filter(files::kind.eq_any([SONG_AUDIO, SONG_IMAGE]))
                .select(files::id)
                .load(&mut *db.lock())?;
            for id in ids {
                materialize(&mut db.lock(), &dir, id)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            crate::diagnostics::note("background", format!("Writing song files to disk failed: {error}"));
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

        // Ranges are read inside SQLite.
        assert_eq!(read_range(conn, id, 1, 2).unwrap(), b"bc");
        assert_eq!(slice(3, None), Slice::Whole);
        assert_eq!(slice(3, parse_range("bytes=1-")), Slice::Part(1, 2));
        assert_eq!(slice(3, parse_range("bytes=0-99")), Slice::Part(0, 2));
        assert_eq!(slice(3, parse_range("bytes=5-")), Slice::Unsatisfiable);

        assert_eq!(delete(conn, &[id]).unwrap(), 1);
        assert!(!Path::new(&path).exists(), "the copy goes with the row");
        assert!(info(conn, id).is_err());
    }
}
