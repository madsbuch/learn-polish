//! Playlists of song takes, and getting a take's MP3 out of the app: saved
//! to a file the learner picks, or handed to another app through the phone's
//! share sheet.

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::db::{self, Db};
use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    /// The entry itself: the same take can be in a playlist twice.
    pub id: i64,
    pub track_id: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub entries: Vec<PlaylistEntry>,
}

fn clean_name(name: &str) -> Result<String> {
    let name = crate::text::clean(name);
    if name.is_empty() {
        return Err(Error::msg("A playlist needs a name."));
    }
    Ok(name.chars().take(80).collect())
}

pub fn list(conn: &Connection) -> Result<Vec<Playlist>> {
    let mut stmt = conn.prepare("SELECT id, name FROM playlists ORDER BY created_at DESC, id DESC")?;
    let heads: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    heads.into_iter().map(|(id, name)| Ok(Playlist { entries: entries(conn, id)?, id, name })).collect()
}

fn entries(conn: &Connection, playlist_id: i64) -> Result<Vec<PlaylistEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY ord, id",
    )?;
    let rows = stmt
        .query_map([playlist_id], |r| Ok(PlaylistEntry { id: r.get(0)?, track_id: r.get(1)? }))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn load(conn: &Connection, playlist_id: i64) -> Result<Playlist> {
    let name: String = conn
        .query_row("SELECT name FROM playlists WHERE id = ?1", [playlist_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::msg("That playlist no longer exists."))?;
    Ok(Playlist { id: playlist_id, name, entries: entries(conn, playlist_id)? })
}

pub fn create(conn: &Connection, name: &str, track_ids: &[i64]) -> Result<Playlist> {
    conn.execute(
        "INSERT INTO playlists (name, created_at) VALUES (?1, ?2)",
        rusqlite::params![clean_name(name)?, db::now_secs()],
    )?;
    let id = conn.last_insert_rowid();
    add(conn, id, track_ids)
}

pub fn add(conn: &Connection, playlist_id: i64, track_ids: &[i64]) -> Result<Playlist> {
    load(conn, playlist_id)?;
    let tx = conn.unchecked_transaction()?;
    let mut ord: i64 = tx.query_row(
        "SELECT COALESCE(MAX(ord), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
        [playlist_id],
        |r| r.get(0),
    )?;
    for track_id in track_ids {
        tx.execute(
            "INSERT INTO playlist_tracks (playlist_id, track_id, ord) VALUES (?1, ?2, ?3)",
            rusqlite::params![playlist_id, track_id, ord],
        )?;
        ord += 1;
    }
    tx.commit()?;
    load(conn, playlist_id)
}

/// Moves an entry `delta` places up (negative) or down, then renumbers.
pub fn move_entry(conn: &Connection, playlist_id: i64, entry_id: i64, delta: i64) -> Result<Playlist> {
    let mut list = entries(conn, playlist_id)?;
    let Some(from) = list.iter().position(|e| e.id == entry_id) else {
        return load(conn, playlist_id);
    };
    let to = (from as i64 + delta).clamp(0, list.len() as i64 - 1) as usize;
    let entry = list.remove(from);
    list.insert(to, entry);
    let tx = conn.unchecked_transaction()?;
    for (ord, entry) in list.iter().enumerate() {
        tx.execute(
            "UPDATE playlist_tracks SET ord = ?2 WHERE id = ?1",
            rusqlite::params![entry.id, ord as i64],
        )?;
    }
    tx.commit()?;
    load(conn, playlist_id)
}

pub fn rename(conn: &Connection, playlist_id: i64, name: &str) -> Result<Playlist> {
    load(conn, playlist_id)?;
    conn.execute(
        "UPDATE playlists SET name = ?2 WHERE id = ?1",
        rusqlite::params![playlist_id, clean_name(name)?],
    )?;
    load(conn, playlist_id)
}

/// Deletes a playlist; its songs stay in the library. Returns its name.
pub fn delete(conn: &Connection, playlist_id: i64) -> Result<String> {
    let name = load(conn, playlist_id)?.name;
    conn.execute("DELETE FROM playlists WHERE id = ?1", [playlist_id])?;
    Ok(name)
}

// ---------------------------------------------------------------------------
// By song, for the tutor
// ---------------------------------------------------------------------------

/// The take of a song a playlist gets when it is asked for by song: the one
/// played most (the first, if none has been), as "play all" picks.
pub fn preferred_track(conn: &Connection, song_id: i64) -> Result<i64> {
    let title: String = conn
        .query_row("SELECT title FROM songs WHERE id = ?1", [song_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::msg(format!("There is no song with id {song_id}.")))?;
    conn.query_row(
        "SELECT id FROM song_tracks WHERE song_id = ?1 ORDER BY play_count DESC, ord LIMIT 1",
        [song_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| Error::msg(format!("“{title}” has no audio yet, so it cannot go in a playlist.")))
}

/// Songs a playlist holds, in order: (entry id, song id, title, take number).
pub fn songs_in(conn: &Connection, playlist_id: i64) -> Result<Vec<(i64, i64, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, s.id, s.title, t.ord + 1 FROM playlist_tracks p
         JOIN song_tracks t ON t.id = p.track_id JOIN songs s ON s.id = t.song_id
         WHERE p.playlist_id = ?1 ORDER BY p.ord, p.id",
    )?;
    let rows = stmt
        .query_map([playlist_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Adds songs (their preferred take) to the end of a playlist. A song the
/// playlist already holds is skipped; returns the titles added and skipped.
pub fn add_songs(conn: &Connection, playlist_id: i64, song_ids: &[i64]) -> Result<(Vec<String>, Vec<String>)> {
    let present: Vec<i64> = songs_in(conn, playlist_id)?.iter().map(|(_, song, _, _)| *song).collect();
    let (mut added, mut skipped, mut tracks) = (Vec::new(), Vec::new(), Vec::new());
    for &song in song_ids {
        let title: String = conn
            .query_row("SELECT title FROM songs WHERE id = ?1", [song], |r| r.get(0))
            .optional()?
            .ok_or_else(|| Error::msg(format!("There is no song with id {song}.")))?;
        if present.contains(&song) || added.contains(&title) {
            skipped.push(title);
            continue;
        }
        tracks.push(preferred_track(conn, song)?);
        added.push(title);
    }
    add(conn, playlist_id, &tracks)?;
    Ok((added, skipped))
}

/// Takes every take of these songs out of a playlist. Returns how many
/// entries went.
pub fn remove_songs(conn: &Connection, playlist_id: i64, song_ids: &[i64]) -> Result<usize> {
    load(conn, playlist_id)?;
    let mut removed = 0;
    for song in song_ids {
        removed += conn.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1
               AND track_id IN (SELECT id FROM song_tracks WHERE song_id = ?2)",
            [playlist_id, *song],
        )?;
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn playlists_list(db: State<'_, Db>) -> Result<Vec<Playlist>> {
    list(&db.lock())
}

#[tauri::command]
pub async fn playlist_create(db: State<'_, Db>, name: String, track_ids: Vec<i64>) -> Result<Playlist> {
    create(&db.lock(), &name, &track_ids)
}

#[tauri::command]
pub async fn playlist_rename(db: State<'_, Db>, playlist_id: i64, name: String) -> Result<Playlist> {
    rename(&db.lock(), playlist_id, &name)
}

#[tauri::command]
pub async fn playlist_delete(db: State<'_, Db>, playlist_id: i64) -> Result<()> {
    delete(&db.lock(), playlist_id).map(|_| ())
}

#[tauri::command]
pub async fn playlist_add(db: State<'_, Db>, playlist_id: i64, track_ids: Vec<i64>) -> Result<Playlist> {
    add(&db.lock(), playlist_id, &track_ids)
}

#[tauri::command]
pub async fn playlist_remove(db: State<'_, Db>, playlist_id: i64, entry_id: i64) -> Result<Playlist> {
    let conn = db.lock();
    conn.execute(
        "DELETE FROM playlist_tracks WHERE id = ?1 AND playlist_id = ?2",
        rusqlite::params![entry_id, playlist_id],
    )?;
    load(&conn, playlist_id)
}

#[tauri::command]
pub async fn playlist_move(
    db: State<'_, Db>,
    playlist_id: i64,
    entry_id: i64,
    delta: i64,
) -> Result<Playlist> {
    move_entry(&db.lock(), playlist_id, entry_id, delta)
}

// ---------------------------------------------------------------------------
// Getting the MP3 out
// ---------------------------------------------------------------------------

/// A take's bytes and a friendly file name, e.g. "Kawa (take 2).mp3".
fn track_file(conn: &Connection, track_id: i64) -> Result<(String, Vec<u8>)> {
    let (title, ord, path, bytes): (String, i64, String, Option<Vec<u8>>) = conn
        .query_row(
            "SELECT s.title, t.ord, t.audio_path, t.audio_bytes
             FROM song_tracks t JOIN songs s ON s.id = t.song_id WHERE t.id = ?1",
            [track_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or_else(|| Error::msg("That take no longer exists."))?;
    let bytes = match bytes {
        Some(bytes) => bytes,
        None => std::fs::read(&path)?,
    };
    let extension = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp3")
        .to_string();
    Ok((format!("{} (take {}).{extension}", safe_file_name(&title), ord + 1), bytes))
}

/// Strips what file systems and share targets dislike from a song title.
fn safe_file_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { ' ' } else { c })
        .collect();
    let cleaned = crate::text::clean(&cleaned);
    if cleaned.is_empty() { "Song".into() } else { cleaned.chars().take(80).collect() }
}

/// Saves a take wherever the learner picks. Returns where it went, or `None`
/// if the dialog was cancelled.
#[tauri::command]
pub async fn song_track_save(app: AppHandle, db: State<'_, Db>, track_id: i64) -> Result<Option<String>> {
    use std::io::Write;
    use tauri_plugin_dialog::DialogExt;
    use tauri_plugin_fs::FsExt;

    let (name, bytes) = track_file(&db.lock(), track_id)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Save song")
        .set_file_name(&name)
        .add_filter("MP3 audio", &["mp3"])
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(target) = receiver.await.ok().flatten() else {
        return Ok(None);
    };
    let shown = target.to_string();
    let mut options = tauri_plugin_fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    let mut out = app
        .fs()
        .open(target, options)
        .map_err(|e| Error::msg(format!("Could not save the song: {e}")))?;
    out.write_all(&bytes)
        .map_err(|e| Error::msg(format!("Could not save the song: {e}")))?;
    Ok(Some(shown))
}

/// Writes a take to the app's cache under its friendly name and returns the
/// path, for the share sheet to hand to another app.
#[tauri::command]
pub async fn song_track_share_file(app: AppHandle, db: State<'_, Db>, track_id: i64) -> Result<String> {
    let (name, bytes) = track_file(&db.lock(), track_id)?;
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| Error::msg(format!("no cache directory: {e}")))?
        .join("share");
    // Only the latest share is kept; the receiving app has copied it by then.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_safe() {
        assert_eq!(safe_file_name("Kawa / herbata?"), "Kawa herbata");
        assert_eq!(safe_file_name("   "), "Song");
        assert_eq!(safe_file_name("Dzień dobry"), "Dzień dobry");
    }
}
