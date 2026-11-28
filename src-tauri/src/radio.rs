//! The radio (.spec/music/radio.json): a programme that plays the library
//! while a mixing agent orders new songs, podcast episodes and news for it,
//! and links what plays with short spoken bridges.
//!
//! The session lives on this device and in memory: tuning in starts one,
//! leaving the radio ends it. What it makes is ordinary library content and
//! outlives it (songs resume after a restart like any other). The player asks
//! for the next items with [`radio_next`] before it runs out; the agent plans
//! a few items ahead in the background, and when it has planned nothing (no
//! key, offline, still thinking) the next item is picked without a model.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::agent::{tool, Agent, Tool, ToolOutput};
use crate::ai::{self, Wire};
use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
use crate::files;
#[allow(unused_imports)]
use crate::ids::*;
use crate::schema::{bridges, courses, song_tracks, songs as song_rows};
use crate::songs::{self, Song, SongLine, SongSection, SongTrack, TimedLine};
use crate::speech::{self, Delivery};

/// Where the bridges' format is kept (synced: it is the learner's).
pub const FORMAT_SETTING: &str = "radio_format";
/// How bridges are spoken, as podcasts are (POD-3).
// Spec: RADIO-9
pub const FORMATS: &[&str] = &["pairs", "immersion"];

/// How many songs and episodes the agent keeps planned (`radio.ahead`).
const AHEAD: usize = 3;
/// How many new items are made for the radio at once (`radio.making`).
const MAKING: usize = 2;
/// The most lines in a bridge (`radio.bridge-lines`).
const BRIDGE_LINES: usize = 3;
/// Songs between two spoken episodes (`radio.songs-per-episode`).
const SONGS_PER_EPISODE: usize = 3;
/// How often the session looks at what it is making, when nothing wakes it.
const TICK: Duration = Duration::from_secs(4);
/// After a failed turn the agent rests this long; the fallback plays on.
const REST: Duration = Duration::from_secs(60);
/// Tool rounds in one of the agent's turns.
const MAX_ROUNDS: usize = 8;
/// Items handed to the player that are not picked again soon.
const RECENT: usize = 8;

/// The bridges' format: "pairs" unless set.
pub fn format(conn: &mut Conn) -> String {
    db::get_setting(conn, FORMAT_SETTING)
        .ok()
        .flatten()
        .filter(|f| FORMATS.contains(&f.as_str()))
        .unwrap_or_else(|| FORMATS[0].to_string())
}

// ---------------------------------------------------------------------------
// Bridges
// ---------------------------------------------------------------------------

/// One line of a bridge: Polish, and its English.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BridgeLine {
    pub pl: String,
    pub en: String,
}

/// A spoken bridge, as the player plays it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bridge {
    pub id: BridgeId,
    /// The song or episode it is about; None: the station.
    pub song_id: Option<SongId>,
    /// "intro" | "outro" | "station"
    pub role: String,
    pub lines: Vec<BridgeLine>,
    pub format: String,
    pub audio_file_id: FileId,
    /// The copy on disk, for Android's native player.
    pub audio_path: String,
    pub duration: f64,
    pub timed_lines: Vec<TimedLine>,
    pub play_count: i64,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = bridges, check_for_backend(diesel::sqlite::Sqlite))]
struct BridgeRow {
    id: BridgeId,
    song_id: Option<SongId>,
    role: String,
    lines: String,
    format: String,
    audio_file_id: FileId,
    duration: f64,
    timed_lines: String,
    play_count: i64,
}

fn bridge_of(conn: &mut Conn, row: BridgeRow) -> Result<Bridge> {
    let audio_path = files::paths(conn, &[row.audio_file_id])?.remove(&row.audio_file_id).unwrap_or_default();
    Ok(Bridge {
        id: row.id,
        song_id: row.song_id,
        role: row.role,
        lines: serde_json::from_str(&row.lines).unwrap_or_default(),
        format: row.format,
        audio_file_id: row.audio_file_id,
        audio_path,
        duration: row.duration,
        timed_lines: serde_json::from_str(&row.timed_lines).unwrap_or_default(),
        play_count: row.play_count,
    })
}

pub(crate) fn load_bridge(conn: &mut Conn, id: BridgeId) -> Result<Option<Bridge>> {
    let row = bridges::table.find(id).select(BridgeRow::as_select()).first(conn).optional()?;
    row.map(|row| bridge_of(conn, row)).transpose()
}

/// Kept bridges, least played first: about one song or episode, or (None)
/// about the station.
fn kept_bridges(conn: &mut Conn, about: Option<SongId>, role: Option<&str>, limit: i64) -> Result<Vec<Bridge>> {
    let mut query = bridges::table.into_boxed();
    query = match about {
        Some(song_id) => query.filter(bridges::song_id.eq(song_id)),
        None => query.filter(bridges::song_id.is_null()),
    };
    if let Some(role) = role {
        query = query.filter(bridges::role.eq(role.to_string()));
    }
    let rows: Vec<BridgeRow> = query
        .order((bridges::play_count, bridges::id))
        .limit(limit)
        .select(BridgeRow::as_select())
        .load(conn)?;
    rows.into_iter().map(|row| bridge_of(conn, row)).collect()
}

/// Deletes the bridges about a song or episode, and returns their audio
/// files for the caller to delete with its own.
// Spec: RADIO-8
pub(crate) fn delete_bridges_about(conn: &mut Conn, song_id: SongId) -> Result<Vec<FileId>> {
    let about = bridges::table.filter(bridges::song_id.eq(song_id));
    let files: Vec<FileId> = about.select(bridges::audio_file_id).load(conn)?;
    diesel::delete(about).execute(conn)?;
    Ok(files)
}

/// A bridge's title and "artist" on a speaker's display.
pub(crate) fn cast_title(conn: &mut Conn, id: BridgeId) -> Result<Option<(String, String)>> {
    let lines: Option<String> = bridges::table.find(id).select(bridges::lines).first(conn).optional()?;
    Ok(lines.map(|json| {
        let lines: Vec<BridgeLine> = serde_json::from_str(&json).unwrap_or_default();
        let first = lines.first().map(|l| l.pl.clone()).unwrap_or_default();
        ("Radio".to_string(), first)
    }))
}

pub(crate) fn bridge_played(conn: &mut Conn, id: BridgeId) -> Result<()> {
    diesel::update(bridges::table.find(id))
        .set((bridges::play_count.eq(bridges::play_count + 1), bridges::last_played_at.eq(db::now_secs())))
        .execute(conn)?;
    Ok(())
}

/// Checks a bridge the agent wrote: one to `radio.bridge-lines` lines, each
/// with its Polish.
// Spec: RADIO-5
fn clean_lines(lines: Vec<BridgeLine>) -> Result<Vec<BridgeLine>> {
    let lines: Vec<BridgeLine> = lines
        .into_iter()
        .map(|l| BridgeLine { pl: l.pl.trim().to_string(), en: l.en.trim().to_string() })
        .filter(|l| !l.pl.is_empty())
        .collect();
    if lines.is_empty() {
        return Err(Error::msg("A bridge needs at least one line with its Polish."));
    }
    if lines.len() > BRIDGE_LINES {
        return Err(Error::msg(format!("A bridge is at most {BRIDGE_LINES} lines.")));
    }
    if lines.iter().any(|l| l.pl.chars().count() > 240 || l.en.chars().count() > 240) {
        return Err(Error::msg("Keep a bridge's lines short: one sentence each."));
    }
    Ok(lines)
}

/// Speaks a bridge in the speech voices, in the radio's format, and keeps it
/// with its audio (RADIO-7). Its clips are cached like any speech, but the
/// bridge keeps its own file, so clearing the speech cache does not lose it.
// Spec: RADIO-5, RADIO-7, RADIO-9
async fn make_bridge(
    app: &AppHandle,
    db: &Db,
    song_id: Option<SongId>,
    role: &str,
    lines: Vec<BridgeLine>,
) -> Result<BridgeId> {
    let lines = clean_lines(lines)?;
    let (format, polish_voice, english_voice) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        if let Some(song_id) = song_id {
            song_rows::table
                .find(song_id)
                .select(song_rows::id)
                .first::<SongId>(conn)
                .optional()?
                .ok_or_else(|| Error::msg("That song or episode no longer exists."))?;
        }
        (format(conn), ai::voice_for(conn, "f"), ai::voice_for(conn, "m"))
    };
    let audio = crate::podcasts::PodcastAudio::for_format(&format);
    let section = SongSection {
        kind: "bridge".into(),
        lines: lines.iter().map(|l| SongLine { en: l.en.clone(), pl: l.pl.clone(), ..Default::default() }).collect(),
    };
    let mut clips = Vec::new();
    for line in &section.lines {
        if !line.en.is_empty() && (audio.english || line.pl.is_empty()) {
            clips.push(speech::pcm_clip(db, &line.en, &english_voice, Delivery::English, "").await?);
        }
        if !line.pl.is_empty() {
            clips.push(speech::pcm_clip(db, &line.pl, &polish_voice, Delivery::Polish, "").await?);
        }
    }
    let episode = crate::podcasts::assemble(std::slice::from_ref(&section), &clips, &audio)?;
    let (duration, timed) = (episode.duration, episode.lines);
    let (mime, bytes) = tauri::async_runtime::spawn_blocking(move || {
        crate::audio::compact(crate::audio::WAV.to_string(), episode.wav)
    })
    .await
    .map_err(|e| Error::msg(format!("Could not store the bridge's audio: {e}")))?;

    let mut guard = db.lock();
    let conn = &mut *guard;
    let id = BridgeId::new();
    let name = format!("bridge-{id}.{}", files::extension_for(&mime));
    let file_id = files::store(conn, files::BRIDGE_AUDIO, &name, &mime, &bytes)?;
    files::materialize(conn, &files::media_dir(app)?, file_id)?;
    insert_bridge(conn, id, song_id, role, &lines, &format, &polish_voice, file_id, duration, &timed)?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
fn insert_bridge(
    conn: &mut Conn,
    id: BridgeId,
    song_id: Option<SongId>,
    role: &str,
    lines: &[BridgeLine],
    format: &str,
    voice: &str,
    audio: FileId,
    duration: f64,
    timed: &[TimedLine],
) -> Result<()> {
    diesel::insert_into(bridges::table)
        .values((
            bridges::id.eq(id),
            bridges::song_id.eq(song_id),
            bridges::role.eq(role),
            bridges::lines.eq(serde_json::to_string(lines)?),
            bridges::format.eq(format),
            bridges::voice.eq(voice),
            bridges::audio_file_id.eq(audio),
            bridges::duration.eq(duration),
            bridges::timed_lines.eq(serde_json::to_string(timed)?),
            bridges::created_at.eq(db::now_secs()),
        ))
        .execute(conn)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The programme
// ---------------------------------------------------------------------------

/// One thing the radio plays.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Entry {
    Bridge(BridgeId),
    Take(SongId, TrackId),
}

impl Entry {
    fn song(self) -> Option<SongId> {
        match self {
            Entry::Take(song, _) => Some(song),
            Entry::Bridge(_) => None,
        }
    }
}

/// What the player is handed: a bridge, or a take with its song.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProgrammeItem {
    Bridge { bridge: Bridge },
    Take { song: Box<Song>, track: SongTrack },
}

/// A new song or episode the radio ordered this session.
#[derive(Debug, Clone)]
struct Order {
    song_id: SongId,
    /// "song" | "podcast" | "news"
    what: &'static str,
    /// Put on the programme already.
    planned: bool,
    /// The status last seen, to tell the screen when it changes.
    status: String,
}

struct Session {
    generation: u64,
    playing: bool,
    planned: VecDeque<Entry>,
    /// Handed to the player, oldest first.
    handed: Vec<Entry>,
    orders: Vec<Order>,
    thinking: bool,
    rest_until: Option<Instant>,
    /// Why the agent is not planning (no key, a failed turn).
    note: Option<String>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static WAKE: OnceLock<tokio::sync::Notify> = OnceLock::new();

/// Wakes the session's loop (an item handed out, play or pause).
fn wake() -> &'static tokio::sync::Notify {
    WAKE.get_or_init(tokio::sync::Notify::new)
}

fn session() -> std::sync::MutexGuard<'static, Option<Session>> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `f` on the session of `generation`, if it is still the one.
fn with_session<T>(generation: u64, f: impl FnOnce(&mut Session) -> T) -> Option<T> {
    session().as_mut().filter(|s| s.generation == generation).map(f)
}

fn current_generation() -> Option<u64> {
    session().as_ref().map(|s| s.generation)
}

fn changed(app: &AppHandle) {
    let _ = app.emit("radio://changed", ());
}

/// A song's take as the radio plays it: the one played most.
fn take_of(conn: &mut Conn, song_id: SongId) -> Result<Option<TrackId>> {
    let ready: Option<String> = song_rows::table.find(song_id).select(song_rows::status).first(conn).optional()?;
    if ready.as_deref() != Some("ready") {
        return Ok(None);
    }
    Ok(song_tracks::table
        .filter(song_tracks::song_id.eq(song_id))
        .filter(song_tracks::audio_file_id.is_not_null())
        .order((song_tracks::play_count.desc(), song_tracks::ord))
        .select(song_tracks::id)
        .first(conn)
        .optional()?)
}

// ---------------------------------------------------------------------------
// The mix: a music station, with now and then something spoken
// ---------------------------------------------------------------------------

/// What an item is, for the mix.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Song,
    /// A podcast or news episode.
    Episode,
    /// News from an earlier day: not played.
    OldNews,
}

/// When the learner's day began, in seconds (as db::today counts days).
fn day_start(conn: &mut Conn) -> i64 {
    db::today(conn) * 86_400 - db::get_setting_i64(conn, "tz_offset_minutes", 0) * 60
}

fn classify(kind: &str, origin: &str, created_at: i64, today: i64) -> Kind {
    if kind != "podcast" {
        return Kind::Song;
    }
    let news = serde_json::from_str::<songs::SongOrigin>(origin).is_ok_and(|o| o.news);
    if news && created_at < today {
        Kind::OldNews
    } else {
        Kind::Episode
    }
}

fn kind_of(conn: &mut Conn, song_id: SongId) -> Result<Kind> {
    let today = day_start(conn);
    let (kind, origin, created_at): (String, String, i64) = song_rows::table
        .find(song_id)
        .select((song_rows::kind, song_rows::origin, song_rows::created_at))
        .first(conn)?;
    Ok(classify(&kind, &origin, created_at, today))
}

/// Songs since the last spoken episode in `entries` (oldest first): all of
/// them when there was none.
fn songs_since_episode(conn: &mut Conn, entries: &[Entry]) -> Result<usize> {
    let mut songs = 0;
    for entry in entries.iter().rev() {
        if let Entry::Take(song_id, _) = entry {
            match kind_of(conn, *song_id) {
                Ok(Kind::Song) => songs += 1,
                Ok(_) => return Ok(songs),
                // Deleted since: it does not count either way.
                Err(_) => {}
            }
        }
    }
    Ok(songs)
}

/// Whether the library has a finished song to play.
fn has_songs(conn: &mut Conn) -> Result<bool> {
    let any: Option<SongId> = song_tracks::table
        .inner_join(song_rows::table)
        .filter(song_rows::status.eq("ready"))
        .filter(song_rows::kind.ne("podcast"))
        .filter(song_tracks::audio_file_id.is_not_null())
        .select(song_tracks::song_id)
        .first(conn)
        .optional()?;
    Ok(any.is_some())
}

/// Whether a podcast or news episode may come after `entries`: only after
/// `radio.songs-per-episode` songs since the last one, so the station opens
/// with a song too — unless there is no song to play.
// Spec: RADIO-17
fn episode_may_follow(conn: &mut Conn, entries: &[Entry]) -> Result<bool> {
    Ok(songs_since_episode(conn, entries)? >= SONGS_PER_EPISODE || !has_songs(conn)?)
}

/// What to play next when the agent has planned nothing: a new item the radio
/// made, first; else what was played longest ago, leaving out what played
/// just now; with a kept bridge about it, or now and then one about the
/// station.
// Spec: RADIO-4, RADIO-12, RADIO-14
fn pick_next(conn: &mut Conn, handed: &[Entry], ready_orders: &[SongId]) -> Result<Vec<Entry>> {
    let recent: Vec<Entry> = handed.iter().rev().take(RECENT).copied().collect();
    let recent_songs: Vec<SongId> = recent.iter().filter_map(|e| e.song()).collect();

    let episodes_ok = episode_may_follow(conn, handed)?;
    let fits = |kind: Kind| kind == Kind::Song || (kind == Kind::Episode && episodes_ok);

    let mut song = None;
    for id in ready_orders {
        if let Some(track) = take_of(conn, *id)? {
            if fits(kind_of(conn, *id)?) {
                song = Some((*id, track));
                break;
            }
        }
    }
    if song.is_none() {
        // Every finished song and episode with audio, by when it last played.
        let today = day_start(conn);
        let rows: Vec<(SongId, TrackId, Option<i64>, String, String, i64)> = song_tracks::table
            .inner_join(song_rows::table)
            .filter(song_rows::status.eq("ready"))
            .filter(song_tracks::audio_file_id.is_not_null())
            .order((song_tracks::play_count.desc(), song_tracks::ord))
            .select((
                song_tracks::song_id,
                song_tracks::id,
                song_tracks::last_played_at,
                song_rows::kind,
                song_rows::origin,
                song_rows::created_at,
            ))
            .load(conn)?;
        let mut candidates: Vec<(SongId, TrackId, i64, Kind)> = Vec::new();
        for (song_id, track_id, played, kind, origin, created_at) in rows {
            match candidates.iter_mut().find(|c| c.0 == song_id) {
                // The first row of a song is its take played most; its last
                // play is the latest of any take.
                Some(c) => c.2 = c.2.max(played.unwrap_or(0)),
                None => candidates.push((song_id, track_id, played.unwrap_or(0), classify(&kind, &origin, created_at, today))),
            }
        }
        // Spec: RADIO-17, RADIO-19
        candidates.retain(|c| fits(c.3));
        // With a small library, only what played last is left out.
        let keep_out = recent_songs.len().min(candidates.len().saturating_sub(1));
        let left_out = &recent_songs[..keep_out];
        song = candidates
            .iter()
            .filter(|c| !left_out.contains(&c.0))
            .min_by_key(|c| (c.2, c.0.to_string()))
            .map(|c| (c.0, c.1));
    }
    let Some((song_id, track_id)) = song else { return Ok(Vec::new()) };

    let recent_bridges: Vec<BridgeId> = handed
        .iter()
        .rev()
        .take(RECENT * 2)
        .filter_map(|e| match e {
            Entry::Bridge(id) => Some(*id),
            Entry::Take(..) => None,
        })
        .collect();
    let fresh = |bridges: Vec<Bridge>| bridges.into_iter().find(|b| !recent_bridges.contains(&b.id)).map(|b| b.id);
    let mut out = Vec::new();
    // Every item comes after a bridge: one about it, else one about the
    // station — one not heard just now if there is one, else the one heard
    // longest ago (the last one heard is never said twice in a row).
    // Spec: RADIO-20
    let after_bridge = matches!(handed.last(), Some(Entry::Bridge(_)));
    if !after_bridge {
        let last_bridge = recent_bridges.first().copied();
        let stations = kept_bridges(conn, None, Some("station"), 20)?;
        let bridge = fresh(kept_bridges(conn, Some(song_id), Some("intro"), 10)?)
            .or_else(|| fresh(stations.clone()))
            .or_else(|| stations.iter().map(|b| b.id).find(|id| Some(*id) != last_bridge));
        if let Some(bridge) = bridge {
            out.push(Entry::Bridge(bridge));
        }
    }
    out.push(Entry::Take(song_id, track_id));
    Ok(out)
}

/// An entry as the player gets it. With `dir`, a bridge that has no copy on
/// disk yet (it came from another device) gets one, for Android's player.
fn item_of(conn: &mut Conn, entry: Entry, dir: Option<&std::path::Path>) -> Result<Option<ProgrammeItem>> {
    Ok(match entry {
        Entry::Bridge(id) => match load_bridge(conn, id)? {
            Some(mut bridge) => {
                if let (true, Some(dir)) = (bridge.audio_path.is_empty(), dir) {
                    bridge.audio_path = files::materialize(conn, dir, bridge.audio_file_id)?;
                }
                Some(ProgrammeItem::Bridge { bridge })
            }
            None => None,
        },
        Entry::Take(song_id, track_id) => match songs::load_song(conn, song_id) {
            Ok(song) => song
                .tracks
                .iter()
                .find(|t| t.id == track_id)
                .cloned()
                .map(|track| ProgrammeItem::Take { song: Box::new(song), track }),
            Err(_) => None,
        },
    })
}

/// The radio's orders that are ready and not on the programme yet.
fn ready_orders(conn: &mut Conn, orders: &[Order]) -> Vec<SongId> {
    orders
        .iter()
        .filter(|o| !o.planned)
        .filter(|o| take_of(conn, o.song_id).ok().flatten().is_some())
        .map(|o| o.song_id)
        .collect()
}

// ---------------------------------------------------------------------------
// The state the screen shows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedView {
    /// "bridge" | "take"
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub song_id: Option<SongId>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MakingView {
    pub song_id: SongId,
    pub title: String,
    /// "song" | "podcast" | "news"
    pub what: String,
    /// generating | ready | failed
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioState {
    pub on: bool,
    pub playing: bool,
    /// The mixing agent is choosing what comes next.
    pub thinking: bool,
    /// Chosen, not yet handed to the player.
    pub planned: Vec<PlannedView>,
    /// New songs and episodes the radio ordered and has not played yet.
    pub making: Vec<MakingView>,
    /// Why the radio is not making anything new right now.
    pub note: Option<String>,
}

// Spec: RADIO-10
fn state_of(conn: &mut Conn) -> RadioState {
    let snapshot = session()
        .as_ref()
        .map(|s| (s.playing, s.thinking, s.planned.iter().copied().collect::<Vec<_>>(), s.orders.clone(), s.note.clone()));
    let Some((playing, thinking, planned, orders, note)) = snapshot else {
        return RadioState { on: false, playing: false, thinking: false, planned: Vec::new(), making: Vec::new(), note: None };
    };
    let title_of = |conn: &mut Conn, id: SongId| -> (String, String) {
        song_rows::table
            .find(id)
            .select((song_rows::title, song_rows::chorus_pl))
            .first(conn)
            .optional()
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let planned = planned
        .into_iter()
        .map(|entry| match entry {
            Entry::Take(song_id, _) => {
                let (title, detail) = title_of(conn, song_id);
                PlannedView { kind: "take".into(), title, detail, song_id: Some(song_id) }
            }
            Entry::Bridge(id) => {
                let first = cast_title(conn, id).ok().flatten().map(|t| t.1).unwrap_or_default();
                PlannedView { kind: "bridge".into(), title: first, detail: String::new(), song_id: None }
            }
        })
        .collect();
    let making = orders
        .into_iter()
        .filter(|o| !o.planned)
        .map(|o| {
            let (title, _) = title_of(conn, o.song_id);
            let status: String = song_rows::table
                .find(o.song_id)
                .select(song_rows::status)
                .first(conn)
                .optional()
                .ok()
                .flatten()
                .unwrap_or_else(|| "failed".into());
            MakingView { song_id: o.song_id, title, what: o.what.into(), status }
        })
        .filter(|m| m.status != "draft")
        .collect();
    RadioState { on: true, playing, thinking, planned, making, note }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Tunes in: a new programme starts, and the mixing agent with it.
// Spec: RADIO-1
#[tauri::command]
pub async fn radio_start(app: AppHandle, db: State<'_, Db>) -> Result<RadioState> {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    *session() = Some(Session {
        generation,
        playing: true,
        planned: VecDeque::new(),
        handed: Vec::new(),
        orders: Vec::new(),
        thinking: false,
        rest_until: None,
        note: None,
    });
    spawn_session(app.clone(), generation);
    changed(&app);
    Ok(state_of(&mut db.lock()))
}

/// The next `count` items for the player: what the agent planned, else what
/// the fallback picks. Empty only when there is nothing to play at all.
#[tauri::command]
pub async fn radio_next(app: AppHandle, db: State<'_, Db>, count: usize) -> Result<Vec<ProgrammeItem>> {
    let generation = current_generation().ok_or_else(|| Error::msg("The radio is off."))?;
    let mut out = Vec::new();
    let dir = files::media_dir(&app).ok();
    let mut guard = db.lock();
    let conn = &mut *guard;
    // Each round hands out something or gives up: an item that cannot be
    // loaded is passed over, but not forever.
    let mut rounds = 0;
    while out.len() < count.clamp(1, 6) && rounds < 12 {
        rounds += 1;
        let entry = with_session(generation, |s| s.planned.pop_front()).flatten();
        let entries = match entry {
            Some(entry) => vec![entry],
            None => {
                let (handed, orders) = with_session(generation, |s| (s.handed.clone(), s.orders.clone()))
                    .ok_or_else(|| Error::msg("The radio is off."))?;
                let ready = ready_orders(conn, &orders);
                let picked = pick_next(conn, &handed, &ready)?;
                if picked.is_empty() {
                    break;
                }
                picked
            }
        };
        for entry in entries {
            if let Some(item) = item_of(conn, entry, dir.as_deref())? {
                with_session(generation, |s| {
                    s.handed.push(entry);
                    if let Some(song) = entry.song() {
                        s.orders.iter_mut().filter(|o| o.song_id == song).for_each(|o| o.planned = true);
                    }
                });
                out.push(item);
            }
        }
    }
    drop(guard);
    wake().notify_one();
    changed(&app);
    Ok(out)
}

/// The player started or stopped: the radio orders only while it plays.
// Spec: RADIO-11
#[tauri::command]
pub async fn radio_playing(app: AppHandle, playing: bool) -> Result<()> {
    let differs = session().as_mut().is_some_and(|s| {
        let differs = s.playing != playing;
        s.playing = playing;
        differs
    });
    if differs {
        wake().notify_one();
        changed(&app);
    }
    Ok(())
}

/// Leaves the radio. What it was making is finished anyway: it is in the
/// library already.
#[tauri::command]
pub async fn radio_stop(app: AppHandle) -> Result<()> {
    let was_on = session().take().is_some();
    wake().notify_one();
    if was_on {
        changed(&app);
    }
    Ok(())
}

#[tauri::command]
pub async fn radio_state(db: State<'_, Db>) -> Result<RadioState> {
    Ok(state_of(&mut db.lock()))
}

#[tauri::command]
pub async fn radio_bridge_played(db: State<'_, Db>, bridge_id: BridgeId) -> Result<()> {
    bridge_played(&mut db.lock(), bridge_id)
}

// ---------------------------------------------------------------------------
// The session's loop
// ---------------------------------------------------------------------------

fn spawn_session(app: AppHandle, generation: u64) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                () = wake().notified() => {}
                () = tokio::time::sleep(TICK) => {}
            }
            let Some((playing, thinking, resting, planned, orders)) = with_session(generation, |s| {
                (
                    s.playing,
                    s.thinking,
                    s.rest_until.is_some_and(|until| Instant::now() < until),
                    s.planned.iter().filter(|e| matches!(e, Entry::Take(..))).count(),
                    s.orders.clone(),
                )
            }) else {
                return;
            };
            // What the radio is making: tell the screen when it moves on.
            let db = app.state::<Db>();
            let statuses: Vec<(SongId, String)> = {
                let mut guard = db.lock();
                orders
                    .iter()
                    .map(|o| {
                        let status = song_rows::table
                            .find(o.song_id)
                            .select(song_rows::status)
                            .first::<String>(&mut *guard)
                            .unwrap_or_else(|_| "failed".into());
                        (o.song_id, status)
                    })
                    .collect()
            };
            let moved = with_session(generation, |s| {
                let mut moved = false;
                for (song_id, status) in &statuses {
                    if let Some(order) = s.orders.iter_mut().find(|o| o.song_id == *song_id) {
                        if order.status != *status {
                            order.status.clone_from(status);
                            moved = true;
                        }
                    }
                }
                moved
            })
            .unwrap_or(false);
            // An order that has just become ready wakes the agent, to
            // introduce it.
            let newly_ready = statuses.iter().any(|(id, status)| {
                status == "ready" && orders.iter().any(|o| o.song_id == *id && !o.planned && o.status != "ready")
            });
            if moved {
                changed(&app);
            }
            if !playing || thinking || resting {
                continue;
            }
            if planned >= AHEAD && !newly_ready {
                continue;
            }
            with_session(generation, |s| s.thinking = true);
            changed(&app);
            let result = think(&app, &db, generation).await;
            with_session(generation, |s| {
                s.thinking = false;
                match &result {
                    Ok(()) => s.note = None,
                    Err(error) => {
                        s.note = Some(error.to_string());
                        s.rest_until = Some(Instant::now() + REST);
                    }
                }
            });
            if let Err(error) = result {
                crate::diagnostics::note("radio", format!("The radio's mixing agent failed: {error}"));
            }
            changed(&app);
        }
    });
}

// ---------------------------------------------------------------------------
// The mixing agent
// ---------------------------------------------------------------------------

const RULES: &str = "\
You run Polski Radio, a music station for someone learning Polish. You are \
its mixing agent and its presenter: you decide what plays, order new songs \
for it, now and then a podcast or the news, and you speak between every two \
items.

The programme:
- It is a music station. Songs are three or four minutes; a podcast or news \
episode is five to ten minutes of talk. Play at least three songs between \
two episodes, and open with a song. The app holds you to this: an episode \
too soon is refused, and you are told how many songs are still needed.
- Every song and episode comes after a bridge. Put the programme together as \
bridge, item, bridge, item: an item that would follow another item directly \
is refused.
- Keep a few songs or episodes planned with add_to_programme; you are told \
how many are.
- Mix the library: what has not played for a while, never the same item \
twice in a row. News is played only on the day it was made.

New material:
- What you order takes minutes and lands in the listener's library. You are \
told when it is ready: then put it on the programme, with a bridge that \
introduces it as new. Until then, play the library.
- Keep something new in the making while the listener listens, up to the \
number allowed at once. Order songs, mostly: a podcast or the news only \
after three new songs (the app holds you to this too), and the news at most \
once a day.
- Make new songs from what the listener is learning: name an open course by \
its id when it fits, so the song uses its words and is filed there. Draw \
podcast subjects from their interests.

Bridges:
- A bridge is one to three short lines, each a Polish sentence and its \
English, as a presenter speaks between records: introduce what comes next \
(role intro, about that item), or say something about the station, the time \
of day or what was just heard (role station, or role outro about the item \
that just played).
- Make them lively and warm, not a summary: a word or two from the next \
item's chorus or subject, so the bridge prepares the ear for it.
- Polish at the listener's level: short and clear. No ids, no markup, no \
emoji in what is said.
- Bridges are kept. Before writing one, look for a kept bridge that fits with \
find_bridges and reuse it by its id; write a new one when nothing fits or the \
kept ones have played often.

When you have planned enough, stop calling tools and answer with one short \
line on what you planned.";

fn tools() -> Vec<Tool> {
    let bridge = json!({
        "type": "object",
        "properties": {
            "role": { "type": "string", "enum": ["intro", "outro", "station"] },
            "song_id": { "type": "string", "description": "The song or episode it is about (sg_…); leave out for a station bridge." },
            "lines": {
                "type": "array",
                "maxItems": BRIDGE_LINES,
                "items": {
                    "type": "object",
                    "properties": { "pl": { "type": "string" }, "en": { "type": "string" } },
                    "required": ["pl", "en"]
                }
            }
        },
        "required": ["role", "lines"]
    });
    vec![
        tool(
            "list_library",
            "Looking through the library…",
            "The finished songs and podcast episodes in the library, least recently played first.",
            json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["all", "song", "podcast", "news"] },
                    "course_id": { "type": "string", "description": "Only what is filed under this course." },
                    "limit": { "type": "integer", "description": "At most this many (default 40)." }
                }
            }),
        ),
        tool(
            "find_bridges",
            "Looking for a bridge…",
            "Kept bridges, least played first: about one song or episode (song_id), or about the station (no song_id).",
            json!({
                "type": "object",
                "properties": {
                    "song_id": { "type": "string" },
                    "role": { "type": "string", "enum": ["intro", "outro", "station"] }
                }
            }),
        ),
        tool(
            "add_to_programme",
            "Planning the programme…",
            "Adds items to the end of the programme, in order. Each item is a song or episode (song_id), a kept \
             bridge (bridge_id), or a new bridge (bridge), which is spoken and kept.",
            json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "maxItems": 6,
                        "items": {
                            "type": "object",
                            "properties": {
                                "song_id": { "type": "string" },
                                "bridge_id": { "type": "string" },
                                "bridge": bridge
                            }
                        }
                    }
                },
                "required": ["items"]
            }),
        ),
        tool(
            "make_song",
            "Ordering a song…",
            "Orders a new song: the songwriter writes it from the learner's words and Suno sings it, in 2–4 minutes.",
            json!({
                "type": "object",
                "properties": {
                    "about": { "type": "string", "description": "What the song is about." },
                    "requirements": { "type": "string", "description": "Words or grammar it must use." },
                    "course_id": { "type": "string", "description": "An open course whose words it uses; it is filed there." },
                    "style": { "type": "string" },
                    "length": { "type": "string", "enum": ["short", "medium", "long"] }
                },
                "required": ["about"]
            }),
        ),
        tool(
            "make_podcast",
            "Ordering a podcast…",
            "Orders a spoken episode about a subject, from Wikipedia articles or as a made-up story, ready in a few minutes.",
            json!({
                "type": "object",
                "properties": {
                    "about": { "type": "string" },
                    "articles": { "type": "array", "items": { "type": "string" }, "description": "English Wikipedia titles, at most 5." },
                    "story": { "type": "boolean" },
                    "course_id": { "type": "string" },
                    "length": { "type": "string", "enum": ["short", "medium", "long"] }
                },
                "required": ["about"]
            }),
        ),
        tool(
            "make_news",
            "Ordering the news…",
            "Orders a news episode from today's Polish headlines, ready in a few minutes.",
            json!({
                "type": "object",
                "properties": { "about": { "type": "string", "description": "A subject to lean towards; empty for the day's news." } }
            }),
        ),
    ]
}

fn parse<T: serde::de::DeserializeOwned>(arguments: &str) -> Result<T> {
    serde_json::from_str(if arguments.trim().is_empty() { "{}" } else { arguments })
        .map_err(|e| Error::msg(format!("The arguments did not fit: {e}")))
}

/// What the agent is told before each turn.
fn briefing(conn: &mut Conn, generation: u64) -> Result<String> {
    let (handed, planned, orders) = with_session(generation, |s| {
        (s.handed.clone(), s.planned.iter().copied().collect::<Vec<_>>(), s.orders.clone())
    })
    .ok_or_else(|| Error::msg("The radio is off."))?;
    let describe = |conn: &mut Conn, entry: Entry| -> String {
        match entry {
            Entry::Take(song_id, _) => {
                let row: Option<(String, String, String)> = song_rows::table
                    .find(song_id)
                    .select((song_rows::title, song_rows::kind, song_rows::chorus_pl))
                    .first(conn)
                    .optional()
                    .ok()
                    .flatten();
                match row {
                    Some((title, kind, chorus)) => format!("{kind} {song_id}: “{title}” — {chorus}"),
                    None => format!("{song_id} (deleted)"),
                }
            }
            Entry::Bridge(id) => {
                let first = cast_title(conn, id).ok().flatten().map(|t| t.1).unwrap_or_default();
                format!("bridge {id}: {first}")
            }
        }
    };
    let mut out = String::new();
    // The learner's local time, as the rest of the app keeps it (db::today).
    let offset = db::get_setting_i64(conn, "tz_offset_minutes", 0) * 60;
    let day = db::today(conn);
    let local = db::now_secs() + offset;
    let (year, month, date) = crate::commands::civil_date(day);
    const WEEKDAYS: [&str; 7] = ["Thursday", "Friday", "Saturday", "Sunday", "Monday", "Tuesday", "Wednesday"];
    let weekday = WEEKDAYS[usize::try_from(day.rem_euclid(7)).unwrap_or(0)];
    let minutes = local.rem_euclid(86_400) / 60;
    out.push_str(&format!("It is {weekday} {year}-{month:02}-{date:02}, {:02}:{:02}.\n", minutes / 60, minutes % 60));
    let recent: Vec<String> = handed.iter().rev().take(10).rev().map(|e| describe(conn, *e)).collect();
    out.push_str(&format!(
        "\nPlayed or playing now, oldest first:\n{}\n",
        if recent.is_empty() { "  (nothing yet: the station just started)".to_string() } else { format!("  - {}", recent.join("\n  - ")) }
    ));
    let planned_lines: Vec<String> = planned.iter().map(|e| describe(conn, *e)).collect();
    let takes = planned.iter().filter(|e| matches!(e, Entry::Take(..))).count();
    out.push_str(&format!(
        "\nOn the programme, not played yet ({takes} songs or episodes; keep {AHEAD} planned):\n{}\n",
        if planned_lines.is_empty() { "  (nothing)".to_string() } else { format!("  - {}", planned_lines.join("\n  - ")) }
    ));
    let sequence: Vec<Entry> = handed.iter().chain(planned.iter()).copied().collect();
    let next_is = match sequence.last() {
        Some(Entry::Take(..)) => "The programme ends with a song or episode: what you add next starts with a bridge.",
        Some(Entry::Bridge(_)) => "The programme ends with a bridge: what you add next starts with the item it introduces.",
        None => "Nothing has played yet: start with a bridge that welcomes the listener, then a song.",
    };
    let songs = songs_since_episode(conn, &sequence)?;
    let episode = if episode_may_follow(conn, &sequence)? {
        "a podcast or news episode may come next".to_string()
    } else {
        format!("{} more song(s) before a podcast or news episode", SONGS_PER_EPISODE - songs.min(SONGS_PER_EPISODE))
    };
    out.push_str(&format!("{next_is}\nSongs since the last episode: {songs}; {episode}.\n"));
    let songs_ordered = orders.iter().rev().take_while(|o| o.what == "song").count();
    out.push_str(&format!(
        "Songs you ordered since the last podcast or news you ordered: {songs_ordered} (an episode may be ordered after {SONGS_PER_EPISODE}).\n"
    ));
    let mut making = Vec::new();
    let mut ready = Vec::new();
    for order in orders.iter().filter(|o| !o.planned) {
        let row: Option<(String, String)> = song_rows::table
            .find(order.song_id)
            .select((song_rows::title, song_rows::status))
            .first(conn)
            .optional()?;
        match row {
            Some((title, status)) if status == "ready" => {
                ready.push(format!("{} {}: “{title}” — READY: put it on the programme", order.what, order.song_id));
            }
            Some((title, status)) if status == "generating" => {
                making.push(format!("{} {}: “{title}” — being made", order.what, order.song_id));
            }
            Some((title, _)) => making.push(format!("{} {}: “{title}” — failed", order.what, order.song_id)),
            None => {}
        }
    }
    let in_flight = making.iter().filter(|m| m.ends_with("being made")).count();
    out.push_str(&format!(
        "\nNew material you ordered:\n{}\nYou may have {MAKING} being made at once; {in_flight} are now.\n",
        if making.is_empty() && ready.is_empty() {
            "  (none yet)".to_string()
        } else {
            format!("  - {}", ready.iter().chain(making.iter()).cloned().collect::<Vec<_>>().join("\n  - "))
        }
    ));
    let since = day * 86_400 - offset;
    let news_today: Vec<SongId> = song_rows::table
        .filter(song_rows::kind.eq("podcast"))
        .filter(song_rows::created_at.ge(since))
        .filter(song_rows::origin.like("%\"news\":true%"))
        .select(song_rows::id)
        .load(conn)?;
    out.push_str(&match news_today.first() {
        Some(id) => format!("\nToday's news already exists: {id}.\n"),
        None => "\nThere is no news episode from today yet.\n".to_string(),
    });
    let open: Vec<(CourseId, String, String, Option<String>)> = courses::table
        .filter(courses::closed_at.is_null())
        .filter(courses::slug.ne(crate::courses::OTHER))
        .order(crate::lessons::course_order())
        .limit(20)
        .select((courses::id, courses::title_en, courses::title_pl, courses::level))
        .load(conn)?;
    out.push_str("\nThe learner's open courses:\n");
    if open.is_empty() {
        out.push_str("  (none)\n");
    }
    for (id, title, title_pl, level) in open {
        out.push_str(&format!("  - {id}: {title} ({title_pl}), {}\n", level.unwrap_or_else(|| "level not set".into())));
    }
    let interests = crate::podcasts::interests(conn)?;
    if !interests.is_empty() {
        out.push_str(&format!("\nWhat is known about the learner:\n  - {}\n", interests.join("\n  - ")));
    }
    let can_sing = songs::kie_key(conn).is_ok();
    out.push_str(&format!(
        "\nBridges are spoken in the {} format.{}\n",
        format(conn),
        if can_sing { "" } else { " There is no kie.ai key, so songs cannot be made: order podcasts and news instead." }
    ));
    Ok(out)
}

/// One turn of the mixing agent: it plans what plays and orders what is made.
// Spec: RADIO-2
async fn think(app: &AppHandle, db: &Db, generation: u64) -> Result<()> {
    let brief = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        // Without a key the station still plays, from the library.
        ai::api_key(conn).map_err(|_| Error::msg("Add an OpenRouter key in Settings for the radio to make new things."))?;
        ai::require_model(conn)?;
        briefing(conn, generation)?
    };
    let wire = vec![Wire::system(RULES), Wire::user(brief)];
    let agent = Agent {
        app,
        db,
        name: crate::costs::RADIO,
        stream_id: "radio",
        tools: Some(tools()),
        max_rounds: MAX_ROUNDS,
        stop_once_answered: false,
        show_status: false,
        model: None,
        memory: false,
    };
    let outcome = agent
        .run::<(), _>(wire, |call| {
            Box::pin(async move {
                let result = run_tool(app, db, generation, &call.function.name, &call.function.arguments)
                    .await
                    .unwrap_or_else(|error| json!({ "error": error.to_string() }));
                ToolOutput::reply(result)
            })
        })
        .await;
    match outcome.failed {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn run_tool(app: &AppHandle, db: &Db, generation: u64, name: &str, arguments: &str) -> Result<Value> {
    if current_generation() != Some(generation) {
        return Err(Error::msg("The radio was switched off."));
    }
    match name {
        "list_library" => list_library(&mut db.lock(), arguments),
        "find_bridges" => find_bridges(&mut db.lock(), arguments),
        "add_to_programme" => add_to_programme(app, db, generation, arguments).await,
        "make_song" | "make_podcast" | "make_news" => order(app, db, generation, name, arguments).await,
        other => Err(Error::msg(format!("There is no tool called {other}."))),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LibraryArgs {
    kind: String,
    course_id: Option<CourseId>,
    limit: Option<usize>,
}

fn list_library(conn: &mut Conn, arguments: &str) -> Result<Value> {
    let args: LibraryArgs = parse(arguments)?;
    let played: Vec<(SongId, Option<i64>, i64)> = song_tracks::table
        .select((song_tracks::song_id, song_tracks::last_played_at, song_tracks::play_count))
        .load(conn)?;
    let now = db::now_secs();
    let today = day_start(conn);
    let mut rows: Vec<(i64, Value)> = songs::list_songs(conn, false)?
        .into_iter()
        .filter(|s| s.status == "ready" && !s.tracks.is_empty())
        // Yesterday's news is not played (RADIO-19).
        .filter(|s| !(s.origin.news && s.created_at < today))
        .filter(|s| args.course_id.is_none() || s.course_id == args.course_id)
        .filter(|s| match args.kind.as_str() {
            "song" => s.kind == "song",
            "podcast" => s.kind == "podcast" && !s.origin.news,
            "news" => s.origin.news,
            _ => true,
        })
        .map(|s| {
            let mine = played.iter().filter(|p| p.0 == s.id);
            let last = mine.clone().filter_map(|p| p.1).max();
            let plays: i64 = mine.map(|p| p.2).sum();
            let kind = if s.origin.news { "news" } else { s.kind.as_str() };
            (
                last.unwrap_or(0),
                json!({
                    "id": s.id,
                    "kind": kind,
                    "title": s.title,
                    "about": format!("{} — {}", s.chorus_pl, s.chorus_en),
                    "course_id": s.course_id,
                    "made_by": songs::made_by(&s.origin.made_by),
                    "plays": plays,
                    "last_played_days_ago": last.map(|t| (now - t) / 86_400),
                    "made_days_ago": (now - s.created_at) / 86_400,
                    "minutes": (s.tracks.iter().map(|t| t.duration).fold(0.0, f64::max) / 60.0).round(),
                }),
            )
        })
        .collect();
    rows.sort_by_key(|r| r.0);
    let total = rows.len();
    let list: Vec<Value> = rows.into_iter().take(args.limit.unwrap_or(40).clamp(1, 100)).map(|r| r.1).collect();
    Ok(json!({ "total": total, "items": list }))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct FindArgs {
    song_id: Option<SongId>,
    role: Option<String>,
}

// Spec: RADIO-7
fn find_bridges(conn: &mut Conn, arguments: &str) -> Result<Value> {
    let args: FindArgs = parse(arguments)?;
    let found = kept_bridges(conn, args.song_id, args.role.as_deref(), 30)?;
    Ok(json!({
        "bridges": found.into_iter().map(|b| json!({
            "id": b.id,
            "role": b.role,
            "song_id": b.song_id,
            "lines": b.lines,
            "plays": b.play_count,
        })).collect::<Vec<_>>()
    }))
}

#[derive(Deserialize)]
struct NewBridge {
    role: String,
    #[serde(default)]
    song_id: Option<SongId>,
    lines: Vec<BridgeLine>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ProgrammeArg {
    song_id: Option<SongId>,
    bridge_id: Option<BridgeId>,
    bridge: Option<NewBridge>,
}

#[derive(Deserialize)]
struct ProgrammeArgs {
    items: Vec<ProgrammeArg>,
}

/// Whether a song or episode may go on the programme next, and if not, why
/// (for the agent).
// Spec: RADIO-17, RADIO-19
fn mix_allows(conn: &mut Conn, generation: u64, song_id: SongId) -> std::result::Result<(), String> {
    let kind = kind_of(conn, song_id).map_err(|e| e.to_string())?;
    if kind == Kind::OldNews {
        return Err(format!("{song_id} is news from an earlier day; news plays only on its day."));
    }
    if kind == Kind::Song {
        return Ok(());
    }
    let sequence: Vec<Entry> = with_session(generation, |s| s.handed.iter().chain(s.planned.iter()).copied().collect())
        .unwrap_or_default();
    let songs = songs_since_episode(conn, &sequence).map_err(|e| e.to_string())?;
    if episode_may_follow(conn, &sequence).map_err(|e| e.to_string())? {
        Ok(())
    } else {
        Err(format!(
            "{song_id} is a spoken episode, and this is a music station: put {} more song(s) on first.",
            SONGS_PER_EPISODE - songs
        ))
    }
}

/// Whether a song or episode may go on next: only after a bridge.
// Spec: RADIO-20
fn bridged(generation: u64, song_id: SongId) -> std::result::Result<(), String> {
    let last = with_session(generation, |s| s.planned.back().or(s.handed.last()).copied()).flatten();
    match last {
        Some(Entry::Take(..)) => Err(format!(
            "{song_id} would follow a song or episode directly: put a bridge before it (a kept one or a new one)."
        )),
        _ => Ok(()),
    }
}

async fn add_to_programme(app: &AppHandle, db: &Db, generation: u64, arguments: &str) -> Result<Value> {
    let args: ProgrammeArgs = parse(arguments)?;
    let mut added = Vec::new();
    let mut problems = Vec::new();
    for item in args.items.into_iter().take(6) {
        let entry = match item {
            ProgrammeArg { song_id: Some(song_id), .. } => {
                let mut guard = db.lock();
                let conn = &mut *guard;
                match take_of(conn, song_id)? {
                    Some(track) => mix_allows(conn, generation, song_id)
                        .and_then(|()| bridged(generation, song_id))
                        .map(|()| Entry::Take(song_id, track)),
                    None => Err(format!("{song_id} is not ready to play.")),
                }
            }
            ProgrammeArg { bridge_id: Some(id), .. } => match load_bridge(&mut db.lock(), id)? {
                Some(_) => Ok(Entry::Bridge(id)),
                None => Err(format!("There is no bridge {id}.")),
            },
            ProgrammeArg { bridge: Some(bridge), .. } => {
                let role = match bridge.role.as_str() {
                    "intro" | "outro" | "station" => bridge.role.clone(),
                    _ => "station".to_string(),
                };
                let about = if role == "station" { None } else { bridge.song_id };
                make_bridge(app, db, about, &role, bridge.lines).await.map(Entry::Bridge).map_err(|e| e.to_string())
            }
            _ => Err("An item needs a song_id, a bridge_id or a bridge.".to_string()),
        };
        match entry {
            Ok(entry) => {
                let added_now = with_session(generation, |s| {
                    s.planned.push_back(entry);
                    if let Some(song) = entry.song() {
                        s.orders.iter_mut().filter(|o| o.song_id == song).for_each(|o| o.planned = true);
                    }
                });
                if added_now.is_none() {
                    return Err(Error::msg("The radio was switched off."));
                }
                added.push(match entry {
                    Entry::Take(song, _) => song.to_string(),
                    Entry::Bridge(id) => id.to_string(),
                });
            }
            Err(problem) => problems.push(problem),
        }
    }
    changed(app);
    let planned = with_session(generation, |s| s.planned.iter().filter(|e| matches!(e, Entry::Take(..))).count()).unwrap_or(0);
    Ok(json!({ "added": added, "problems": problems, "songs_and_episodes_planned": planned }))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OrderArgs {
    about: String,
    requirements: String,
    course_id: Option<CourseId>,
    style: String,
    length: String,
    articles: Vec<String>,
    story: bool,
}

/// Orders a new song, episode or news episode, made like any other and kept
/// in the library, marked as the radio's and filed under its course.
// Spec: RADIO-1, RADIO-3, RADIO-11, RADIO-15, RADIO-16
async fn order(app: &AppHandle, db: &Db, generation: u64, name: &str, arguments: &str) -> Result<Value> {
    let args: OrderArgs = parse(arguments)?;
    let (playing, in_flight) = {
        let orders = with_session(generation, |s| (s.playing, s.orders.clone())).ok_or_else(|| Error::msg("The radio was switched off."))?;
        let mut guard = db.lock();
        let making = orders
            .1
            .iter()
            .filter(|o| {
                song_rows::table
                    .find(o.song_id)
                    .select(song_rows::status)
                    .first::<String>(&mut *guard)
                    .is_ok_and(|s| s == "generating")
            })
            .count();
        (orders.0, making)
    };
    if !playing {
        return Err(Error::msg("The radio is paused: nothing new is ordered until it plays again."));
    }
    if in_flight >= MAKING {
        return Err(Error::msg(format!("{in_flight} items are being made already; wait until one is ready.")));
    }
    // Spec: RADIO-18
    if name != "make_song" {
        let songs_ordered = with_session(generation, |s| s.orders.iter().rev().take_while(|o| o.what == "song").count())
            .unwrap_or(0);
        let can_sing = songs::kie_key(&mut db.lock()).is_ok();
        if can_sing && songs_ordered < SONGS_PER_EPISODE {
            return Err(Error::msg(format!(
                "This is a music station: order {} more song(s) before another podcast or news.",
                SONGS_PER_EPISODE - songs_ordered
            )));
        }
    }
    let course_id = match args.course_id {
        Some(id) => {
            let mut guard = db.lock();
            crate::courses::require_open_course(&mut guard, id).ok().map(|_| id)
        }
        None => None,
    };
    let (song_id, what) = match name {
        "make_song" => {
            songs::kie_key(&mut db.lock())?;
            let draft = songs::write_draft(
                db,
                songs::DraftRequest {
                    brief: args.about,
                    requirements: args.requirements,
                    made_by: "radio".into(),
                    style: args.style,
                    length: args.length,
                    ..Default::default()
                },
            )
            .await?;
            (songs::start_song(app, db, draft)?, "song")
        }
        _ => {
            let news = name == "make_news";
            let format = format(&mut db.lock());
            let request = crate::podcasts::PodcastRequest {
                topic: args.about,
                articles: args.articles.into_iter().take(crate::podcasts::ARTICLE_LIMIT).collect(),
                story: args.story && !news,
                news,
                format,
                length: args.length,
                requirements: args.requirements,
                made_by: "radio".into(),
                ..Default::default()
            };
            (crate::podcasts::start_from_request(app, db, request)?, if news { "news" } else { "podcast" })
        }
    };
    if let Some(course_id) = course_id.filter(|_| what != "news") {
        diesel::update(song_rows::table.find(song_id))
            .set(song_rows::course_id.eq(course_id))
            .execute(&mut *db.lock())?;
    }
    let title: String = song_rows::table.find(song_id).select(song_rows::title).first(&mut *db.lock())?;
    with_session(generation, |s| {
        s.orders.push(Order { song_id, what, planned: false, status: "generating".into() });
    });
    changed(app);
    Ok(json!({
        "id": song_id,
        "title": title,
        "status": "being made; you are told when it is ready",
    }))
}

/// What the screens are given, for the shapes check (lib.rs).
#[cfg(test)]
pub(crate) fn shapes(conn: &mut Conn) -> (RadioState, Vec<ProgrammeItem>) {
    let id = BridgeId::new();
    let file = files::store(conn, files::BRIDGE_AUDIO, "bridge-shape.mp3", "audio/mpeg", b"bridge").unwrap();
    let lines = [BridgeLine { pl: "Słuchasz Polskiego Radia.".into(), en: "You are listening to Polski Radio.".into() }];
    let timed = [TimedLine {
        start: 0.6,
        end: 2.0,
        text: lines[0].pl.clone(),
        lang: "pl".into(),
        gloss: Some(lines[0].en.clone()),
        section: None,
        speaker: None,
    }];
    insert_bridge(conn, id, None, "station", &lines, "pairs", "Kore", file, 2.4, &timed).unwrap();
    let items = item_of(conn, Entry::Bridge(id), None).unwrap().into_iter().collect();
    (state_of(conn), items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Conn {
        let path = std::env::temp_dir()
            .join("learn-polish-tests")
            .join(format!("radio-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        db::open(&path).expect("open")
    }

    /// A finished song with one take, last played `played` seconds after 0.
    fn song(conn: &mut Conn, title: &str, played: Option<i64>) -> (SongId, TrackId) {
        let draft = songs::SongDraft { title: title.into(), chorus_pl: "Dzień dobry".into(), ..Default::default() };
        let id = songs::insert_song(conn, &draft, "V6").unwrap();
        songs::set_status(conn, id, "ready", None).unwrap();
        let file = files::store(conn, files::SONG_AUDIO, &format!("{title}.mp3"), "audio/mpeg", title.as_bytes()).unwrap();
        let track = TrackId::new();
        diesel::insert_into(song_tracks::table)
            .values((
                song_tracks::id.eq(track),
                song_tracks::song_id.eq(id),
                song_tracks::ord.eq(0),
                song_tracks::remote_id.eq("take"),
                song_tracks::audio_file_id.eq(file),
                song_tracks::last_played_at.eq(played),
            ))
            .execute(conn)
            .unwrap();
        (id, track)
    }

    fn bridge(conn: &mut Conn, about: Option<SongId>, role: &str) -> BridgeId {
        let id = BridgeId::new();
        let file = files::store(conn, files::BRIDGE_AUDIO, &format!("bridge-{id}.mp3"), "audio/mpeg", b"bridge").unwrap();
        let lines = [BridgeLine { pl: "A teraz piosenka.".into(), en: "And now a song.".into() }];
        insert_bridge(conn, id, about, role, &lines, "pairs", "Kore", file, 2.0, &[]).unwrap();
        id
    }

    // Spec: RADIO-8
    #[test]
    fn bridges_go_with_their_song() {
        let mut conn = scratch("cascade");
        let (gone, _) = song(&mut conn, "Gone", None);
        let (kept, _) = song(&mut conn, "Kept", None);
        let about_gone = bridge(&mut conn, Some(gone), "intro");
        let about_kept = bridge(&mut conn, Some(kept), "outro");
        let station = bridge(&mut conn, None, "station");
        let file: FileId = bridges::table.find(about_gone).select(bridges::audio_file_id).first(&mut conn).unwrap();

        songs::delete_song(&mut conn, gone).unwrap();

        assert!(load_bridge(&mut conn, about_gone).unwrap().is_none());
        assert!(load_bridge(&mut conn, about_kept).unwrap().is_some());
        assert!(load_bridge(&mut conn, station).unwrap().is_some());
        assert!(files::info(&mut conn, file).is_err(), "the bridge's audio goes too");
    }

    // Spec: RADIO-6
    #[test]
    fn bridges_never_reach_the_library() {
        let mut conn = scratch("library");
        let (id, _) = song(&mut conn, "Only song", None);
        let before = songs::list_songs(&mut conn, true).unwrap().len();
        bridge(&mut conn, Some(id), "intro");
        bridge(&mut conn, None, "station");
        let library = songs::list_songs(&mut conn, true).unwrap();
        assert_eq!(library.len(), before);
        let song = library.iter().find(|s| s.id == id).unwrap();
        assert_eq!(song.tracks.len(), 1);
    }

    // Spec: RADIO-3
    #[test]
    fn what_the_radio_made_is_marked_so() {
        assert_eq!(songs::made_by("radio"), "radio");
        assert_eq!(songs::made_by("tutor"), "tutor");
        assert_eq!(songs::made_by("anyone"), "composer");
    }

    // Spec: RADIO-12, RADIO-14
    #[test]
    fn without_a_plan_the_longest_unplayed_comes_next() {
        let mut conn = scratch("fallback");
        let (recent, recent_take) = song(&mut conn, "Recent", Some(300));
        let (old, old_take) = song(&mut conn, "Old", Some(100));
        let (never, never_take) = song(&mut conn, "Never", None);

        // Never played comes first; then, with it just handed, the oldest.
        assert_eq!(pick_next(&mut conn, &[], &[]).unwrap(), vec![Entry::Take(never, never_take)]);
        let handed = [Entry::Take(never, never_take)];
        assert_eq!(pick_next(&mut conn, &handed, &[]).unwrap(), vec![Entry::Take(old, old_take)]);

        // A new item the radio made comes before anything else.
        assert_eq!(pick_next(&mut conn, &handed, &[recent]).unwrap(), vec![Entry::Take(recent, recent_take)]);

        // A kept bridge about the item introduces it.
        let intro = bridge(&mut conn, Some(old), "intro");
        assert_eq!(
            pick_next(&mut conn, &handed, &[]).unwrap(),
            vec![Entry::Bridge(intro), Entry::Take(old, old_take)]
        );
        // …but not the same bridge again straight away.
        let again = [Entry::Take(never, never_take), Entry::Bridge(intro), Entry::Take(old, old_take), Entry::Take(never, never_take)];
        let next = pick_next(&mut conn, &again, &[]).unwrap();
        assert!(!next.contains(&Entry::Bridge(intro)));
    }

    #[test]
    fn a_small_library_still_plays() {
        let mut conn = scratch("small");
        let (only, take) = song(&mut conn, "Only", Some(5));
        let handed = [Entry::Take(only, take)];
        assert_eq!(pick_next(&mut conn, &handed, &[]).unwrap(), vec![Entry::Take(only, take)]);
        let mut empty = scratch("empty");
        assert!(pick_next(&mut empty, &[], &[]).unwrap().is_empty());
    }

    /// A finished podcast (or news) episode.
    fn episode(conn: &mut Conn, title: &str, news: bool, made_at: i64) -> (SongId, TrackId) {
        let (id, track) = song(conn, title, None);
        let origin = serde_json::to_string(&songs::SongOrigin { news, ..Default::default() }).unwrap();
        diesel::update(song_rows::table.find(id))
            .set((song_rows::kind.eq("podcast"), song_rows::origin.eq(origin), song_rows::created_at.eq(made_at)))
            .execute(conn)
            .unwrap();
        (id, track)
    }

    fn take(entries: &[Entry]) -> Option<Entry> {
        entries.iter().copied().find(|e| matches!(e, Entry::Take(..)))
    }

    // Spec: RADIO-17, RADIO-19
    #[test]
    fn music_first_and_only_todays_news() {
        let mut conn = scratch("mix");
        db::set_setting(&mut conn, "tz_offset_minutes", "0").unwrap();
        // Songs that have played; an episode and old news that never have,
        // so on age alone they would come first.
        let a = song(&mut conn, "A", Some(10));
        let b = song(&mut conn, "B", Some(20));
        let c = song(&mut conn, "C", Some(30));
        let talk = episode(&mut conn, "Talk", false, db::now_secs());
        let old_news = episode(&mut conn, "Old news", true, db::now_secs() - 3 * 86_400);
        let as_entry = |(s, t): (SongId, TrackId)| Entry::Take(s, t);

        // The station opens with a song.
        assert_eq!(take(&pick_next(&mut conn, &[], &[]).unwrap()), Some(as_entry(a)));
        // Two songs are not enough for talk; three are.
        let two = [as_entry(a), as_entry(b)];
        assert_eq!(take(&pick_next(&mut conn, &two, &[]).unwrap()), Some(as_entry(c)));
        let three = [as_entry(a), as_entry(b), as_entry(c)];
        assert_eq!(take(&pick_next(&mut conn, &three, &[]).unwrap()), Some(as_entry(talk)));
        // After the episode, songs again; yesterday's news never.
        let after = [as_entry(a), as_entry(b), as_entry(c), as_entry(talk)];
        let next = take(&pick_next(&mut conn, &after, &[]).unwrap()).unwrap();
        assert!(next != as_entry(old_news) && next != as_entry(talk));
        assert!(episode_may_follow(&mut conn, &three).unwrap());
        assert!(!episode_may_follow(&mut conn, &after).unwrap());
        // A ready episode the radio made waits for its songs too.
        assert_eq!(take(&pick_next(&mut conn, &two, &[talk.0]).unwrap()), Some(as_entry(c)));
    }

    #[test]
    fn talk_plays_when_there_are_no_songs() {
        let mut conn = scratch("talk-only");
        let talk = episode(&mut conn, "Talk", false, db::now_secs());
        assert_eq!(take(&pick_next(&mut conn, &[], &[]).unwrap()), Some(Entry::Take(talk.0, talk.1)));
    }

    // Spec: RADIO-20
    #[test]
    fn every_item_comes_after_a_bridge() {
        let mut conn = scratch("bridged");
        let a = song(&mut conn, "A", Some(10));
        let b = song(&mut conn, "B", Some(20));
        let first = bridge(&mut conn, None, "station");
        let second = bridge(&mut conn, None, "station");
        let played = [Entry::Take(a.0, a.1)];
        let next = pick_next(&mut conn, &played, &[]).unwrap();
        assert!(matches!(next.as_slice(), [Entry::Bridge(_), Entry::Take(..)]), "{next:?}");
        // The bridge just heard is not said again: the other one is.
        let Entry::Bridge(said) = next[0] else { unreachable!() };
        let again = [Entry::Take(a.0, a.1), Entry::Bridge(said), Entry::Take(b.0, b.1)];
        let next = pick_next(&mut conn, &again, &[]).unwrap();
        let other = if said == first { second } else { first };
        assert_eq!(next[0], Entry::Bridge(other));
        // Right after a bridge, no second one.
        let after_bridge = [Entry::Take(a.0, a.1), Entry::Bridge(said)];
        assert!(matches!(pick_next(&mut conn, &after_bridge, &[]).unwrap().as_slice(), [Entry::Take(..)]));
    }

    // Spec: RADIO-5
    #[test]
    fn bridges_are_short() {
        let line = |pl: &str| BridgeLine { pl: pl.into(), en: "x".into() };
        assert!(clean_lines(vec![]).is_err());
        assert!(clean_lines(vec![line("  ")]).is_err());
        assert!(clean_lines(vec![line("a"), line("b"), line("c"), line("d")]).is_err());
        assert_eq!(clean_lines(vec![line(" Cześć! "), line("")]).unwrap(), vec![line("Cześć!")]);
    }

    #[test]
    fn formats_default_to_pairs() {
        let mut conn = scratch("format");
        assert_eq!(format(&mut conn), "pairs");
        db::set_setting(&mut conn, FORMAT_SETTING, "immersion").unwrap();
        assert_eq!(format(&mut conn), "immersion");
        db::set_setting(&mut conn, FORMAT_SETTING, "nonsense").unwrap();
        assert_eq!(format(&mut conn), "pairs");
    }

    #[test]
    fn programme_items_name_their_kind() {
        let mut conn = scratch("items");
        let (id, take) = song(&mut conn, "Named", None);
        let brg = bridge(&mut conn, None, "station");
        let take = serde_json::to_value(item_of(&mut conn, Entry::Take(id, take), None).unwrap().unwrap()).unwrap();
        assert_eq!(take["kind"], "take");
        assert_eq!(take["song"]["title"], "Named");
        let bridge = serde_json::to_value(item_of(&mut conn, Entry::Bridge(brg), None).unwrap().unwrap()).unwrap();
        assert_eq!(bridge["kind"], "bridge");
        assert_eq!(bridge["bridge"]["lines"][0]["pl"], "A teraz piosenka.");
    }
}
