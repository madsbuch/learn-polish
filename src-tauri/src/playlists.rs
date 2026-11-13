//! Playlists of song takes, and getting a take's MP3 out of the app: saved
//! to a file the learner picks, or handed to another app through the phone's
//! share sheet.

use diesel::dsl::max;
use diesel::prelude::*;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::db::{self, Conn, Db};
use crate::schema::{playlist_tracks, playlists, song_tracks, songs};
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

pub fn list(conn: &mut Conn) -> Result<Vec<Playlist>> {
    let heads: Vec<(i64, String)> = playlists::table
        .order((playlists::created_at.desc(), playlists::id.desc()))
        .select((playlists::id, playlists::name))
        .load(conn)?;
    heads.into_iter().map(|(id, name)| Ok(Playlist { entries: entries(conn, id)?, id, name })).collect()
}

fn entries(conn: &mut Conn, playlist_id: i64) -> Result<Vec<PlaylistEntry>> {
    Ok(playlist_tracks::table
        .filter(playlist_tracks::playlist_id.eq(playlist_id))
        .order((playlist_tracks::ord, playlist_tracks::id))
        .select((playlist_tracks::id, playlist_tracks::track_id))
        .load::<(i64, i64)>(conn)?
        .into_iter()
        .map(|(id, track_id)| PlaylistEntry { id, track_id })
        .collect())
}

fn load(conn: &mut Conn, playlist_id: i64) -> Result<Playlist> {
    let name: String = playlists::table
        .find(playlist_id)
        .select(playlists::name)
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg("That playlist no longer exists."))?;
    Ok(Playlist { id: playlist_id, name, entries: entries(conn, playlist_id)? })
}

pub fn create(conn: &mut Conn, name: &str, track_ids: &[i64]) -> Result<Playlist> {
    let id: i64 = diesel::insert_into(playlists::table)
        .values((playlists::name.eq(clean_name(name)?), playlists::created_at.eq(db::now_secs())))
        .returning(playlists::id)
        .get_result(conn)?;
    add(conn, id, track_ids)
}

pub fn add(conn: &mut Conn, playlist_id: i64, track_ids: &[i64]) -> Result<Playlist> {
    load(conn, playlist_id)?;
    conn.transaction::<_, Error, _>(|conn| {
        let mut ord = playlist_tracks::table
            .filter(playlist_tracks::playlist_id.eq(playlist_id))
            .select(max(playlist_tracks::ord))
            .first::<Option<i64>>(conn)?
            .map_or(0, |o| o + 1);
        for track_id in track_ids {
            diesel::insert_into(playlist_tracks::table)
                .values((
                    playlist_tracks::playlist_id.eq(playlist_id),
                    playlist_tracks::track_id.eq(track_id),
                    playlist_tracks::ord.eq(ord),
                ))
                .execute(conn)?;
            ord += 1;
        }
        Ok(())
    })?;
    load(conn, playlist_id)
}

/// Moves an entry `delta` places up (negative) or down, then renumbers.
pub fn move_entry(conn: &mut Conn, playlist_id: i64, entry_id: i64, delta: i64) -> Result<Playlist> {
    let mut list = entries(conn, playlist_id)?;
    let Some(from) = list.iter().position(|e| e.id == entry_id) else {
        return load(conn, playlist_id);
    };
    let to = (from as i64 + delta).clamp(0, list.len() as i64 - 1) as usize;
    let entry = list.remove(from);
    list.insert(to, entry);
    conn.transaction::<_, Error, _>(|conn| {
        for (ord, entry) in list.iter().enumerate() {
            diesel::update(playlist_tracks::table.find(entry.id))
                .set(playlist_tracks::ord.eq(ord as i64))
                .execute(conn)?;
        }
        Ok(())
    })?;
    load(conn, playlist_id)
}

pub fn rename(conn: &mut Conn, playlist_id: i64, name: &str) -> Result<Playlist> {
    load(conn, playlist_id)?;
    diesel::update(playlists::table.find(playlist_id)).set(playlists::name.eq(clean_name(name)?)).execute(conn)?;
    load(conn, playlist_id)
}

/// Deletes a playlist; its songs stay in the library. Returns its name.
pub fn delete(conn: &mut Conn, playlist_id: i64) -> Result<String> {
    let name = load(conn, playlist_id)?.name;
    diesel::delete(playlists::table.find(playlist_id)).execute(conn)?;
    Ok(name)
}

/// Takes one entry out of a playlist.
pub fn remove_entry(conn: &mut Conn, playlist_id: i64, entry_id: i64) -> Result<Playlist> {
    diesel::delete(
        playlist_tracks::table.find(entry_id).filter(playlist_tracks::playlist_id.eq(playlist_id)),
    )
    .execute(conn)?;
    load(conn, playlist_id)
}

// ---------------------------------------------------------------------------
// By song, for the tutor
// ---------------------------------------------------------------------------

/// The take of a song a playlist gets when it is asked for by song: the one
/// played most (the first, if none has been), as "play all" picks.
pub fn preferred_track(conn: &mut Conn, song_id: i64) -> Result<i64> {
    let title = song_title(conn, song_id)?;
    song_tracks::table
        .filter(song_tracks::song_id.eq(song_id))
        .order((song_tracks::play_count.desc(), song_tracks::ord))
        .select(song_tracks::id)
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg(format!("“{title}” has no audio yet, so it cannot go in a playlist.")))
}

fn song_title(conn: &mut Conn, song_id: i64) -> Result<String> {
    songs::table
        .find(song_id)
        .select(songs::title)
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg(format!("There is no song with id {song_id}.")))
}

/// Songs a playlist holds, in order: (entry id, song id, title, take number).
pub fn songs_in(conn: &mut Conn, playlist_id: i64) -> Result<Vec<(i64, i64, String, i64)>> {
    Ok(playlist_tracks::table
        .inner_join(song_tracks::table.inner_join(songs::table))
        .filter(playlist_tracks::playlist_id.eq(playlist_id))
        .order((playlist_tracks::ord, playlist_tracks::id))
        .select((playlist_tracks::id, songs::id, songs::title, song_tracks::ord))
        .load::<(i64, i64, String, i64)>(conn)?
        .into_iter()
        .map(|(entry, song, title, ord)| (entry, song, title, ord + 1))
        .collect())
}

/// Adds songs (their preferred take) to the end of a playlist. A song the
/// playlist already holds is skipped; returns the titles added and skipped.
pub fn add_songs(conn: &mut Conn, playlist_id: i64, song_ids: &[i64]) -> Result<(Vec<String>, Vec<String>)> {
    let present: Vec<i64> = songs_in(conn, playlist_id)?.iter().map(|(_, song, _, _)| *song).collect();
    let (mut added, mut skipped, mut tracks) = (Vec::new(), Vec::new(), Vec::new());
    for &song in song_ids {
        let title = song_title(conn, song)?;
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
pub fn remove_songs(conn: &mut Conn, playlist_id: i64, song_ids: &[i64]) -> Result<usize> {
    load(conn, playlist_id)?;
    let takes = song_tracks::table.filter(song_tracks::song_id.eq_any(song_ids)).select(song_tracks::id);
    Ok(diesel::delete(
        playlist_tracks::table
            .filter(playlist_tracks::playlist_id.eq(playlist_id))
            .filter(playlist_tracks::track_id.eq_any(takes)),
    )
    .execute(conn)?)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn playlists_list(db: State<'_, Db>) -> Result<Vec<Playlist>> {
    list(&mut db.lock())
}

#[tauri::command]
pub async fn playlist_create(db: State<'_, Db>, name: String, track_ids: Vec<i64>) -> Result<Playlist> {
    create(&mut db.lock(), &name, &track_ids)
}

#[tauri::command]
pub async fn playlist_rename(db: State<'_, Db>, playlist_id: i64, name: String) -> Result<Playlist> {
    rename(&mut db.lock(), playlist_id, &name)
}

#[tauri::command]
pub async fn playlist_delete(db: State<'_, Db>, playlist_id: i64) -> Result<()> {
    delete(&mut db.lock(), playlist_id).map(|_| ())
}

#[tauri::command]
pub async fn playlist_add(db: State<'_, Db>, playlist_id: i64, track_ids: Vec<i64>) -> Result<Playlist> {
    add(&mut db.lock(), playlist_id, &track_ids)
}

#[tauri::command]
pub async fn playlist_remove(db: State<'_, Db>, playlist_id: i64, entry_id: i64) -> Result<Playlist> {
    remove_entry(&mut db.lock(), playlist_id, entry_id)
}

#[tauri::command]
pub async fn playlist_move(
    db: State<'_, Db>,
    playlist_id: i64,
    entry_id: i64,
    delta: i64,
) -> Result<Playlist> {
    move_entry(&mut db.lock(), playlist_id, entry_id, delta)
}

// ---------------------------------------------------------------------------
// Getting the MP3 out
// ---------------------------------------------------------------------------

/// A take's bytes and a friendly file name, e.g. "Kawa (take 2).mp3".
fn track_file(conn: &mut Conn, track_id: i64) -> Result<(String, Vec<u8>)> {
    let (title, ord, path, bytes): (String, i64, String, Option<Vec<u8>>) = song_tracks::table
        .inner_join(songs::table)
        .filter(song_tracks::id.eq(track_id))
        .select((songs::title, song_tracks::ord, song_tracks::audio_path, song_tracks::audio_bytes))
        .first(conn)
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

    let (name, bytes) = track_file(&mut db.lock(), track_id)?;
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
    let (name, bytes) = track_file(&mut db.lock(), track_id)?;
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
