//! One sync between two devices, and pairing a new device into a vault
//! (docs/SYNC.md; as Tally's). A sync, after the encrypted handshake:
//!
//! ```text
//! client                              server
//!   Hello  ───────────────────────────▶       both check: same vault, same
//!          ◀─────────────────────────── Hello  schema and build, a key the
//!   Seen   ───────────────────────────▶        vault knows
//!          ◀─────────────────────────── Seen   …neither is an older copy
//!   Pull{since} ──────────────────────▶
//!          ◀─────────────────────────── Batch  …until the batch says done
//!   YourTurn ─────────────────────────▶
//!          ◀─────────────────────────── Pull{since}
//!   Batch ────────────────────────────▶        …until done
//!          ◀─────────────────────────── Finished
//!   Digest ───────────────────────────▶        (SkipDigest when nothing moved
//!          ◀─────────────────────────── Digest  and the last sync matched)
//! ```
//!
//! Pairing: the host shows a QR code ([`PairingOffer`]) holding its
//! addresses, its key and a one-time token. The joiner connects with that
//! token (Noise XXpsk3), checks the host is who the code said, introduces
//! itself (`Join`), and once let in (`Welcome`) empties itself and is added
//! to the vault's device list under the key it proved in the handshake. The
//! same connection then carries on as a sync, which brings everything over.
//!
//! The database is locked only around each engine call ([`with`]), never
//! while waiting on the network: a sync of thousands of rows leaves the
//! screens free between its batches.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Nullable, Text};
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

use super::engine::{self, ApplyStats, Batch, Digest};
use super::wire::{self, Channel, PURPOSE_PAIR, PURPOSE_SYNC};
use super::{app, now_iso, with, Store};
use crate::db::{self, Conn};
use crate::error::{Error, Result};
use crate::ids::DeviceId;
use crate::schema::{sync_epochs, sync_peers, sync_state, vault_devices, vault_revocations};

/// The version of this conversation; two devices only sync on the same one.
pub const PROTOCOL: u32 = 1;
/// Rows and bytes per batch over the wire: big enough to move along, small
/// enough that one transaction never holds the database for long.
const BATCH_ROWS: i64 = 500;
const BATCH_BYTES: usize = 4 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
/// How long a QR code works.
pub const PAIRING_TTL: Duration = Duration::from_secs(10 * 60);
/// The most a device may send before it's known to belong to the vault.
const PRE_AUTH_LIMIT: usize = 64 * 1024;
/// Connections handled at once; more are turned away at the door.
const MAX_CONNECTIONS: usize = 8;
/// What a device answers while it's syncing with another: not a failure,
/// just another time.
pub const BUSY: &str = "busy";
/// The device setting holding the name the learner gave this device.
pub const NAME_SETTING: &str = "sync_device_name";

/// Whether an error is only "the other side is busy".
pub fn is_busy(e: &Error) -> bool {
    matches!(e, Error::Message(m) if m == BUSY)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub device_id: String,
    pub name: String,
    pub platform: String,
    /// `db::SCHEMA_VERSION`: rows only mean the same on the same schema.
    pub schema: i64,
    pub vault_id: Option<String>,
    /// This device's current epoch (`sync_epochs`, see [`current_epoch`]).
    pub epoch: String,
    /// "dev" for a development build, "release" otherwise. The two never sync.
    pub build: String,
    /// The TCP port this device listens on, when it listens (the app does; a
    /// background run doesn't). The side it connected to records where to
    /// reach it: the address the connection came from, at this port — the
    /// connection's own port is a throwaway one. Absent from an older
    /// version, which is then just not recorded.
    #[serde(default)]
    pub port: Option<u16>,
}

/// What kind of build this is, for [`Hello::build`].
pub fn build_kind() -> &'static str {
    if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    }
}

/// Whether two builds may sync: a development build never touches the real
/// app's data, whatever database it was given — unfinished code's mistakes
/// would otherwise reach every device.
fn same_build(a: &str, b: &str) -> bool {
    a == b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Msg {
    Hello(Hello),
    Join { device_id: String, name: String, platform: String },
    Welcome { vault_id: String },
    /// What the sender knows of the receiver's history: how far it has
    /// pulled from it, and the epoch it last saw it in. Further than the
    /// receiver has recorded, or an epoch it never had, means the receiver
    /// is an older copy of itself (restored from a backup) — see
    /// [`check_not_stale`].
    Seen { pulled_from_you: i64, last_epoch: Option<String> },
    Pull { since: i64 },
    Batch(Batch),
    YourTurn,
    Finished,
    Digest(Digest),
    /// Nothing moved either way and the last sync ended identical: they still
    /// are, so the full comparison (a pass over every table) is skipped.
    SkipDigest,
    Refused { reason: String },
}

/// Who this device is, for sync.
#[derive(Debug, Clone)]
pub struct Identity {
    pub device_id: DeviceId,
    pub vault_id: Option<String>,
    pub secret: Vec<u8>,
    pub public: Vec<u8>,
    pub name: String,
    pub platform: String,
}

/// What kind of device this is, as the device list shows it.
pub fn platform() -> &'static str {
    match std::env::consts::OS {
        "android" => "android",
        "ios" => "ios",
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    }
}

fn default_name() -> &'static str {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        "Phone"
    } else {
        "Computer"
    }
}

/// This device's identity, making its key pair the first time.
pub fn identity(conn: &mut Conn) -> Result<Identity> {
    type Row = (DeviceId, Option<String>, Option<Vec<u8>>, Option<Vec<u8>>);
    let (device_id, vault_id, secret, public): Row = sync_state::table
        .find(1)
        .select((sync_state::device_id, sync_state::vault_id, sync_state::secret_key, sync_state::public_key))
        .first(conn)?;
    let (secret, public) = match (secret, public) {
        (Some(s), Some(p)) => (s, p),
        _ => {
            let (s, p) = wire::generate_keypair()?;
            diesel::update(sync_state::table.find(1))
                .set((sync_state::secret_key.eq(&s), sync_state::public_key.eq(&p)))
                .execute(conn)?;
            (s, p)
        }
    };
    let name = db::get_setting(conn, NAME_SETTING)?
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| default_name().to_string());
    Ok(Identity { device_id, vault_id, secret, public, name, platform: platform().to_string() })
}

fn hello(conn: &mut Conn, me: &Identity) -> Result<Hello> {
    Ok(Hello {
        protocol: PROTOCOL,
        device_id: me.device_id.to_string(),
        name: me.name.clone(),
        platform: me.platform.clone(),
        schema: db::SCHEMA_VERSION,
        vault_id: me.vault_id.clone(),
        epoch: current_epoch(conn)?,
        build: build_kind().to_string(),
        port: None,
    })
}

/// The hello for a session about to start, with the port this device
/// listens on, if it does.
async fn introduce<S: Store>(store: &S) -> Result<Hello> {
    let mut hello = with(store, |c| {
        let me = identity(c)?;
        hello(c, &me)
    })
    .await?;
    hello.port = store.listen_port();
    Ok(hello)
}

/// Where a device that connected here can be reached: the address its
/// connection came from, at the port it says it listens on. Nothing when it
/// doesn't listen (or doesn't say): its connection's port is a throwaway one,
/// and recording that would replace an address that works.
fn reachable_at(from: Option<SocketAddr>, theirs: &Hello) -> Option<SocketAddr> {
    match (from, theirs.port) {
        (Some(from), Some(port)) if port != 0 => Some(SocketAddr::new(from.ip(), port)),
        _ => None,
    }
}

#[derive(QueryableByName)]
struct EpochRow {
    #[diesel(sql_type = Text)]
    epoch: String,
    #[diesel(sql_type = BigInt)]
    seq: i64,
}

/// The epoch to introduce this device with: the latest, or a new one if
/// anything has been written since it began. Peers remember it; a copy of
/// this database from before it — restored from a backup, a folder copied
/// back — never had it, and the first peer to meet that copy says so.
fn current_epoch(conn: &mut Conn) -> Result<String> {
    let seq = engine::local_state(conn)?.seq;
    let latest = diesel::sql_query("SELECT epoch, seq FROM sync_epochs ORDER BY rowid DESC LIMIT 1")
        .load::<EpochRow>(conn)?
        .into_iter()
        .next();
    match latest {
        Some(e) if e.seq >= seq => Ok(e.epoch),
        _ => {
            let epoch = super::new_marker();
            diesel::insert_into(sync_epochs::table)
                .values((sync_epochs::epoch.eq(&epoch), sync_epochs::seq.eq(seq), sync_epochs::created_at.eq(now_iso())))
                .execute(conn)?;
            Ok(epoch)
        }
    }
}

fn last_epoch_of(conn: &mut Conn, peer: &str) -> Result<Option<String>> {
    Ok(sync_peers::table.find(peer).select(sync_peers::last_epoch).first(conn).optional()?.flatten())
}

/// What this device tells a peer it knows of the peer's history.
fn seen(conn: &mut Conn, peer: &str) -> Result<Msg> {
    Ok(Msg::Seen { pulled_from_you: engine::pulled_from(conn, peer)?, last_epoch: last_epoch_of(conn, peer)? })
}

fn revoked(conn: &mut Conn, key_hex: &str) -> Result<bool> {
    Ok(vault_revocations::table.find(key_hex).count().get_result::<i64>(conn)? > 0)
}

/// Before anything else is read from a connection: whether the key it
/// proved belongs to a device of this vault (and isn't revoked). A stranger
/// is turned away before it can send anything of size.
fn key_known(conn: &mut Conn, key: &[u8]) -> Result<bool> {
    let k = hex::encode(key);
    let listed = vault_devices::table.filter(vault_devices::public_key.eq(&k)).count().get_result::<i64>(conn)? > 0;
    Ok(listed && !revoked(conn, &k)?)
}

/// Whether `theirs` may sync with this device: the same protocol, schema
/// and kind of build, the same vault, a key the vault lists and hasn't
/// revoked — or, while joining (the device list is still empty here), the
/// key the QR code named.
///
/// A device is known by its key; its id can change (a restored copy takes a
/// new one, see instance.rs), and the new id arrives with the sync itself.
fn check_peer(conn: &mut Conn, me: &Hello, theirs: &Hello, proven_key: &[u8], vouched: Option<&[u8]>) -> Result<()> {
    if theirs.protocol != me.protocol {
        return Err(Error::msg(format!("{} runs a different version of sync; update both devices", theirs.name)));
    }
    if theirs.schema != me.schema {
        return Err(Error::msg(format!(
            "{} is on a different version of {} (data version {} here, {} there); update both devices",
            theirs.name,
            app::APP_NAME,
            me.schema,
            theirs.schema
        )));
    }
    if !same_build(&me.build, &theirs.build) {
        return Err(Error::msg(format!(
            "{} is a {} build and this is a {} one: development builds never sync with the real app",
            theirs.name, theirs.build, me.build
        )));
    }
    if me.vault_id.is_none() || theirs.vault_id != me.vault_id {
        return Err(Error::msg("not in the same vault"));
    }
    if DeviceId::parse(&theirs.device_id).is_err() {
        return Err(Error::msg(format!("{} sent a malformed device id", theirs.name)));
    }
    if theirs.device_id == me.device_id {
        return Err(Error::msg("the other device has this device's id: one of them is a copy"));
    }
    if let Some(k) = vouched {
        return if k == proven_key { Ok(()) } else { Err(Error::msg("the host's key changed mid-pairing")) };
    }
    let key = hex::encode(proven_key);
    if revoked(conn, &key)? {
        return Err(Error::msg(format!("{} was removed from this vault", theirs.name)));
    }
    let listed: Option<String> =
        vault_devices::table.find(&theirs.device_id).select(vault_devices::public_key).first(conn).optional()?;
    match listed {
        Some(k) if k == key => Ok(()),
        Some(_) => Err(Error::msg(format!("{} presented a key the vault doesn't know it by", theirs.name))),
        None if key_known(conn, proven_key)? => Ok(()),
        None => Err(Error::msg(format!("{} isn't (or is no longer) in this vault", theirs.name))),
    }
}

/// After the hellos, each side says what it knows of the other's history. A
/// device that's been pulled from further than it has ever recorded, or seen
/// in an epoch it never had, is an older copy of itself: it takes a new id
/// (`instance::rotate`) and the sync stops here — the next one starts clean.
fn check_not_stale(conn: &mut Conn, pulled_from_me: i64, last_epoch: Option<String>) -> Result<()> {
    let mine = engine::local_state(conn)?.seq;
    let unknown_epoch = match &last_epoch {
        Some(e) => sync_epochs::table.find(e).count().get_result::<i64>(conn)? == 0,
        None => false,
    };
    if pulled_from_me > mine || unknown_epoch {
        let new_id = super::instance::rotate(conn)?;
        crate::diagnostics::note("sync", format!("This database is an older copy of itself; it is now device {new_id}."));
        return Err(Error::msg(
            "this device's data is an older copy of itself (restored?); it has taken a new id and will sync from scratch",
        ));
    }
    Ok(())
}

/// What a finished sync did.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub peer_id: String,
    pub peer_name: String,
    /// What this device took in.
    pub received: Received,
    /// Whether both devices ended holding exactly the same data.
    pub complete: bool,
    /// Tables that differed, when not.
    pub differences: Vec<String>,
    /// Nothing moved and the last sync had ended identical, so the full
    /// comparison was skipped.
    pub digest_skipped: bool,
}

/// What a sync changed on this device: the payload of `sync-changed`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Received {
    /// Rows taken over as they came.
    pub applied: usize,
    /// Rows changed on both devices, settled by their clocks.
    pub conflicts: usize,
    /// Rows dropped for clashing with another on a unique column.
    pub unique_merges: usize,
    /// Rows removed after the sync because their parent was deleted elsewhere.
    pub removed: usize,
}

impl Received {
    fn add(&mut self, s: ApplyStats) {
        self.applied += s.applied;
        self.conflicts += s.conflicts;
        self.unique_merges += s.unique_merges;
    }

    pub fn changed_anything(&self) -> bool {
        self.applied + self.conflicts + self.unique_merges + self.removed > 0
    }
}

fn record_outcome(conn: &mut Conn, peer: &str, epoch: &str, addr: Option<SocketAddr>, complete: bool) -> Result<()> {
    diesel::sql_query(
        "INSERT INTO sync_peers (device_id, last_addr, last_sync_at, complete, last_complete_at, last_epoch)
         VALUES (?1, ?2, ?3, ?4, CASE WHEN ?4 = 1 THEN ?3 END, ?5)
         ON CONFLICT (device_id) DO UPDATE SET
           last_addr = coalesce(excluded.last_addr, last_addr),
           last_sync_at = excluded.last_sync_at,
           complete = excluded.complete,
           last_complete_at = CASE WHEN excluded.complete = 1 THEN excluded.last_sync_at ELSE last_complete_at END,
           last_epoch = excluded.last_epoch",
    )
    .bind::<Text, _>(peer)
    .bind::<Nullable<Text>, _>(addr.map(|a| a.to_string()))
    .bind::<Text, _>(now_iso())
    .bind::<BigInt, _>(i64::from(complete))
    .bind::<Text, _>(epoch)
    .execute(conn)?;
    Ok(())
}

/// Whether the last finished sync with `peer` ended with both identical.
fn was_complete(conn: &mut Conn, peer: &str) -> Result<bool> {
    Ok(sync_peers::table.find(peer).select(sync_peers::complete).first::<i64>(conn).optional()? == Some(1))
}

async fn expect(ch: &mut Channel) -> Result<Msg> {
    match ch.recv::<Msg>().await? {
        Msg::Refused { reason } => Err(Error::msg(reason)),
        m => Ok(m),
    }
}

/// Pulls from the far side until it says it's done.
async fn pull<S: Store>(ch: &mut Channel, store: &S, peer: &str) -> Result<Received> {
    let mut got = Received::default();
    loop {
        let p = peer.to_string();
        let since = with(store, move |c| engine::pulled_from(c, &p)).await?;
        ch.send(&Msg::Pull { since }).await?;
        let Msg::Batch(batch) = expect(ch).await? else {
            return Err(Error::msg("expected a batch"));
        };
        let done = batch.done;
        let p = peer.to_string();
        match with(store, move |c| engine::apply(c, &p, &batch)).await {
            Ok(s) => got.add(s),
            Err(e) => {
                let _ = ch.send(&Msg::Refused { reason: format!("couldn't apply: {e}") }).await;
                return Err(e);
            }
        }
        if done {
            return Ok(got);
        }
    }
}

/// Answers the far side's pulls until it hands the turn back with `end`.
/// Returns how many changes it sent.
async fn serve_pulls<S: Store>(ch: &mut Channel, store: &S, peer: &str, end: &Msg) -> Result<usize> {
    let mut sent = 0;
    loop {
        match expect(ch).await? {
            Msg::Pull { since } => {
                let p = peer.to_string();
                let batch = with(store, move |c| engine::export(c, since, &p, BATCH_ROWS, BATCH_BYTES)).await?;
                sent += batch.changes.len();
                ch.send(&Msg::Batch(batch)).await?;
            }
            m if &m == end => return Ok(sent),
            other => return Err(Error::msg(format!("unexpected message: {}", kind(&other)))),
        }
    }
}

/// A message's name, for errors (not its contents, which can be large).
fn kind(m: &Msg) -> &'static str {
    match m {
        Msg::Hello(_) => "Hello",
        Msg::Join { .. } => "Join",
        Msg::Welcome { .. } => "Welcome",
        Msg::Seen { .. } => "Seen",
        Msg::Pull { .. } => "Pull",
        Msg::Batch(_) => "Batch",
        Msg::YourTurn => "YourTurn",
        Msg::Finished => "Finished",
        Msg::Digest(_) => "Digest",
        Msg::SkipDigest => "SkipDigest",
        Msg::Refused { .. } => "Refused",
    }
}

/// Compares digests, records how the sync ended, and settles rows whose
/// parent was deleted elsewhere — what both sides do at the end of a sync
/// that moved something.
fn finish_sync(conn: &mut Conn, theirs: &Hello, their_digest: &Digest, addr: Option<SocketAddr>) -> Result<(bool, Vec<String>, usize)> {
    let mine = engine::digest(conn)?;
    let differences = engine::digest_differences(&mine, their_digest);
    let complete = differences.is_empty();
    record_outcome(conn, &theirs.device_id, &theirs.epoch, addr, complete)?;
    let removed = engine::remove_orphans(conn)?;
    Ok((complete, differences, removed))
}

async fn client_session<S: Store>(
    ch: &mut Channel,
    store: &S,
    me: &Hello,
    addr: Option<SocketAddr>,
    vouched: Option<Vec<u8>>,
) -> Result<Outcome> {
    ch.send(&Msg::Hello(me.clone())).await?;
    let theirs = match ch.recv_limited::<Msg>(PRE_AUTH_LIMIT).await? {
        Msg::Refused { reason } => return Err(Error::msg(reason)),
        Msg::Hello(h) => h,
        _ => return Err(Error::msg("expected a hello")),
    };
    let peer = theirs.device_id.clone();
    {
        let (me, theirs, key) = (me.clone(), theirs.clone(), ch.remote_key.clone());
        with(store, move |c| check_peer(c, &me, &theirs, &key, vouched.as_deref())).await?;
    }
    let p = peer.clone();
    ch.send(&with(store, move |c| seen(c, &p)).await?).await?;
    let Msg::Seen { pulled_from_you, last_epoch } = expect(ch).await? else {
        return Err(Error::msg("expected how far it has pulled"));
    };
    with(store, move |c| check_not_stale(c, pulled_from_you, last_epoch)).await?;
    let mut received = pull(ch, store, &peer).await?;
    ch.send(&Msg::YourTurn).await?;
    let sent = serve_pulls(ch, store, &peer, &Msg::Finished).await?;
    // A sync that moved nothing, after one that ended identical: still
    // identical. Skipping the comparison makes the frequent idle syncs cheap.
    let p = peer.clone();
    let idle = !received.changed_anything() && sent == 0 && with(store, move |c| was_complete(c, &p)).await?;
    if idle {
        ch.send(&Msg::SkipDigest).await?;
        let (p, epoch) = (peer.clone(), theirs.epoch.clone());
        with(store, move |c| record_outcome(c, &p, &epoch, addr, true)).await?;
        return Ok(Outcome { peer_id: peer, peer_name: theirs.name, received, complete: true, differences: vec![], digest_skipped: true });
    }
    ch.send(&Msg::Digest(with(store, engine::digest).await?)).await?;
    let Msg::Digest(their_digest) = expect(ch).await? else {
        return Err(Error::msg("expected a digest"));
    };
    let t = theirs.clone();
    let (complete, differences, removed) = with(store, move |c| finish_sync(c, &t, &their_digest, addr)).await?;
    received.removed = removed;
    Ok(Outcome { peer_id: peer, peer_name: theirs.name, received, complete, differences, digest_skipped: false })
}

/// The listening side of a sync. `from` is where the connection came from:
/// recorded, with the port the other side listens on, as where to reach it
/// next time — so a computer learns where the phone is from the phone's own
/// syncs, without depending on discovery (which a network may block).
async fn server_session<S: Store>(ch: &mut Channel, store: &S, me: &Hello, theirs: Hello, from: Option<SocketAddr>) -> Result<Outcome> {
    let addr = reachable_at(from, &theirs);
    let checked = {
        let (me, theirs, key) = (me.clone(), theirs.clone(), ch.remote_key.clone());
        with(store, move |c| check_peer(c, &me, &theirs, &key, None)).await
    };
    if let Err(reason) = checked {
        let _ = ch.send(&Msg::Refused { reason: reason.to_string() }).await;
        return Err(reason);
    }
    ch.send(&Msg::Hello(me.clone())).await?;
    let peer = theirs.device_id.clone();
    let Msg::Seen { pulled_from_you, last_epoch } = expect(ch).await? else {
        return Err(Error::msg("expected how far it has pulled"));
    };
    let p = peer.clone();
    ch.send(&with(store, move |c| seen(c, &p)).await?).await?;
    with(store, move |c| check_not_stale(c, pulled_from_you, last_epoch)).await?;
    serve_pulls(ch, store, &peer, &Msg::YourTurn).await?;
    let mut received = pull(ch, store, &peer).await?;
    ch.send(&Msg::Finished).await?;
    let their_digest = match expect(ch).await? {
        Msg::Digest(d) => d,
        // The client saw nothing move after a sync that ended identical.
        Msg::SkipDigest => {
            let (p, epoch) = (peer.clone(), theirs.epoch.clone());
            with(store, move |c| record_outcome(c, &p, &epoch, addr, true)).await?;
            return Ok(Outcome { peer_id: peer, peer_name: theirs.name, received, complete: true, differences: vec![], digest_skipped: true });
        }
        _ => return Err(Error::msg("expected a digest")),
    };
    ch.send(&Msg::Digest(with(store, engine::digest).await?)).await?;
    let t = theirs.clone();
    let (complete, differences, removed) = with(store, move |c| finish_sync(c, &t, &their_digest, addr)).await?;
    received.removed = removed;
    Ok(Outcome { peer_id: peer, peer_name: theirs.name, received, complete, differences, digest_skipped: false })
}

// ---------------------------------------------------------------------------
// One sync at a time, per device
// ---------------------------------------------------------------------------

/// Devices (by id) with a sync running in this process. Keyed by device
/// rather than one lock for the process, so the tests can run two devices in
/// one process; in the app there's one device.
static BUSY_DEVICES: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

pub(crate) struct Busy(String);

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY_DEVICES.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

pub(crate) fn try_busy(device: &DeviceId) -> Option<Busy> {
    let id = device.to_string();
    let mut b = BUSY_DEVICES.lock().unwrap_or_else(|e| e.into_inner());
    // Lazily: a guard made and dropped on refusal would free the other
    // sync's place (and deadlock on this lock).
    #[allow(clippy::unnecessary_lazy_evaluations)]
    b.insert(id.clone()).then(|| Busy(id))
}

async fn connect(addr: SocketAddr) -> Result<TcpStream> {
    timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| Error::msg(format!("{addr}: no answer")))?
        .map_err(|e| Error::msg(format!("{addr}: {e}")))
}

/// Syncs with the device at `addr`. `Ok(None)` when a sync is already running
/// here or there (it'll come round again).
pub async fn sync_with<S: Store>(store: &S, addr: SocketAddr) -> Result<Option<Outcome>> {
    let me = with(store, identity).await?;
    let Some(_guard) = try_busy(&me.device_id) else {
        return Ok(None);
    };
    let stream = connect(addr).await?;
    let mut ch = Channel::initiate(stream, PURPOSE_SYNC, &me.secret, None).await?;
    // Whoever answered at a discovered address has to be one of the vault's
    // devices before this one sends or reads anything more.
    let key = ch.remote_key.clone();
    if !with(store, move |c| key_known(c, &key)).await? {
        return Err(Error::msg(format!("{addr}: answered by a device outside the vault")));
    }
    let hello = introduce(store).await?;
    match client_session(&mut ch, store, &hello, Some(addr), None).await {
        // The other side is mid-sync with someone (or just finishing with
        // this one): not a failure, just another time.
        Err(e) if is_busy(&e) => Ok(None),
        r => r.map(Some),
    }
}

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------

/// What the QR code carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairingOffer {
    pub vault_id: String,
    pub host_id: String,
    pub host_name: String,
    #[serde(with = "serde_bytes")]
    pub host_key: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub token: [u8; 32],
    pub addrs: Vec<String>,
}

impl PairingOffer {
    /// The code as text: the scheme, then the offer in MessagePack, in
    /// URL-safe base64 (what a QR code holds well).
    pub fn encode(&self) -> Result<String> {
        let bytes = rmp_serde::to_vec(self).map_err(|e| Error::msg(format!("could not write the pairing code: {e}")))?;
        Ok(format!("{}{}", app::PAIRING_SCHEME, base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn decode(s: &str) -> Result<PairingOffer> {
        let body = s
            .trim()
            .strip_prefix(app::PAIRING_SCHEME)
            .ok_or_else(|| Error::msg(format!("not a {} pairing code", app::APP_NAME)))?;
        let damaged = || Error::msg("the pairing code is damaged");
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(body).map_err(|_| damaged())?;
        rmp_serde::from_slice(&bytes).map_err(|_| damaged())
    }
}

/// A QR code on screen, waiting to be scanned.
#[derive(Clone)]
pub struct PendingPairing {
    pub token: [u8; 32],
    pub expires: Instant,
}

pub type PairingSlot = Arc<Mutex<Option<PendingPairing>>>;

/// Puts this device in a vault of its own if it isn't in one, and makes sure
/// it is listed in its vault's devices (with its name, key and platform).
pub fn ensure_vault(conn: &mut Conn) -> Result<Identity> {
    let mut me = identity(conn)?;
    if me.vault_id.is_none() {
        let vault = super::new_vault_id();
        diesel::update(sync_state::table.find(1)).set(sync_state::vault_id.eq(&vault)).execute(conn)?;
        me.vault_id = Some(vault);
    }
    diesel::insert_into(vault_devices::table)
        .values((
            vault_devices::id.eq(me.device_id.to_string()),
            vault_devices::name.eq(&me.name),
            vault_devices::public_key.eq(hex::encode(&me.public)),
            vault_devices::platform.eq(&me.platform),
            vault_devices::added_at.eq(now_iso()),
        ))
        .on_conflict(vault_devices::id)
        .do_nothing()
        .execute(conn)?;
    Ok(me)
}

/// Starts a pairing: a fresh one-time token, and the code to show.
pub async fn offer<S: Store>(store: &S, slot: &PairingSlot, addrs: Vec<String>) -> Result<PairingOffer> {
    let me = with(store, ensure_vault).await?;
    let token: [u8; 32] = rand::random();
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(PendingPairing { token, expires: Instant::now() + PAIRING_TTL });
    Ok(PairingOffer {
        vault_id: me.vault_id.unwrap_or_default(),
        host_id: me.device_id.to_string(),
        host_name: me.name,
        host_key: me.public,
        token,
        addrs,
    })
}

/// Joins the vault `offer` came from. Without `replace` this device must hold
/// none of the learner's data; with it, whatever it holds is replaced by the
/// vault's (the caller saves a copy first: `service::join`). Ends with
/// everything the host has.
pub async fn join<S: Store>(store: &S, offer: &PairingOffer, replace: bool) -> Result<Outcome> {
    let held = with(store, engine::data_tables).await?;
    if !held.is_empty() && !replace {
        return Err(Error::msg(format!(
            "this device already holds data ({}), so it can't join without replacing it",
            held.join(", ")
        )));
    }
    let me = with(store, identity).await?;
    let Some(_guard) = try_busy(&me.device_id) else {
        return Err(Error::msg("a sync is running; try again in a moment"));
    };
    // A fresh key for a fresh member: a device once removed (its key revoked)
    // can be paired again, and a key never outlives the device's place. Kept
    // in memory until the host lets this device in: a failed join changes
    // nothing here.
    let (secret, public) = wire::generate_keypair()?;
    let new_id = DeviceId::new();
    let mut last_error = String::from("the code holds no address");
    for addr in &offer.addrs {
        let Ok(addr) = addr.parse::<SocketAddr>() else { continue };
        let stream = match connect(addr).await {
            Ok(s) => s,
            Err(e) => {
                last_error = e.to_string();
                continue;
            }
        };
        let Ok(mut ch) = Channel::initiate(stream, PURPOSE_PAIR, &secret, Some(&offer.token)).await else {
            return Err(Error::msg("the code has expired or was already used; show a new one"));
        };
        if ch.remote_key != offer.host_key {
            return Err(Error::msg("the device that answered isn't the one that showed the code"));
        }
        // The host is who the code said. Ask to be let in; only once it has,
        // empty out — a refusal leaves this device exactly as it was.
        ch.send(&Msg::Join { device_id: new_id.to_string(), name: me.name.clone(), platform: me.platform.clone() }).await?;
        // A wrong token only shows on the host's side of the handshake (the
        // key is mixed in at its last message): the host hangs up, and that
        // is what this side sees.
        match ch.recv::<Msg>().await {
            Ok(Msg::Welcome { vault_id }) if vault_id == offer.vault_id => {}
            Ok(Msg::Refused { reason }) => return Err(Error::msg(reason)),
            Ok(_) => return Err(Error::msg("the host didn't let this device in")),
            Err(_) => return Err(Error::msg("the code has expired or was already used; show a new one")),
        }
        let (vault, s, p) = (offer.vault_id.clone(), secret.clone(), public.clone());
        with(store, move |c| engine::reset_for_join(c, &vault, new_id, Some((&s, &p)), replace)).await?;
        let hello = introduce(store).await?;
        return client_session(&mut ch, store, &hello, Some(addr), Some(offer.host_key.clone())).await;
    }
    Err(Error::msg(format!("couldn't reach {}: {last_error}", offer.host_name)))
}

// ---------------------------------------------------------------------------
// The listening side
// ---------------------------------------------------------------------------

/// Lets a joining device in: listed under the key it proved, whatever it
/// claims — unless that key was revoked or the id is another device's.
fn admit(conn: &mut Conn, device_id: &str, name: &str, platform: &str, key: &[u8]) -> Result<Identity> {
    let me = ensure_vault(conn)?;
    let key = hex::encode(key);
    if DeviceId::parse(device_id).is_err() {
        return Err(Error::msg("refused a join with a malformed device id"));
    }
    let taken: Option<String> = vault_devices::table.find(device_id).select(vault_devices::public_key).first(conn).optional()?;
    if revoked(conn, &key)? || taken.is_some_and(|k| k != key) {
        return Err(Error::msg("refused a join under another device's id or a revoked key"));
    }
    let name: String = name.trim().chars().take(60).collect();
    let platform: String = platform.chars().take(20).collect();
    diesel::insert_into(vault_devices::table)
        .values((
            vault_devices::id.eq(device_id),
            vault_devices::name.eq(name),
            vault_devices::public_key.eq(&key),
            vault_devices::platform.eq(platform),
            vault_devices::added_at.eq(now_iso()),
        ))
        .on_conflict(vault_devices::id)
        .do_nothing()
        .execute(conn)?;
    Ok(me)
}

/// Handles one incoming connection: a sync, or a device joining with the
/// code currently on screen.
pub async fn serve<S: Store>(store: &S, mut stream: TcpStream, slot: &PairingSlot) -> Result<Outcome> {
    let from = stream.peer_addr().ok();
    let me = with(store, identity).await?;
    match Channel::purpose(&mut stream).await? {
        PURPOSE_SYNC => {
            let mut ch = Channel::respond(stream, &me.secret, None).await?;
            let key = ch.remote_key.clone();
            if !with(store, move |c| key_known(c, &key)).await? {
                return Err(Error::msg("a device outside the vault knocked"));
            }
            let Msg::Hello(theirs) = ch.recv_limited::<Msg>(PRE_AUTH_LIMIT).await? else {
                return Err(Error::msg("expected a hello"));
            };
            let Some(_guard) = try_busy(&me.device_id) else {
                let _ = ch.send(&Msg::Refused { reason: BUSY.into() }).await;
                return Err(Error::msg(BUSY));
            };
            let hello = introduce(store).await?;
            server_session(&mut ch, store, &hello, theirs, from).await
        }
        PURPOSE_PAIR => {
            let token = {
                let slot = slot.lock().unwrap_or_else(|e| e.into_inner());
                match slot.as_ref() {
                    Some(p) if p.expires > Instant::now() => p.token,
                    _ => return Err(Error::msg("a device tried to pair, but no pairing code is showing")),
                }
            };
            let mut ch = Channel::respond(stream, &me.secret, Some(&token)).await?;
            // Busy: say so before the code is used up, so it can be scanned again.
            let Some(_guard) = try_busy(&me.device_id) else {
                let _ = ch.send(&Msg::Refused { reason: "busy; scan the code again in a moment".into() }).await;
                return Err(Error::msg(BUSY));
            };
            // The token is good for one device only.
            {
                let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
                match slot.as_ref() {
                    Some(p) if p.token == token => *slot = None,
                    _ => return Err(Error::msg("pairing code already used")),
                }
            }
            let Msg::Join { device_id, name, platform } = ch.recv_limited::<Msg>(PRE_AUTH_LIMIT).await? else {
                return Err(Error::msg("expected a join"));
            };
            let key = ch.remote_key.clone();
            let admitted = with(store, move |c| admit(c, &device_id, &name, &platform, &key)).await;
            let me = match admitted {
                Ok(me) => me,
                Err(e) => {
                    let _ = ch.send(&Msg::Refused { reason: "that device can't join".into() }).await;
                    return Err(e);
                }
            };
            ch.send(&Msg::Welcome { vault_id: me.vault_id.clone().unwrap_or_default() }).await?;
            let Msg::Hello(theirs) = expect(&mut ch).await? else {
                return Err(Error::msg("expected a hello"));
            };
            let hello = introduce(store).await?;
            server_session(&mut ch, store, &hello, theirs, from).await
        }
        _ => Err(Error::msg(format!("not a {} connection", app::APP_NAME))),
    }
}

/// Listens for other devices, on `preferred_port` if it's free (any free
/// port otherwise; discovery says which). Each connection is handled on its
/// own task; `on_outcome` hears how it went.
pub async fn listen<S, F>(
    store: S,
    slot: PairingSlot,
    bind_ip: &str,
    preferred_port: u16,
    on_outcome: F,
) -> Result<(u16, tokio::task::JoinHandle<()>)>
where
    S: Store,
    F: Fn(Result<Outcome>) + Send + Sync + 'static,
{
    let listener = match TcpListener::bind((bind_ip, preferred_port)).await {
        Ok(l) => l,
        Err(_) => TcpListener::bind((bind_ip, 0)).await?,
    };
    let port = listener.local_addr()?.port();
    let on_outcome = Arc::new(on_outcome);
    let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let handle = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                // Out of file descriptors, say: a pause rather than a spin.
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            };
            let Ok(permit) = permits.clone().try_acquire_owned() else {
                drop(stream);
                continue;
            };
            let (store, slot, on_outcome) = (store.clone(), slot.clone(), on_outcome.clone());
            tokio::spawn(async move {
                on_outcome(serve(&store, stream, &slot).await);
                drop(permit);
            });
        }
    });
    Ok((port, handle))
}

/// This device's addresses on the local network, for the QR code.
pub fn local_addrs(port: u16) -> Vec<String> {
    let mut out: Vec<String> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(v4) if v4.is_private() || v4.is_link_local() => Some(format!("{v4}:{port}")),
            _ => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests;
