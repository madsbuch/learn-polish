//! Titles for ids. People never read an id (DATA.md): where one turns up in
//! text — a tutor reply, the Diagnostics log — the screens show what it
//! names instead. This looks each one up by its prefix.

use diesel::prelude::*;
use serde::Serialize;
use tauri::State;

use crate::db::{Conn, Db};
use crate::error::Result;
use crate::ids::*;
use crate::schema::*;

/// What an id names: its kind ("lesson") and its title, or no title when
/// nothing has it (deleted, or never was).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    pub id: String,
    pub kind: String,
    pub title: Option<String>,
}

/// At most this many characters of a title (a memory is a whole note).
const LONGEST: usize = 60;

fn short(text: String) -> String {
    let text = text.trim();
    match text.char_indices().nth(LONGEST) {
        Some((cut, _)) => format!("{}…", text[..cut].trim_end()),
        None => text.to_string(),
    }
}

/// The title of one id, if it is a known kind and something has it.
fn title(conn: &mut Conn, id: &str) -> Result<Option<String>> {
    fn first<T: std::str::FromStr>(id: &str) -> Option<T> {
        id.parse().ok()
    }
    let found = match id.split_once('_').map(|(prefix, _)| prefix) {
        Some("crs") => match first::<CourseId>(id) {
            Some(key) => courses::table.find(key).select(courses::title_en).first(conn).optional()?,
            None => None,
        },
        Some("ls") => match first::<LessonId>(id) {
            Some(key) => lessons::table.find(key).select(lessons::title).first(conn).optional()?,
            None => None,
        },
        Some("wd") => match first::<WordId>(id) {
            Some(key) => words::table.find(key).select(words::pl).first(conn).optional()?,
            None => None,
        },
        Some("sn") => match first::<SentenceId>(id) {
            Some(key) => sentences::table.find(key).select(sentences::pl).first(conn).optional()?,
            None => None,
        },
        Some("dl") => match first::<LineId>(id) {
            Some(key) => conversation_lines::table.find(key).select(conversation_lines::pl).first(conn).optional()?,
            None => None,
        },
        Some("sg") => match first::<SongId>(id) {
            Some(key) => songs::table.find(key).select(songs::title).first(conn).optional()?,
            None => None,
        },
        Some("tk") => match first::<TrackId>(id) {
            Some(key) => song_tracks::table
                .inner_join(songs::table)
                .filter(song_tracks::id.eq(key))
                .select(songs::title)
                .first(conn)
                .optional()?,
            None => None,
        },
        Some("pl") => match first::<PlaylistId>(id) {
            Some(key) => playlists::table.find(key).select(playlists::name).first(conn).optional()?,
            None => None,
        },
        Some("fl") => match first::<FileId>(id) {
            Some(key) => files::table.find(key).select(files::name).first(conn).optional()?,
            None => None,
        },
        Some("ci") => match first::<CheckinId>(id) {
            Some(key) => checkins::table
                .inner_join(lessons::table)
                .filter(checkins::id.eq(key))
                .select(lessons::title)
                .first(conn)
                .optional()?,
            None => None,
        },
        Some("cv") => match first::<ConversationId>(id) {
            Some(key) => tutor_conversations::table.find(key).select(tutor_conversations::title).first(conn).optional()?,
            None => None,
        },
        Some("mem") => match first::<MemoryId>(id) {
            Some(key) => memories::table.find(key).select(memories::content).first(conn).optional()?,
            None => None,
        },
        Some("src") => match first::<SourceId>(id) {
            Some(key) => wiki_sources::table.find(key).select(wiki_sources::title).first(conn).optional()?,
            None => None,
        },
        _ => None,
    };
    Ok(found.map(short).filter(|t| !t.is_empty()))
}

/// What each id names. Ids of no known kind are left out; known kinds with
/// nothing behind them come back without a title.
pub fn resolve(conn: &mut Conn, ids: &[String]) -> Result<Vec<Resolved>> {
    let mut out = Vec::new();
    for id in ids {
        let Some(kind) = kind_of(id) else { continue };
        out.push(Resolved { id: id.clone(), kind: kind.to_string(), title: title(conn, id)? });
    }
    Ok(out)
}

#[tauri::command]
pub async fn ids_resolve(db: State<'_, Db>, ids: Vec<String>) -> Result<Vec<Resolved>> {
    let mut guard = db.lock();
    resolve(&mut guard, &ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_by_kind() {
        let path = std::env::temp_dir().join("learn-polish-tests").join(format!("resolve-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut conn = crate::db::open(&path).expect("db");
        let course = crate::courses::create_empty(&mut conn, "A course", "").expect("course");
        let ids = vec![course.to_string(), LessonId::new().to_string(), "nope_123".to_string()];
        let found = resolve(&mut conn, &ids).expect("resolve");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].kind, "course");
        assert_eq!(found[0].title.as_deref(), Some("A course"));
        assert_eq!(found[1].kind, "lesson");
        assert_eq!(found[1].title, None);
    }

    #[test]
    fn long_titles_are_cut() {
        let cut = short("x".repeat(100));
        assert_eq!(cut.chars().count(), LONGEST + 1);
        assert!(cut.ends_with('…'));
    }
}
