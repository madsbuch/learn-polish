//! Sync as the app runs it (as Tally's): listening for the vault's other
//! devices, answering discovery, syncing a few seconds after every change
//! (and every so often regardless), and the commands the Devices screen
//! calls. Events tell the screens when another device's data arrived
//! (`sync-changed`: they reload what they show) and when the status moved
//! (`sync-status`).
//!
//! Everything runs on Tauri's async runtime. The database is locked only
//! around each engine call (`sync::with`), on a blocking thread, so the
//! screens never wait for the network.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Nullable, Text};
use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Notify;

use super::session::{self, Outcome, PairingOffer, PairingSlot, Received};
use super::{app, discovery, engine, now_iso, snapshots, with, Store};
use crate::db::{self, Conn};
use crate::error::{Error, Result};
use crate::ids::DeviceId;

/// Syncs at least this often while the app runs, changes or not. The
/// computer asks often: the phone's own pushes may not reach it (a desktop
/// firewall — macOS's for an unsigned app, Windows' on a "public" network —
/// turns away incoming connections, and the learner is never asked), so the
/// computer's own asking is what brings the phone's changes over; a
/// connection the computer makes is answered. The same goes for the phone's
/// background runs (background.rs): one that can't reach the computer leaves
/// its changes for the computer's next round. Idle syncs are cheap (no full
/// comparison when nothing moved). The phone asks less, for the battery.
const IDLE_ROUND: Duration = if cfg!(any(target_os = "android", target_os = "ios")) {
    Duration::from_secs(60)
} else {
    Duration::from_secs(10)
};
/// How often the loop looks for a local change (`sync_state.seq` moving):
/// a change is synced within this long, and a burst of them together.
const DEBOUNCE: Duration = Duration::from_secs(3);
/// How long after the app starts the computer looks for today's snapshot.
const SNAPSHOT_DELAY: Duration = Duration::from_secs(60);
/// How long a round waits for discovery's answers.
const PROBE_WAIT: Duration = Duration::from_millis(800);

/// The event the screens reload on.
pub const CHANGED_EVENT: &str = "sync-changed";
/// The event that says the status moved.
pub const STATUS_EVENT: &str = "sync-status";
/// The event that says the app came back to the front (lib.rs).
pub const RESUMED_EVENT: &str = "app-resumed";

/// What sync is doing: the payload of `sync-status`, and part of the status.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub syncing: bool,
    pub last_error: Option<String>,
    pub last_round_at: Option<String>,
}

#[derive(Default)]
pub struct SyncService {
    slot: PairingSlot,
    port: AtomicU16,
    started: AtomicBool,
    kick: Notify,
    activity: Mutex<Activity>,
    /// (vault tag, device id) for answering discovery without the database.
    discovery_state: Arc<Mutex<Option<(String, String)>>>,
    /// Where the database is: backups go beside it.
    db_path: OnceLock<PathBuf>,
    /// The last error noted in the Diagnostics log, so one that repeats every
    /// round is noted once.
    last_noted: Mutex<Option<String>>,
    /// Answering other devices' discovery probes, bound again on a later
    /// round when it couldn't be.
    responder: discovery::Responder,
    /// The front end has started (its first command): the Android plugins
    /// are loaded by then, and the background job can be scheduled.
    app_up: AtomicBool,
    /// What the Android background job was last successfully set to:
    /// scheduled (true) or cancelled (false); nothing yet this launch.
    background_job: Mutex<Option<bool>>,
}

/// The service as the commands receive it.
pub type SyncState<'a> = State<'a, Arc<SyncService>>;

impl SyncService {
    fn set_activity(&self, app: &AppHandle, f: impl FnOnce(&mut Activity)) {
        let snapshot = {
            let mut a = self.activity.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut a);
            a.clone()
        };
        let _ = app.emit(STATUS_EVENT, snapshot);
    }

    /// Notes an error in the Diagnostics log, unless it is the one noted last.
    fn note(&self, error: &str) {
        let mut last = self.last_noted.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_deref() != Some(error) {
            crate::diagnostics::note("sync", error);
            *last = Some(error.to_string());
        }
    }

    fn announce(&self, app: &AppHandle, received: &Received) {
        if received.changed_anything() {
            let _ = app.emit(CHANGED_EVENT, received);
        }
    }

    /// The port this device listens for its vault on, once it does.
    pub fn port(&self) -> Option<u16> {
        Some(self.port.load(Ordering::SeqCst)).filter(|p| *p != 0)
    }

    /// The front end has started: called by its first command (`init_app`).
    pub fn app_is_up(&self) {
        self.app_up.store(true, Ordering::SeqCst);
    }

    /// Answers discovery, if it isn't yet: at start, and again each round
    /// until it can (another program may hold the port for a while).
    async fn keep_answering(&self, port: u16) {
        let state = self.discovery_state.clone();
        let answering = self
            .responder
            .ensure(app::DISCOVERY_PORT, move || state.lock().ok()?.clone().map(|(tag, id)| (tag, id, port)))
            .await;
        // Not answering only means this device isn't found by broadcast; it
        // still asks, and is reached at the address it was last seen at.
        if let Err(e) = answering {
            self.note(&format!("Could not answer other devices looking for this one (trying again): {e}"));
        }
    }

    /// Android's background sync (the sync-background plugin, background.rs):
    /// scheduled while this device has another to sync with, cancelled when it
    /// has none (left the vault, removed the last other device) — a job with
    /// nothing to do would still wake the phone every half hour. Asked each
    /// round: only once the front end is up (the plugin isn't ready before),
    /// and again until the plugin has done it — then once per launch, which
    /// keeps the job's timing and takes a new database path if there is one.
    async fn keep_background_job(&self, app: &AppHandle) {
        if !cfg!(target_os = "android") || !self.app_up.load(Ordering::SeqCst) {
            return;
        }
        let Some(db_path) = self.db_path.get().cloned() else { return };
        let Ok(wanted) = with(app, paired).await else { return };
        let done = *self.background_job.lock().unwrap_or_else(|e| e.into_inner());
        if done == Some(wanted) {
            return;
        }
        let app2 = app.clone();
        let result = tokio::task::spawn_blocking(move || {
            if wanted {
                tauri_plugin_sync_background::schedule(&app2, &db_path.to_string_lossy())
            } else {
                tauri_plugin_sync_background::cancel(&app2)
            }
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        match result {
            Ok(()) => *self.background_job.lock().unwrap_or_else(|e| e.into_inner()) = Some(wanted),
            Err(e) if wanted => self.note(&format!("Could not schedule syncing in the background (trying again): {e}")),
            Err(e) => self.note(&format!("Could not stop syncing in the background (trying again): {e}")),
        }
    }

    /// Starts listening, answering discovery and the sync loop, once. Called
    /// from the app's setup once the database is open.
    pub async fn start(self: &Arc<Self>, app: AppHandle, db_path: PathBuf) -> Result<()> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let _ = self.db_path.set(db_path.clone());
        // The key pair, made now rather than on the first sync.
        with(&app, session::identity).await?;

        // Incoming connections.
        let (svc, app2) = (self.clone(), app.clone());
        let (port, _) = session::listen(app.clone(), self.slot.clone(), "0.0.0.0", app::SYNC_PORT, move |r| match r {
            Ok(outcome) => {
                svc.announce(&app2, &outcome.received);
                svc.set_activity(&app2, |a| {
                    a.last_error = None;
                    a.last_round_at = Some(now_iso());
                });
            }
            Err(e) if session::is_busy(&e) => {}
            Err(e) => svc.note(&format!("A sync from another device failed: {e}")),
        })
        .await?;
        self.port.store(port, Ordering::SeqCst);

        // Discovery answers (and again each round, if this fails).
        self.keep_answering(port).await;

        // The computer keeps a copy of the database a day, while it syncs
        // (snapshots.rs): what sync brings over can't then take anything
        // past going back. A phone has the computer's.
        if cfg!(not(any(target_os = "android", target_os = "ios"))) {
            if let Some(dir) = snapshots::dir_for(&db_path) {
                let (svc, app) = (self.clone(), app.clone());
                tokio::spawn(async move {
                    // Not while the app is starting: the copy reads the whole
                    // database, and the first screens read it too.
                    tokio::time::sleep(SNAPSHOT_DELAY).await;
                    loop {
                        if with(&app, paired).await.unwrap_or(false) {
                            let (path, dir) = (db_path.clone(), dir.clone());
                            let taken = tokio::task::spawn_blocking(move || snapshots::run(&path, &dir, &snapshots::today()))
                                .await
                                .map_err(|e| Error::msg(e.to_string()))
                                .and_then(|r| r);
                            match taken {
                                Ok(Some(path)) => crate::diagnostics::note("database", format!("Today's copy of the database: {}", path.display())),
                                Ok(None) => {}
                                Err(e) => svc.note(&format!("Could not take today's copy of the database: {e}")),
                            }
                        }
                        tokio::time::sleep(snapshots::EVERY).await;
                    }
                });
            }
        }

        // The loop: soon after a change, and every so often anyway.
        let svc = self.clone();
        tokio::spawn(async move {
            let mut last_seq = -1i64;
            let mut last_round: Option<Instant> = None;
            loop {
                let asked = tokio::select! {
                    _ = tokio::time::sleep(DEBOUNCE) => false,
                    _ = svc.kick.notified() => true,
                };
                let Ok(state) = with(&app, engine::local_state).await else { continue };
                if let (Some(vault), Ok(mut d)) = (&state.vault_id, svc.discovery_state.lock()) {
                    *d = Some((discovery::vault_tag(vault), state.device_id.clone()));
                }
                svc.keep_background_job(&app).await;
                if state.vault_id.is_none() {
                    continue;
                }
                let due = match last_round {
                    Some(t) => t.elapsed() >= IDLE_ROUND,
                    None => true,
                };
                if !asked && state.seq == last_seq && !due {
                    continue;
                }
                last_round = Some(Instant::now());
                svc.keep_answering(port).await;
                svc.round(&app).await;
                // This round's own writes move seq (rows applied): re-read,
                // so they alone don't call for another round.
                last_seq = with(&app, engine::local_state).await.map(|s| s.seq).unwrap_or(-1);
            }
        });
        Ok(())
    }

    /// Finds the vault's devices on the network and syncs with each.
    async fn round(&self, app: &AppHandle) {
        self.set_activity(app, |a| a.syncing = true);
        let result = sync_round(app).await;
        for o in &result.outcomes {
            self.announce(app, &o.received);
        }
        if let Some(e) = result.errors.first() {
            self.note(e);
        }
        self.set_activity(app, |a| {
            a.syncing = false;
            a.last_round_at = Some(now_iso());
            a.last_error = result.errors.first().cloned();
        });
    }

    pub fn kick(&self) {
        self.kick.notify_one();
    }

    pub fn activity(&self) -> Activity {
        self.activity.lock().map(|a| a.clone()).unwrap_or_default()
    }

    pub fn offer_active(&self) -> bool {
        self.slot
            .lock()
            .map(|s| s.as_ref().is_some_and(|p| p.expires > Instant::now()))
            .unwrap_or(false)
    }

    async fn offer(&self, app: &AppHandle) -> Result<SyncOffer> {
        let Some(port) = self.port() else {
            return Err(Error::msg("Sync isn't listening yet. Try again in a moment."));
        };
        let addrs = session::local_addrs(port);
        if addrs.is_empty() {
            return Err(Error::msg("This device isn't on a local network (Wi-Fi)."));
        }
        let offer = session::offer(app, &self.slot, addrs).await?;
        self.kick();
        let code = offer.encode()?;
        Ok(SyncOffer { svg: qr_svg(&code)?, code, expires_at: expires_at() })
    }

    pub fn cancel_offer(&self) {
        *self.slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Joins the vault behind `code`. With `replace`, a device that already
    /// holds data joins too: a full copy of its database is saved in
    /// `backups/` first, and its path returned.
    async fn join(&self, app: &AppHandle, code: &str, replace: bool) -> Result<JoinResult> {
        let offer = PairingOffer::decode(code)?;
        let backup = if replace {
            let path = self.db_path.get().cloned().ok_or_else(|| Error::msg("Sync hasn't started yet."))?;
            with(app, move |c| backup_before_join(c, &path)).await?
        } else {
            None
        };
        self.set_activity(app, |a| a.syncing = true);
        let result = session::join(app, &offer, replace).await;
        self.set_activity(app, |a| {
            a.syncing = false;
            a.last_error = result.as_ref().err().map(ToString::to_string);
            a.last_round_at = Some(now_iso());
        });
        match result {
            Ok(outcome) => {
                // Everything changed: the screens reload whatever they show.
                let _ = app.emit(CHANGED_EVENT, outcome.received);
                Ok(JoinResult { outcome, backup })
            }
            Err(e) => {
                crate::diagnostics::note("sync", format!("Joining another device failed: {e}"));
                Err(e)
            }
        }
    }
}

/// Saves a full copy of the database before a join replaces what is here,
/// when there is anything of the learner's to lose. Returns its path.
pub fn backup_before_join(conn: &mut Conn, db_path: &std::path::Path) -> Result<Option<String>> {
    if engine::data_tables(conn)?.is_empty() {
        return Ok(None);
    }
    let path = db::back_up(conn, db_path, "before-join", "before-join")?;
    crate::diagnostics::note("database", format!("Backed up before joining another device: {}", path.display()));
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn expires_at() -> String {
    let ms = std::time::SystemTime::now()
        .checked_add(session::PAIRING_TTL)
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    super::iso(ms)
}

/// A pairing code as a QR code, in SVG: black on white whatever the theme,
/// for the scanner's sake.
fn qr_svg(code: &str) -> Result<String> {
    let qr = QrCode::with_error_correction_level(code.as_bytes(), EcLevel::M)
        .map_err(|e| Error::msg(format!("Could not make the QR code: {e}")))?;
    Ok(qr
        .render::<svg::Color>()
        .quiet_zone(true)
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

/// A code to show for another device to join this one's vault.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOffer {
    /// The code itself (what the QR holds), for a device without a camera.
    pub code: String,
    /// The QR code, as SVG markup.
    pub svg: String,
    /// When the code stops working (ISO 8601, UTC).
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinResult {
    pub outcome: Outcome,
    /// Where the copy of this device's own data went, when it held any.
    pub backup: Option<String>,
}

/// What one round found.
pub struct Round {
    pub outcomes: Vec<Outcome>,
    pub errors: Vec<String>,
}

#[derive(QueryableByName)]
struct Peer {
    #[diesel(sql_type = Text)]
    id: String,
    #[diesel(sql_type = Nullable<Text>)]
    last_addr: Option<String>,
}

/// Whether this device is in a vault with at least one other device (not
/// revoked) — whether there is anyone to sync with. Only reads: the
/// background run asks before it writes anything.
pub fn paired(conn: &mut Conn) -> Result<bool> {
    #[derive(QueryableByName)]
    struct Found {
        #[diesel(sql_type = BigInt)]
        n: i64,
    }
    let found = diesel::sql_query(
        "SELECT count(*) AS n FROM sync_state s JOIN vault_devices d ON d.id <> s.device_id
         WHERE s.id = 1 AND s.vault_id IS NOT NULL
           AND (s.public_key IS NULL OR d.public_key <> lower(hex(s.public_key)))
           AND d.public_key NOT IN (SELECT public_key FROM vault_revocations)",
    )
    .get_result::<Found>(conn)?;
    Ok(found.n > 0)
}

/// The vault's other devices (not revoked), with where each was last reached.
fn peers(conn: &mut Conn) -> Result<(session::Identity, Vec<Peer>)> {
    let me = session::identity(conn)?;
    let peers = diesel::sql_query(
        "SELECT d.id, p.last_addr FROM vault_devices d LEFT JOIN sync_peers p ON p.device_id = d.id
         WHERE d.id <> ? AND d.public_key <> ? AND d.public_key NOT IN (SELECT public_key FROM vault_revocations)",
    )
    .bind::<Text, _>(me.device_id.to_string())
    .bind::<Text, _>(hex::encode(&me.public))
    .load::<Peer>(conn)?;
    Ok((me, peers))
}

/// One round: probe for the vault's devices, then sync with each one found —
/// and with any known one that didn't answer, at the address it was last
/// reached at (a network that drops broadcasts still reaches it).
pub async fn sync_round<S: Store>(store: &S) -> Round {
    let mut round = Round { outcomes: vec![], errors: vec![] };
    let (me, peers) = match with(store, peers).await {
        Ok(found) => found,
        Err(e) => {
            round.errors.push(e.to_string());
            return round;
        }
    };
    let Some(vault) = me.vault_id.clone() else { return round };
    if peers.is_empty() {
        return round;
    }
    let last: Vec<String> = peers.iter().filter_map(|p| p.last_addr.clone()).collect();
    let found = discovery::probe(
        &discovery::vault_tag(&vault),
        &me.device_id.to_string(),
        &discovery::targets(&last, app::DISCOVERY_PORT),
        PROBE_WAIT,
    )
    .await;
    for peer in &peers {
        // Where discovery says it is, then where it was last reached: an
        // answer to a probe isn't proof of anything (anyone on the network
        // can send one), so a failed attempt there doesn't end the search.
        let mut addrs: Vec<SocketAddr> = found.iter().filter(|f| f.device_id == peer.id).map(|f| f.addr).collect();
        if let Some(a) = peer.last_addr.as_ref().and_then(|a| a.parse().ok()) {
            if !addrs.contains(&a) {
                addrs.push(a);
            }
        }
        let mut last_err = None;
        for addr in addrs {
            match session::sync_with(store, addr).await {
                Ok(Some(o)) => {
                    round.outcomes.push(o);
                    last_err = None;
                    break;
                }
                Ok(None) => {
                    last_err = None;
                    break;
                }
                Err(e) => last_err = Some(e.to_string()),
            }
        }
        match last_err {
            // Not around is normal; anything else is worth showing.
            Some(e) if e.contains("no answer") || e.contains("refused") || e.contains("unreachable") => {}
            Some(e) => round.errors.push(e),
            None => {}
        }
    }
    round
}

// ---------------------------------------------------------------------------
// What the Devices screen shows
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatus {
    pub id: DeviceId,
    pub name: String,
    pub platform: String,
    pub is_self: bool,
    /// When the two last synced (ISO 8601), and when they last ended
    /// holding exactly the same data.
    pub last_sync_at: Option<String>,
    pub last_complete_at: Option<String>,
    pub complete: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub device_id: DeviceId,
    pub name: String,
    pub platform: String,
    pub in_vault: bool,
    pub devices: Vec<DeviceStatus>,
    pub offer_active: bool,
    #[serde(flatten)]
    pub activity: Activity,
}

/// One table's worth of the learner's data on this device.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Held {
    pub table: String,
    pub rows: i64,
}

#[derive(QueryableByName)]
struct DeviceRow {
    #[diesel(sql_type = Text)]
    id: String,
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Text)]
    platform: String,
    #[diesel(sql_type = Nullable<Text>)]
    last_sync_at: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    last_complete_at: Option<String>,
    #[diesel(sql_type = Nullable<BigInt>)]
    complete: Option<i64>,
}

pub fn status(conn: &mut Conn, offer_active: bool, activity: Activity) -> Result<Status> {
    let me = session::identity(conn)?;
    let rows = diesel::sql_query(
        "SELECT d.id, d.name, d.platform, p.last_sync_at, p.last_complete_at, p.complete
         FROM vault_devices d LEFT JOIN sync_peers p ON p.device_id = d.id
         WHERE d.public_key NOT IN (SELECT public_key FROM vault_revocations)
           -- One entry per key: a device that took a new id (instance.rs) is
           -- listed once, under the newest. This device is always its own.
           AND (d.id = ?1 OR NOT EXISTS (
             SELECT 1 FROM vault_devices n WHERE n.public_key = d.public_key AND n.id <> d.id
               AND (n.added_at > d.added_at OR n.id = ?1)))
         ORDER BY d.added_at, d.id",
    )
    .bind::<Text, _>(me.device_id.to_string())
    .load::<DeviceRow>(conn)?;
    let devices = rows
        .into_iter()
        // A malformed id (only another device could have written one) is
        // left out rather than failing the whole list.
        .filter_map(|r| {
            let id = DeviceId::parse(&r.id).ok()?;
            Some(DeviceStatus {
                is_self: id == me.device_id,
                id,
                name: r.name,
                platform: r.platform,
                last_sync_at: r.last_sync_at,
                last_complete_at: r.last_complete_at,
                complete: r.complete == Some(1),
            })
        })
        .collect();
    Ok(Status {
        device_id: me.device_id,
        name: me.name,
        platform: me.platform,
        in_vault: me.vault_id.is_some(),
        devices,
        offer_active,
        activity,
    })
}

/// What of the learner's this device holds, table by table: what joining
/// another device would replace.
pub fn held(conn: &mut Conn) -> Result<Vec<Held>> {
    Ok(engine::held_data(conn)?.into_iter().map(|(table, rows)| Held { table, rows }).collect())
}

pub fn set_name(conn: &mut Conn, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(Error::msg("A device's name is 1 to 60 characters."));
    }
    db::set_setting(conn, session::NAME_SETTING, name)?;
    let me = session::identity(conn)?;
    diesel::sql_query("UPDATE vault_devices SET name = ? WHERE id = ?")
        .bind::<Text, _>(name)
        .bind::<Text, _>(me.device_id.to_string())
        .execute(conn)?;
    Ok(())
}

/// Takes a device out of the vault: its key goes on the revocation list,
/// which only ever grows, so it's refused from then on everywhere the list
/// reaches — whatever later edits say about its entry. Its data stays where
/// it is.
pub fn remove_device(conn: &mut Conn, id: DeviceId) -> Result<()> {
    let me = session::identity(conn)?;
    if id == me.device_id {
        return Err(Error::msg("A device can't remove itself."));
    }
    conn.immediate_transaction(|conn| {
        diesel::sql_query(
            "INSERT INTO vault_revocations (public_key, device_id, revoked_at)
             SELECT public_key, id, ? FROM vault_devices WHERE id = ?
             ON CONFLICT (public_key) DO NOTHING",
        )
        .bind::<Text, _>(now_iso())
        .bind::<Text, _>(id.to_string())
        .execute(conn)?;
        diesel::sql_query("DELETE FROM vault_devices WHERE id = ?").bind::<Text, _>(id.to_string()).execute(conn)?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// This device, the vault's devices, and what sync is doing.
#[tauri::command]
pub async fn sync_status(app: AppHandle, svc: SyncState<'_>) -> Result<Status> {
    let (offer_active, activity) = (svc.offer_active(), svc.activity());
    with(&app, move |c| status(c, offer_active, activity)).await
}

/// Names this device, as the vault's device list shows it.
#[tauri::command]
pub async fn sync_set_name(app: AppHandle, svc: SyncState<'_>, name: String) -> Result<()> {
    with(&app, move |c| set_name(c, &name)).await?;
    svc.kick();
    Ok(())
}

/// A pairing code and its QR, good for one device for ten minutes.
#[tauri::command]
pub async fn sync_offer(app: AppHandle, svc: SyncState<'_>) -> Result<SyncOffer> {
    svc.offer(&app).await
}

#[tauri::command]
pub fn sync_cancel_offer(svc: SyncState<'_>) {
    svc.cancel_offer();
}

/// Joins the vault behind a scanned code. `replace`: this device holds data
/// and the learner agreed to have it replaced (a copy is saved first).
#[tauri::command]
pub async fn sync_join(app: AppHandle, svc: SyncState<'_>, code: String, replace: bool) -> Result<JoinResult> {
    svc.join(&app, &code, replace).await
}

/// Syncs with the vault's devices now rather than in a few seconds: the
/// Devices screen's button, and the app coming back to the front. Nothing
/// when there's nobody to sync with.
#[tauri::command]
pub async fn sync_now(app: AppHandle, svc: SyncState<'_>) -> Result<()> {
    if with(&app, paired).await? {
        svc.kick();
    }
    Ok(())
}

/// Removes a device from the vault (its key is revoked).
#[tauri::command]
pub async fn sync_remove_device(app: AppHandle, svc: SyncState<'_>, id: DeviceId) -> Result<()> {
    with(&app, move |c| remove_device(c, id)).await?;
    svc.kick();
    Ok(())
}

/// What of the learner's this device holds: for the "replace it?" question
/// before joining.
#[tauri::command]
pub async fn sync_held_data(app: AppHandle) -> Result<Vec<Held>> {
    with(&app, held).await
}
