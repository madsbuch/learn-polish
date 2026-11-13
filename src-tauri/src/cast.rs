//! Playing songs on Sonos speakers over the local network.
//!
//! Sonos speakers are UPnP renderers, so no Sonos account or cloud API is
//! involved. The app
//!  1. finds speakers with an SSDP search and reads the room/group layout,
//!  2. serves the downloaded songs over HTTP on the phone's Wi-Fi address,
//!  3. loads them into the speaker's own queue and lets Sonos play them, so
//!     next/repeat keep working even while this app sits in the background.
//!
//! The same HTTP server answers the phone's lock-screen player on 127.0.0.1,
//! which is how media controls reach the speaker without the web layer.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::db::{self, Db};
use crate::error::{Error, Result};

const SSDP_ADDR: &str = "239.255.255.250:1900";
const SONOS_ST: &str = "urn:schemas-upnp-org:device:ZonePlayer:1";
const AV_TRANSPORT: (&str, &str) = (
    "/MediaRenderer/AVTransport/Control",
    "urn:schemas-upnp-org:service:AVTransport:1",
);
const GROUP_RENDERING: (&str, &str) = (
    "/MediaRenderer/GroupRenderingControl/Control",
    "urn:schemas-upnp-org:service:GroupRenderingControl:1",
);
const ZONE_TOPOLOGY: (&str, &str) = (
    "/ZoneGroupTopology/Control",
    "urn:schemas-upnp-org:service:ZoneGroupTopology:1",
);
const POLL_INTERVAL: Duration = Duration::from_millis(1000);

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(6))
            // Speakers are on the LAN; never route them through a proxy.
            .no_proxy()
            .build()
            .expect("failed to build HTTP client")
    })
}

fn speaker_error(error: reqwest::Error) -> Error {
    Error::msg(format!("Could not reach the speaker: {error}"))
}

// ---------------------------------------------------------------------------
// Types shared with the frontend
// ---------------------------------------------------------------------------

/// A Sonos group (or single room) that can play music.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CastDevice {
    /// The coordinator's id, e.g. RINCON_000E58A0B1C201400.
    pub id: String,
    /// "Living Room", or "Living Room + 2" for a group.
    pub name: String,
    /// The coordinator's control address, e.g. http://192.168.1.20:1400.
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CastItem {
    pub track_id: i64,
    pub title: String,
    pub artist: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CastState {
    pub device: CastDevice,
    pub items: Vec<CastItem>,
    /// Index into `items` of the song Sonos is on.
    pub index: i64,
    pub track_id: Option<i64>,
    pub playing: bool,
    /// Raw UPnP state: PLAYING, PAUSED_PLAYBACK, STOPPED, TRANSITIONING.
    pub transport: String,
    pub position: f64,
    pub duration: f64,
    pub volume: i64,
    pub repeat: String,
    /// True when the speaker has moved on to something that is not ours.
    pub detached: bool,
}

/// What the phone's native player needs to talk to this module.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlInfo {
    pub port: u16,
    pub token: String,
}

// ---------------------------------------------------------------------------
// Small XML helpers. Sonos replies are flat and predictable, so a full XML
// parser would be more code than it saves.
// ---------------------------------------------------------------------------

pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

pub fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Text inside the first `<tag>…</tag>`, ignoring any namespace prefix.
pub fn tag_text(xml: &str, tag: &str) -> Option<String> {
    let mut search = 0;
    while let Some(rel) = xml[search..].find('<') {
        let start = search + rel;
        let rest = &xml[start + 1..];
        let name_end = rest.find(|c: char| c == '>' || c == ' ' || c == '/')?;
        let name = &rest[..name_end];
        let local = name.rsplit(':').next().unwrap_or(name);
        if local == tag && !name.starts_with('/') {
            let open_end = start + 1 + rest.find('>')?;
            if xml[..open_end].ends_with('/') {
                return Some(String::new());
            }
            let body_start = open_end + 1;
            let close = format!("</{name}>");
            let body_end = body_start + xml[body_start..].find(&close)?;
            return Some(xml_unescape(&xml[body_start..body_end]));
        }
        search = start + 1;
    }
    None
}

/// Attributes of every `<tag …>` element, in document order.
pub fn elements(xml: &str, tag: &str) -> Vec<HashMap<String, String>> {
    let mut found = Vec::new();
    let needle = format!("<{tag}");
    let mut search = 0;
    while let Some(rel) = xml[search..].find(&needle) {
        let start = search + rel + needle.len();
        search = start;
        // Must be the whole tag name, not a prefix of a longer one.
        if !xml[start..].starts_with([' ', '>', '/', '\t', '\n']) {
            continue;
        }
        let Some(end) = xml[start..].find('>') else { break };
        let mut attrs = HashMap::new();
        let mut rest = &xml[start..start + end];
        while let Some(eq) = rest.find("=\"") {
            let key = rest[..eq].trim().trim_start_matches('/').to_string();
            let after = &rest[eq + 2..];
            let Some(close) = after.find('"') else { break };
            attrs.insert(key, xml_unescape(&after[..close]));
            rest = &after[close + 1..];
        }
        found.push(attrs);
    }
    found
}

/// "0:03:04" → 184.0. Sonos reports NOT_IMPLEMENTED for streams.
pub fn parse_hms(text: &str) -> f64 {
    let parts: Vec<f64> = text
        .split(':')
        .map(|p| p.trim().parse::<f64>().unwrap_or(f64::NAN))
        .collect();
    if parts.iter().any(|p| p.is_nan()) {
        return 0.0;
    }
    parts.iter().fold(0.0, |total, part| total * 60.0 + part)
}

pub fn format_hms(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    format!("{}:{:02}:{:02}", total / 3600, (total / 60) % 60, total % 60)
}

// ---------------------------------------------------------------------------
// SOAP
// ---------------------------------------------------------------------------

pub fn soap_envelope(service: &str, action: &str, args: &[(&str, &str)]) -> String {
    let body: String = args
        .iter()
        .map(|(key, value)| format!("<{key}>{}</{key}>", xml_escape(value)))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body><u:{action} xmlns:u=\"{service}\">{body}</u:{action}></s:Body></s:Envelope>"
    )
}

/// Explains the UPnP error codes Sonos actually sends.
fn upnp_error(code: &str) -> &'static str {
    match code {
        "701" => "that track is not available",
        "711" | "712" => "the play mode is not supported",
        "714" => "the song file is in a format Sonos cannot play",
        "716" => "the song could not be found",
        "800" => "this speaker is part of a group — pick the group instead",
        "402" => "the speaker rejected the request",
        _ => "the speaker refused the request",
    }
}

async fn soap(
    base_url: &str,
    endpoint: (&str, &str),
    action: &str,
    args: &[(&str, &str)],
) -> Result<String> {
    let (path, service) = endpoint;
    let response = client()
        .post(format!("{base_url}{path}"))
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPACTION", format!("\"{service}#{action}\""))
        .body(soap_envelope(service, action, args))
        .send()
        .await
        .map_err(speaker_error)?;
    let status = response.status();
    let body = response.text().await.map_err(speaker_error)?;
    if !status.is_success() {
        let code = tag_text(&body, "errorCode").unwrap_or_default();
        return Err(Error::msg(format!(
            "Sonos: {} ({action}{}).",
            upnp_error(&code),
            if code.is_empty() { String::new() } else { format!(", error {code}") }
        )));
    }
    Ok(body)
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

fn ssdp_search() -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_ADDR}\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: {SONOS_ST}\r\n\r\n"
    )
}

/// `http://192.168.1.20:1400` from an SSDP reply's LOCATION header.
pub fn base_from_ssdp(reply: &str) -> Option<String> {
    let location = reply.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case("location").then(|| value.trim().to_string())
    })?;
    let after_scheme = location.strip_prefix("http://")?;
    let host = after_scheme.split('/').next()?;
    Some(format!("http://{host}"))
}

/// Sends SSDP searches and gathers every Sonos that answers.
fn ssdp_discover(wait: Duration) -> Result<Vec<String>> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(Duration::from_millis(250)))?;
    let _ = socket.set_multicast_ttl_v4(2);
    let message = ssdp_search();
    let deadline = std::time::Instant::now() + wait;
    let mut bases: Vec<String> = Vec::new();
    let mut sent = 0;
    let mut buffer = [0u8; 2048];
    while std::time::Instant::now() < deadline {
        // UDP is lossy; ask three times.
        if sent < 3 {
            let _ = socket.send_to(message.as_bytes(), SSDP_ADDR);
            sent += 1;
        }
        if let Ok((len, _)) = socket.recv_from(&mut buffer) {
            let reply = String::from_utf8_lossy(&buffer[..len]);
            if let Some(base) = base_from_ssdp(&reply) {
                if !bases.contains(&base) {
                    bases.push(base);
                }
            }
        }
    }
    Ok(bases)
}

/// Turns Sonos's zone-group layout into playable groups.
pub fn groups_from_topology(state_xml: &str) -> Vec<CastDevice> {
    let mut devices = Vec::new();
    let mut search = 0;
    while let Some(rel) = state_xml[search..].find("<ZoneGroup ") {
        let start = search + rel;
        let end = state_xml[start..]
            .find("</ZoneGroup>")
            .map(|e| start + e)
            .unwrap_or(state_xml.len());
        let block = &state_xml[start..end];
        search = end.max(start + 1);

        let Some(group) = elements(block, "ZoneGroup").into_iter().next() else { continue };
        let Some(coordinator) = group.get("Coordinator").cloned() else { continue };
        let members: Vec<HashMap<String, String>> = elements(block, "ZoneGroupMember")
            .into_iter()
            // Surround speakers, subs and bridges are not rooms of their own.
            .filter(|m| m.get("Invisible").map(String::as_str) != Some("1"))
            .collect();
        let Some(lead) = members
            .iter()
            .find(|m| m.get("UUID") == Some(&coordinator))
        else {
            continue;
        };
        let Some(base_url) = lead.get("Location").and_then(|l| base_from_ssdp(&format!("LOCATION: {l}")))
        else {
            continue;
        };
        let room = lead.get("ZoneName").cloned().unwrap_or_else(|| "Sonos".into());
        let mut rooms: Vec<&String> = members.iter().filter_map(|m| m.get("ZoneName")).collect();
        rooms.sort();
        rooms.dedup();
        let name = if rooms.len() > 1 {
            format!("{room} + {}", rooms.len() - 1)
        } else {
            room
        };
        devices.push(CastDevice { id: coordinator, name, base_url });
    }
    devices.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    devices
}

/// Asks one speaker for the whole household's layout; falls back to that
/// speaker alone when the topology call is not answered.
async fn devices_via(base_url: &str) -> Result<Vec<CastDevice>> {
    if let Ok(body) = soap(base_url, ZONE_TOPOLOGY, "GetZoneGroupState", &[]).await {
        if let Some(state) = tag_text(&body, "ZoneGroupState") {
            let groups = groups_from_topology(&state);
            if !groups.is_empty() {
                return Ok(groups);
            }
        }
    }
    let description = client()
        .get(format!("{base_url}/xml/device_description.xml"))
        .send()
        .await
        .map_err(speaker_error)?
        .text()
        .await
        .map_err(speaker_error)?;
    let id = tag_text(&description, "UDN")
        .map(|udn| udn.trim_start_matches("uuid:").to_string())
        .ok_or_else(|| Error::msg("That address does not look like a Sonos speaker."))?;
    let name = tag_text(&description, "roomName").unwrap_or_else(|| "Sonos".into());
    Ok(vec![CastDevice { id, name, base_url: base_url.to_string() }])
}

// ---------------------------------------------------------------------------
// The media server
// ---------------------------------------------------------------------------

struct Server {
    port: u16,
    token: String,
}

static SERVER: OnceLock<Server> = OnceLock::new();

fn random_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    (0..24).map(|_| format!("{:x}", rng.random_range(0..16u8))).collect()
}

/// Starts the server once; later calls return the running one.
async fn ensure_server(app: &AppHandle) -> Result<&'static Server> {
    if let Some(server) = SERVER.get() {
        return Ok(server);
    }
    let listener = TcpListener::bind("0.0.0.0:0").await?;
    let port = listener.local_addr()?.port();
    let server = SERVER.get_or_init(|| Server { port, token: random_token() });
    if server.port != port {
        // Lost a race with another caller; theirs is already serving.
        return Ok(server);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else { continue };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = handle_connection(app, stream, peer).await;
            });
        }
    });
    Ok(server)
}

/// The phone's address as seen from the speaker.
fn local_ip_towards(base_url: &str) -> Result<IpAddr> {
    let host = base_url.trim_start_matches("http://");
    let target: SocketAddr = host
        .parse()
        .map_err(|_| Error::msg(format!("Unexpected speaker address {host}")))?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect(target)?;
    Ok(socket.local_addr()?.ip())
}

#[derive(Debug, PartialEq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub range: Option<(u64, Option<u64>)>,
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn parse_request(head: &str) -> Option<Request> {
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let target = first.next()?;
    let (path, query_text) = target.split_once('?').unwrap_or((target, ""));
    let query = query_text
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect();
    let range = lines.find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if !key.trim().eq_ignore_ascii_case("range") {
            return None;
        }
        let spec = value.trim().strip_prefix("bytes=")?;
        let (start, end) = spec.split_once('-')?;
        Some((start.trim().parse().ok()?, end.trim().parse().ok()))
    });
    Some(Request { method, path: path.to_string(), query, range })
}

async fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    Ok(())
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().map(str::to_lowercase).as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("m4a") | Some("mp4") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}

async fn send_file(stream: &mut TcpStream, request: &Request, path: &str) -> Result<()> {
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(_) => return respond(stream, "404 Not Found", "text/plain", b"gone").await,
    };
    let size = file.metadata().await?.len();
    let (status, start, end) = match request.range {
        Some((start, end)) if start < size => {
            let end = end.unwrap_or(size - 1).min(size - 1);
            ("206 Partial Content", start, end)
        }
        Some(_) => {
            let head = format!(
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{size}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(head.as_bytes()).await?;
            return Ok(());
        }
        None => ("200 OK", 0, size.saturating_sub(1)),
    };
    let length = if size == 0 { 0 } else { end - start + 1 };
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {}\r\nContent-Length: {length}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n",
        mime_for(path)
    );
    if status.starts_with("206") {
        head.push_str(&format!("Content-Range: bytes {start}-{end}/{size}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    if request.method == "HEAD" || length == 0 {
        return Ok(());
    }
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let mut remaining = length;
    let mut buffer = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let want = remaining.min(buffer.len() as u64) as usize;
        let read = file.read(&mut buffer[..want]).await?;
        if read == 0 {
            break;
        }
        stream.write_all(&buffer[..read]).await?;
        remaining -= read as u64;
    }
    Ok(())
}

fn track_files(conn: &Connection, track_id: i64) -> Result<Option<(String, Option<String>)>> {
    Ok(conn
        .query_row(
            "SELECT audio_path, image_path FROM song_tracks WHERE id = ?1",
            [track_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

async fn handle_connection(app: AppHandle, mut stream: TcpStream, peer: SocketAddr) -> Result<()> {
    let mut head = Vec::new();
    let mut buffer = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
            .await
            .map_err(|_| Error::msg("request timed out"))??;
        if read == 0 || head.len() > 16 * 1024 {
            return Ok(());
        }
        head.extend_from_slice(&buffer[..read]);
    }
    let Some(request) = parse_request(&String::from_utf8_lossy(&head)) else {
        return respond(&mut stream, "400 Bad Request", "text/plain", b"bad request").await;
    };
    let Some(server) = SERVER.get() else { return Ok(()) };
    let parts: Vec<&str> = request.path.trim_start_matches('/').split('/').collect();

    match parts.as_slice() {
        // /media/<token>/<track>.mp3 and /art/<token>/<track> — for the speaker.
        [kind @ ("media" | "art"), token, file] if *token == server.token => {
            let track_id: i64 = file.split('.').next().unwrap_or("").parse().unwrap_or(-1);
            let files = {
                let db = app.state::<Db>();
                let conn = db.lock();
                track_files(&conn, track_id)?
            };
            let path = match (files, *kind) {
                (Some((audio, _)), "media") => Some(audio),
                (Some((_, image)), _) => image,
                (None, _) => None,
            };
            match path {
                Some(path) => send_file(&mut stream, &request, &path).await,
                None => respond(&mut stream, "404 Not Found", "text/plain", b"unknown").await,
            }
        }
        // /control/... — only for the phone's own media session.
        ["control", action] if peer.ip().is_loopback()
            && request.query.get("token") == Some(&server.token) =>
        {
            let result = match *action {
                "state" => current_state(&app).map(|s| serde_json::to_vec(&s).unwrap_or_default()),
                "command" => {
                    let action = request.query.get("action").cloned().unwrap_or_default();
                    let value = request.query.get("value").and_then(|v| v.parse::<f64>().ok());
                    run_command(&app, &action, value).await.map(|_| b"{}".to_vec())
                }
                _ => Err(Error::msg("unknown control")),
            };
            match result {
                Ok(body) => respond(&mut stream, "200 OK", "application/json", &body).await,
                Err(error) => {
                    respond(&mut stream, "409 Conflict", "text/plain", error.to_string().as_bytes())
                        .await
                }
            }
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", b"not found").await,
    }
}

// ---------------------------------------------------------------------------
// The cast session
// ---------------------------------------------------------------------------

struct Session {
    device: CastDevice,
    items: Vec<CastItem>,
    repeat: String,
    /// The media URL prefix, used to recognise our own tracks.
    media_prefix: String,
    /// Bumped on every new session so an old poller stops.
    generation: u64,
    last: Option<CastState>,
    /// The track whose play has already been counted on this pass.
    counted: Option<i64>,
}

#[derive(Default)]
pub struct CastSessions(Mutex<Option<Session>>);

static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn sessions(app: &AppHandle) -> State<'_, CastSessions> {
    app.state::<CastSessions>()
}

fn current_state(app: &AppHandle) -> Result<CastState> {
    let sessions = sessions(app);
    let guard = sessions.0.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .and_then(|s| s.last.clone())
        .ok_or_else(|| Error::msg("Not casting."))
}

fn session_device(app: &AppHandle) -> Result<CastDevice> {
    let sessions = sessions(app);
    let guard = sessions.0.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .map(|s| s.device.clone())
        .ok_or_else(|| Error::msg("Not casting."))
}

pub fn didl(title: &str, artist: &str, url: &str, art: &str, mime: &str, id: i64) -> String {
    format!(
        "<DIDL-Lite xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
         xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\" \
         xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\">\
         <item id=\"polski-{id}\" parentID=\"polski\" restricted=\"1\">\
         <dc:title>{}</dc:title><dc:creator>{}</dc:creator><upnp:album>Polski</upnp:album>\
         <upnp:albumArtURI>{}</upnp:albumArtURI>\
         <upnp:class>object.item.audioItem.musicTrack</upnp:class>\
         <res protocolInfo=\"http-get:*:{mime}:*\">{}</res></item></DIDL-Lite>",
        xml_escape(title),
        xml_escape(artist),
        xml_escape(art),
        xml_escape(url)
    )
}

pub fn play_mode(repeat: &str) -> &'static str {
    match repeat {
        "one" => "REPEAT_ONE",
        "off" => "NORMAL",
        _ => "REPEAT_ALL",
    }
}

fn load_items(conn: &Connection, track_ids: &[i64]) -> Result<Vec<(CastItem, String)>> {
    let mut items = Vec::new();
    for id in track_ids {
        let row: Option<(String, String, String)> = conn
            .query_row(
                "SELECT s.title, s.chorus_pl, t.audio_path FROM song_tracks t
                 JOIN songs s ON s.id = t.song_id WHERE t.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((title, chorus, path)) = row {
            items.push((CastItem { track_id: *id, title, artist: chorus }, path));
        }
    }
    Ok(items)
}

async fn poll_once(app: &AppHandle) -> Result<CastState> {
    let (device, items, repeat, prefix) = {
        let sessions = sessions(app);
        let guard = sessions.0.lock().unwrap_or_else(|e| e.into_inner());
        let session = guard.as_ref().ok_or_else(|| Error::msg("Not casting."))?;
        (
            session.device.clone(),
            session.items.clone(),
            session.repeat.clone(),
            session.media_prefix.clone(),
        )
    };
    let transport = soap(&device.base_url, AV_TRANSPORT, "GetTransportInfo", &[("InstanceID", "0")]).await?;
    let position = soap(&device.base_url, AV_TRANSPORT, "GetPositionInfo", &[("InstanceID", "0")]).await?;
    let volume = soap(
        &device.base_url,
        GROUP_RENDERING,
        "GetGroupVolume",
        &[("InstanceID", "0")],
    )
    .await
    .ok()
    .and_then(|body| tag_text(&body, "CurrentVolume"))
    .and_then(|v| v.parse().ok())
    .unwrap_or(-1);

    let transport_state = tag_text(&transport, "CurrentTransportState").unwrap_or_default();
    let uri = tag_text(&position, "TrackURI").unwrap_or_default();
    let ours = uri.starts_with(&prefix);
    let track_id = ours
        .then(|| uri[prefix.len()..].split('.').next().and_then(|id| id.parse::<i64>().ok()))
        .flatten();
    let index = track_id
        .and_then(|id| items.iter().position(|item| item.track_id == id))
        .map(|i| i as i64)
        .unwrap_or(-1);

    Ok(CastState {
        device,
        items,
        index,
        track_id,
        playing: transport_state == "PLAYING" || transport_state == "TRANSITIONING",
        transport: transport_state,
        position: parse_hms(&tag_text(&position, "RelTime").unwrap_or_default()),
        duration: parse_hms(&tag_text(&position, "TrackDuration").unwrap_or_default()),
        volume,
        repeat,
        detached: !uri.is_empty() && !ours,
    })
}

/// Keeps the cached state fresh, tells the UI, and counts plays.
fn spawn_poller(app: AppHandle, generation: u64) {
    tauri::async_runtime::spawn(async move {
        let mut failures = 0;
        loop {
            {
                let sessions = sessions(&app);
                let guard = sessions.0.lock().unwrap_or_else(|e| e.into_inner());
                match guard.as_ref() {
                    Some(session) if session.generation == generation => {}
                    _ => return,
                }
            }
            match poll_once(&app).await {
                Ok(state) => {
                    failures = 0;
                    let count = {
                        let sessions = sessions(&app);
                        let mut guard = sessions.0.lock().unwrap_or_else(|e| e.into_inner());
                        let mut count = None;
                        if let Some(session) = guard.as_mut().filter(|s| s.generation == generation) {
                            // Back near the start (repeat-one, or the same
                            // song again) is a new pass that may be counted.
                            if state.position < 5.0 {
                                session.counted = None;
                            }
                            if let Some(id) = state.track_id {
                                if session.counted != Some(id)
                                    && state.duration > 0.0
                                    && state.position / state.duration > 0.5
                                {
                                    session.counted = Some(id);
                                    count = Some(id);
                                }
                            }
                            session.last = Some(state.clone());
                        }
                        count
                    };
                    if let Some(id) = count {
                        let db = app.state::<Db>();
                        let _ = db.lock().execute(
                            "UPDATE song_tracks SET play_count = play_count + 1, last_played_at = ?2 WHERE id = ?1",
                            rusqlite::params![id, db::now_secs()],
                        );
                    }
                    let _ = app.emit("cast://state", Some(state));
                }
                Err(_) => {
                    failures += 1;
                    // Ten seconds without an answer: the speaker is gone.
                    if failures >= 10 {
                        end_session(&app);
                        return;
                    }
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

fn end_session(app: &AppHandle) {
    {
        let sessions = sessions(app);
        let mut guard = sessions.0.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }
    let _ = app.emit("cast://state", Option::<CastState>::None);
}

async fn run_command(app: &AppHandle, action: &str, value: Option<f64>) -> Result<()> {
    let device = session_device(app)?;
    let base = device.base_url.as_str();
    let instance = [("InstanceID", "0")];
    match action {
        "play" => {
            soap(base, AV_TRANSPORT, "Play", &[("InstanceID", "0"), ("Speed", "1")]).await?;
        }
        "pause" => {
            soap(base, AV_TRANSPORT, "Pause", &instance).await?;
        }
        "toggle" => {
            let playing = current_state(app).map(|s| s.playing).unwrap_or(false);
            return Box::pin(run_command(app, if playing { "pause" } else { "play" }, None)).await;
        }
        "next" => {
            soap(base, AV_TRANSPORT, "Next", &instance).await?;
        }
        "previous" => {
            // Like any player: restart the song unless it has barely begun.
            let position = current_state(app).map(|s| s.position).unwrap_or(0.0);
            if position > 4.0 {
                soap(base, AV_TRANSPORT, "Seek", &[("InstanceID", "0"), ("Unit", "REL_TIME"), ("Target", "0:00:00")]).await?;
            } else {
                soap(base, AV_TRANSPORT, "Previous", &instance).await?;
            }
        }
        "seek" => {
            let target = format_hms(value.unwrap_or(0.0));
            soap(base, AV_TRANSPORT, "Seek", &[("InstanceID", "0"), ("Unit", "REL_TIME"), ("Target", &target)]).await?;
        }
        "track" => {
            // `value` is a 0-based index into the queue.
            let number = (value.unwrap_or(0.0).max(0.0) as i64 + 1).to_string();
            soap(base, AV_TRANSPORT, "Seek", &[("InstanceID", "0"), ("Unit", "TRACK_NR"), ("Target", &number)]).await?;
            soap(base, AV_TRANSPORT, "Play", &[("InstanceID", "0"), ("Speed", "1")]).await?;
        }
        "volume" => {
            let level = value.unwrap_or(20.0).clamp(0.0, 100.0).round().to_string();
            soap(base, GROUP_RENDERING, "SetGroupVolume", &[("InstanceID", "0"), ("DesiredVolume", &level)]).await?;
        }
        "stop" => {
            let _ = soap(base, AV_TRANSPORT, "Stop", &instance).await;
            end_session(app);
            return Ok(());
        }
        other => return Err(Error::msg(format!("Unknown command '{other}'."))),
    }
    // Answer promptly instead of waiting for the next poll.
    if let Ok(state) = poll_once(app).await {
        let sessions = sessions(app);
        let mut guard = sessions.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = guard.as_mut() {
            session.last = Some(state.clone());
        }
        drop(guard);
        let _ = app.emit("cast://state", Some(state));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Finds Sonos rooms and groups on the Wi-Fi. `address` skips the search and
/// asks one speaker directly (for networks that block discovery).
#[tauri::command]
pub async fn cast_discover(address: Option<String>) -> Result<Vec<CastDevice>> {
    let bases = match address.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()) {
        Some(address) => {
            let host = address.trim_start_matches("http://").trim_end_matches('/');
            let host = if host.contains(':') { host.to_string() } else { format!("{host}:1400") };
            vec![format!("http://{host}")]
        }
        None => tauri::async_runtime::spawn_blocking(|| ssdp_discover(Duration::from_millis(2500)))
            .await
            .map_err(|e| Error::msg(e.to_string()))??,
    };
    if bases.is_empty() {
        return Ok(Vec::new());
    }
    let mut last_error = None;
    // Any speaker can describe the whole household.
    for base in &bases {
        match devices_via(base).await {
            Ok(devices) => return Ok(devices),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| Error::msg("No speaker answered.")))
}

/// Puts the songs on the speaker's queue and starts playing at `index`.
#[tauri::command]
pub async fn cast_start(
    app: AppHandle,
    db: State<'_, Db>,
    device: CastDevice,
    track_ids: Vec<i64>,
    index: i64,
    repeat: String,
) -> Result<CastState> {
    let items = {
        let conn = db.lock();
        load_items(&conn, &track_ids)?
    };
    if items.is_empty() {
        return Err(Error::msg("There are no downloaded songs to play."));
    }
    let server = ensure_server(&app).await?;
    let ip = local_ip_towards(&device.base_url)?;
    let root = format!("http://{ip}:{}", server.port);
    let media_prefix = format!("{root}/media/{}/", server.token);
    let base = device.base_url.as_str();

    soap(base, AV_TRANSPORT, "RemoveAllTracksFromQueue", &[("InstanceID", "0")]).await?;
    for (item, path) in &items {
        let ext = path.rsplit('.').next().unwrap_or("mp3");
        let url = format!("{media_prefix}{}.{ext}", item.track_id);
        let art = format!("{root}/art/{}/{}", server.token, item.track_id);
        let metadata = didl(&item.title, &item.artist, &url, &art, mime_for(path), item.track_id);
        soap(
            base,
            AV_TRANSPORT,
            "AddURIToQueue",
            &[
                ("InstanceID", "0"),
                ("EnqueuedURI", &url),
                ("EnqueuedURIMetaData", &metadata),
                ("DesiredFirstTrackNumberEnqueued", "0"),
                ("EnqueueAsNext", "0"),
            ],
        )
        .await?;
    }
    let queue = format!("x-rincon-queue:{}#0", device.id);
    soap(
        base,
        AV_TRANSPORT,
        "SetAVTransportURI",
        &[("InstanceID", "0"), ("CurrentURI", &queue), ("CurrentURIMetaData", "")],
    )
    .await?;
    let _ = soap(
        base,
        AV_TRANSPORT,
        "SetPlayMode",
        &[("InstanceID", "0"), ("NewPlayMode", play_mode(&repeat))],
    )
    .await;
    let number = (index.clamp(0, items.len() as i64 - 1) + 1).to_string();
    soap(base, AV_TRANSPORT, "Seek", &[("InstanceID", "0"), ("Unit", "TRACK_NR"), ("Target", &number)]).await?;
    soap(base, AV_TRANSPORT, "Play", &[("InstanceID", "0"), ("Speed", "1")]).await?;

    let generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    {
        let sessions = sessions(&app);
        let mut guard = sessions.0.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(Session {
            device,
            items: items.into_iter().map(|(item, _)| item).collect(),
            repeat,
            media_prefix,
            generation,
            last: None,
            counted: None,
        });
    }
    let state = poll_once(&app).await?;
    {
        let sessions = sessions(&app);
        let mut guard = sessions.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = guard.as_mut() {
            session.last = Some(state.clone());
        }
    }
    spawn_poller(app.clone(), generation);
    let _ = app.emit("cast://state", Some(state.clone()));
    Ok(state)
}

/// play | pause | toggle | next | previous | seek (seconds) | track (index)
/// | volume (0–100) | stop
#[tauri::command]
pub async fn cast_command(app: AppHandle, action: String, value: Option<f64>) -> Result<()> {
    run_command(&app, &action, value).await
}

#[tauri::command]
pub async fn cast_set_repeat(app: AppHandle, repeat: String) -> Result<()> {
    let device = session_device(&app)?;
    soap(
        &device.base_url,
        AV_TRANSPORT,
        "SetPlayMode",
        &[("InstanceID", "0"), ("NewPlayMode", play_mode(&repeat))],
    )
    .await?;
    let sessions = sessions(&app);
    let mut guard = sessions.0.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(session) = guard.as_mut() {
        session.repeat = repeat;
    }
    Ok(())
}

#[tauri::command]
pub async fn cast_state(app: AppHandle) -> Result<Option<CastState>> {
    Ok(current_state(&app).ok())
}

/// For the phone's native media session: where to send lock-screen commands.
#[tauri::command]
pub async fn cast_control_info(app: AppHandle) -> Result<ControlInfo> {
    let server = ensure_server(&app).await?;
    Ok(ControlInfo { port: server.port, token: server.token.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_round_trip() {
        assert_eq!(parse_hms("0:03:04"), 184.0);
        assert_eq!(parse_hms("1:00:00"), 3600.0);
        assert_eq!(parse_hms("NOT_IMPLEMENTED"), 0.0);
        assert_eq!(parse_hms(""), 0.0);
        assert_eq!(format_hms(184.4), "0:03:04");
        assert_eq!(format_hms(-3.0), "0:00:00");
    }

    #[test]
    fn soap_arguments_are_escaped() {
        let body = soap_envelope(AV_TRANSPORT.1, "AddURIToQueue", &[("EnqueuedURIMetaData", "<DIDL a=\"1\">&</DIDL>")]);
        assert!(body.contains("<u:AddURIToQueue xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">"));
        assert!(body.contains("<EnqueuedURIMetaData>&lt;DIDL a=&quot;1&quot;&gt;&amp;&lt;/DIDL&gt;</EnqueuedURIMetaData>"));
    }

    #[test]
    fn replies_are_read_regardless_of_namespace() {
        let reply = "<s:Envelope><s:Body><u:GetPositionInfoResponse xmlns:u=\"x\">\
                     <Track>2</Track><TrackDuration>0:03:04</TrackDuration>\
                     <TrackMetaData>&lt;DIDL&gt;</TrackMetaData>\
                     <TrackURI>http://10.0.0.5:5000/media/abc/7.mp3</TrackURI>\
                     <RelTime>0:01:10</RelTime><Empty/></u:GetPositionInfoResponse></s:Body></s:Envelope>";
        assert_eq!(tag_text(reply, "Track").as_deref(), Some("2"));
        assert_eq!(tag_text(reply, "TrackMetaData").as_deref(), Some("<DIDL>"));
        assert_eq!(tag_text(reply, "RelTime").as_deref(), Some("0:01:10"));
        assert_eq!(tag_text(reply, "Empty").as_deref(), Some(""));
        assert_eq!(tag_text(reply, "Missing"), None);
        let error = "<s:Fault><detail><UPnPError xmlns=\"urn:x\"><errorCode>800</errorCode></UPnPError></detail></s:Fault>";
        assert_eq!(tag_text(error, "errorCode").as_deref(), Some("800"));
    }

    #[test]
    fn ssdp_replies_give_the_control_address() {
        let reply = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age = 1800\r\n\
                     LOCATION: http://192.168.1.20:1400/xml/device_description.xml\r\n\
                     ST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\r\n";
        assert_eq!(base_from_ssdp(reply).as_deref(), Some("http://192.168.1.20:1400"));
        assert_eq!(base_from_ssdp("HTTP/1.1 200 OK\r\n\r\n"), None);
    }

    #[test]
    fn groups_are_named_after_their_rooms() {
        let topology = r#"<ZoneGroupState><ZoneGroups>
            <ZoneGroup Coordinator="RINCON_A" ID="RINCON_A:1">
              <ZoneGroupMember UUID="RINCON_A" Location="http://192.168.1.20:1400/xml/device_description.xml" ZoneName="Living Room"/>
              <ZoneGroupMember UUID="RINCON_SUB" Location="http://192.168.1.21:1400/xml/device_description.xml" ZoneName="Living Room" Invisible="1"/>
              <ZoneGroupMember UUID="RINCON_B" Location="http://192.168.1.22:1400/xml/device_description.xml" ZoneName="Kitchen"/>
            </ZoneGroup>
            <ZoneGroup Coordinator="RINCON_C" ID="RINCON_C:5">
              <ZoneGroupMember UUID="RINCON_C" Location="http://192.168.1.30:1400/xml/device_description.xml" ZoneName="Bedroom">
                <Satellite UUID="RINCON_D" ZoneName="Bedroom" Invisible="1"/>
              </ZoneGroupMember>
            </ZoneGroup>
          </ZoneGroups></ZoneGroupState>"#;
        let groups = groups_from_topology(topology);
        assert_eq!(
            groups,
            vec![
                CastDevice { id: "RINCON_C".into(), name: "Bedroom".into(), base_url: "http://192.168.1.30:1400".into() },
                CastDevice { id: "RINCON_A".into(), name: "Living Room + 1".into(), base_url: "http://192.168.1.20:1400".into() },
            ]
        );
    }

    #[test]
    fn requests_parse_paths_queries_and_ranges() {
        let request = parse_request(
            "GET /control/command?token=abc&action=seek&value=12.5 HTTP/1.1\r\nHost: x\r\nRange: bytes=100-\r\n\r\n",
        )
        .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/control/command");
        assert_eq!(request.query.get("action").map(String::as_str), Some("seek"));
        assert_eq!(request.query.get("value").map(String::as_str), Some("12.5"));
        assert_eq!(request.range, Some((100, None)));

        let request = parse_request("HEAD /media/t/7.mp3 HTTP/1.1\r\nrange: bytes=0-99\r\n\r\n").unwrap();
        assert_eq!(request.range, Some((0, Some(99))));
        assert_eq!(percent_decode("a%20b%C5%82+c"), "a bł c");
    }

    fn http_reply(status: &str, body: &str) -> String {
        format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    async fn serve_once(reply: String) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut buffer = [0u8; 4096];
            // Read headers, then the body by Content-Length.
            loop {
                let n = stream.read(&mut buffer).await.unwrap();
                seen.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&seen).to_string();
                if let Some(split) = text.find("\r\n\r\n") {
                    let length: usize = text
                        .lines()
                        .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap()))
                        .unwrap_or(0);
                    if seen.len() >= split + 4 + length {
                        break;
                    }
                }
            }
            stream.write_all(reply.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&seen).to_string()
        });
        (base, handle)
    }

    #[tokio::test]
    async fn soap_calls_carry_the_action_and_surface_upnp_errors() {
        let ok = http_reply(
            "200 OK",
            "<s:Envelope><s:Body><u:GetVolumeResponse><CurrentVolume>23</CurrentVolume></u:GetVolumeResponse>",
        );
        let (base, seen) = serve_once(ok).await;
        let body = soap(&base, GROUP_RENDERING, "GetGroupVolume", &[("InstanceID", "0")]).await.unwrap();
        assert_eq!(tag_text(&body, "CurrentVolume").as_deref(), Some("23"));
        let request = seen.await.unwrap();
        assert!(request.starts_with("POST /MediaRenderer/GroupRenderingControl/Control HTTP/1.1"));
        assert!(request.to_lowercase().contains(
            "soapaction: \"urn:schemas-upnp-org:service:grouprenderingcontrol:1#getgroupvolume\""
        ));
        assert!(request.contains("<InstanceID>0</InstanceID>"));

        let fault = http_reply(
            "500 Internal Server Error",
            "<s:Fault><UPnPError><errorCode>800</errorCode></UPnPError></s:Fault>",
        );
        let (base, _) = serve_once(fault).await;
        let error = soap(&base, AV_TRANSPORT, "Play", &[]).await.unwrap_err().to_string();
        assert!(error.contains("pick the group instead"), "{error}");
    }

    #[tokio::test]
    async fn songs_are_served_whole_or_by_range() {
        let dir = std::env::temp_dir().join(format!("polski-serve-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.mp3");
        std::fs::write(&path, (0u8..200).collect::<Vec<u8>>()).unwrap();
        let path = path.to_string_lossy().to_string();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let served = path.clone();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = [0u8; 2048];
                let n = stream.read(&mut buffer).await.unwrap();
                let request = parse_request(&String::from_utf8_lossy(&buffer[..n])).unwrap();
                send_file(&mut stream, &request, &served).await.unwrap();
            }
        });

        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = format!("http://{address}/media/t/1.mp3");
        let whole = http.get(&url).send().await.unwrap();
        assert_eq!(whole.status(), 200);
        assert_eq!(whole.headers()["content-type"], "audio/mpeg");
        assert_eq!(whole.headers()["accept-ranges"], "bytes");
        assert_eq!(whole.bytes().await.unwrap().len(), 200);

        let part = http.get(&url).header("Range", "bytes=10-19").send().await.unwrap();
        assert_eq!(part.status(), 206);
        assert_eq!(part.headers()["content-range"], "bytes 10-19/200");
        assert_eq!(part.bytes().await.unwrap().to_vec(), (10u8..20).collect::<Vec<u8>>());

        let tail = http.get(&url).header("Range", "bytes=190-").send().await.unwrap();
        assert_eq!(tail.bytes().await.unwrap().len(), 10);

        let beyond = http.get(&url).header("Range", "bytes=500-").send().await.unwrap();
        assert_eq!(beyond.status(), 416);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn metadata_names_the_song() {
        let meta = didl("Kawa & mleko", "Poproszę <kawę>", "http://h/media/t/1.mp3", "http://h/art/t/1", "audio/mpeg", 1);
        assert!(meta.contains("<dc:title>Kawa &amp; mleko</dc:title>"));
        assert!(meta.contains("<dc:creator>Poproszę &lt;kawę&gt;</dc:creator>"));
        assert!(meta.contains("protocolInfo=\"http-get:*:audio/mpeg:*\">http://h/media/t/1.mp3</res>"));
        assert_eq!(play_mode("one"), "REPEAT_ONE");
        assert_eq!(play_mode("off"), "NORMAL");
        assert_eq!(play_mode("all"), "REPEAT_ALL");
    }
}
