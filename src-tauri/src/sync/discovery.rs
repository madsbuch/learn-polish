//! Finding the vault's other devices on the local network (as Tally's).
//!
//! Every device answers on UDP [`app::DISCOVERY_PORT`]. A device looking for
//! its vault sends a probe — to the network's broadcast address, and
//! straight to every address a peer was last seen at — naming its vault by
//! a short tag (never the vault id itself). Devices of that vault answer
//! with their id and the TCP port they sync on. Nothing here is trusted:
//! discovery only says where to knock, and the encrypted handshake decides
//! who is at the door.
//!
//! Plain broadcast rather than mDNS: a phone can send one without holding a
//! multicast lock, and the answer comes back to it directly.
//!
//! The messages: `POLSKI/1 Q <tag> <me>` and `POLSKI/1 R <tag> <id> <port>`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use sha2::{Digest as _, Sha256};
use tokio::net::UdpSocket;
use tokio::time::{timeout, Instant};

use super::app;

/// A device of the vault that answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub device_id: String,
    pub addr: SocketAddr,
}

/// A short tag for the vault: devices of other vaults on the same network
/// ignore each other without learning the vault's id.
pub fn vault_tag(vault_id: &str) -> String {
    hex::encode(&Sha256::digest(format!("{}{vault_id}", app::VAULT_TAG_PREFIX).as_bytes())[..8])
}

fn query_prefix() -> String {
    format!("{} Q", app::DISCOVERY_PROTOCOL)
}

fn reply_prefix() -> String {
    format!("{} R", app::DISCOVERY_PROTOCOL)
}

fn parse_query(msg: &str) -> Option<(&str, &str)> {
    let mut parts = msg.strip_prefix(&query_prefix())?.split_whitespace();
    Some((parts.next()?, parts.next()?))
}

fn parse_reply(msg: &str) -> Option<(&str, &str, u16)> {
    let mut parts = msg.strip_prefix(&reply_prefix())?.split_whitespace();
    Some((parts.next()?, parts.next()?, parts.next()?.parse().ok()?))
}

/// Answers probes for this device's vault until the socket fails. `state`
/// says, at the moment a probe arrives, (vault tag, device id, TCP port) —
/// or nothing while this device isn't in a vault.
pub async fn respond<F>(socket: UdpSocket, state: F)
where
    F: Fn() -> Option<(String, String, u16)>,
{
    let mut buf = [0u8; 512];
    loop {
        let Ok((n, from)) = socket.recv_from(&mut buf).await else { return };
        let Ok(msg) = std::str::from_utf8(&buf[..n]) else { continue };
        let Some((tag, asker)) = parse_query(msg) else { continue };
        let Some((my_tag, me, port)) = state() else { continue };
        if tag != my_tag || asker == me {
            continue;
        }
        let _ = socket.send_to(format!("{} {my_tag} {me} {port}", reply_prefix()).as_bytes(), from).await;
    }
}

/// Binds the answering socket on every interface.
pub async fn bind_responder(port: u16) -> std::io::Result<UdpSocket> {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).await
}

/// Where to send a probe: the broadcast address, plus each address a peer
/// was last reached at (its discovery port, same host).
pub fn targets(last_addrs: &[String], port: u16) -> Vec<SocketAddr> {
    let mut out = vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), port)];
    for a in last_addrs {
        if let Ok(sa) = a.parse::<SocketAddr>() {
            let t = SocketAddr::new(sa.ip(), port);
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

/// Asks who of the vault `tag` is around, listening for answers for `wait`.
pub async fn probe(tag: &str, me: &str, targets: &[SocketAddr], wait: Duration) -> Vec<Found> {
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await else { return vec![] };
    let _ = socket.set_broadcast(true);
    let query = format!("{} {tag} {me}", query_prefix());
    for t in targets {
        let _ = socket.send_to(query.as_bytes(), t).await;
    }
    let deadline = Instant::now() + wait;
    let mut found: Vec<Found> = Vec::new();
    let mut buf = [0u8; 512];
    while let Ok(Ok((n, from))) = timeout(deadline.saturating_duration_since(Instant::now()), socket.recv_from(&mut buf)).await {
        let Ok(msg) = std::str::from_utf8(&buf[..n]) else { continue };
        let Some((t, id, port)) = parse_reply(msg) else { continue };
        if t != tag || id == me {
            continue;
        }
        let f = Found { device_id: id.to_string(), addr: SocketAddr::new(from.ip(), port) };
        if !found.contains(&f) {
            found.push(f);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn devices_of_the_vault_answer_and_others_do_not() {
        let mut answering = Vec::new();
        for (tag, id, port) in [("vault-a", "phone", 5001u16), ("vault-a", "mac", 5002), ("vault-b", "stranger", 5003)] {
            let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let addr = socket.local_addr().unwrap();
            tokio::spawn(respond(socket, move || Some((tag.to_string(), id.to_string(), port))));
            answering.push(addr);
        }
        let mut found = probe("vault-a", "phone", &answering, Duration::from_millis(300)).await;
        found.sort_by(|a, b| a.device_id.cmp(&b.device_id));
        // Not itself, not another vault: just the Mac, at its sync port.
        assert_eq!(found, vec![Found { device_id: "mac".into(), addr: "127.0.0.1:5002".parse().unwrap() }]);
    }

    #[tokio::test]
    async fn a_device_outside_any_vault_stays_quiet() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        tokio::spawn(respond(socket, || None));
        assert!(probe("vault-a", "phone", &[addr], Duration::from_millis(200)).await.is_empty());
    }

    #[test]
    fn junk_is_ignored() {
        assert_eq!(parse_query("hello"), None);
        assert_eq!(parse_reply("POLSKI/1 R tag id notaport"), None);
        assert_eq!(parse_reply("POLSKI/1 R tag id 47822"), Some(("tag", "id", 47822)));
        // Another app's answers are not ours.
        assert_eq!(parse_reply("TALLY/1 R tag id 47822"), None);
        assert_eq!(parse_query("POLSKI/1 Q tag me"), Some(("tag", "me")));
    }

    #[test]
    fn probes_go_to_the_broadcast_address_and_last_known_peers() {
        let t = targets(&["192.168.1.5:47822".into(), "garbage".into(), "192.168.1.5:50000".into()], app::DISCOVERY_PORT);
        assert_eq!(t, vec!["255.255.255.255:47821".parse().unwrap(), "192.168.1.5:47821".parse().unwrap()]);
    }

    #[test]
    fn a_vault_tag_is_eight_bytes_of_its_hash() {
        let tag = vault_tag("v");
        assert_eq!(tag.len(), 16);
        assert_eq!(tag, hex::encode(&Sha256::digest(b"polski-vault:v")[..8]));
        assert_ne!(vault_tag("w"), tag);
    }
}
