//! The encrypted channel between two devices: Noise over TCP (as Tally's).
//!
//! - Every connection starts with one plain byte saying what it is for —
//!   [`PURPOSE_SYNC`] between devices of a vault, [`PURPOSE_PAIR`] for a
//!   device joining one — then a Noise handshake:
//!   - sync: `Noise_XX_25519_ChaChaPoly_BLAKE2s`; each side learns the
//!     other's static key and checks it against the vault's device list.
//!   - pairing: `Noise_XXpsk3_…` keyed with the QR code's one-time token, so
//!     only whoever scanned the code gets through, and the joiner checks the
//!     host's key against the one the code carried.
//! - After that, messages of any size up to [`MAX_MESSAGE`]: split into
//!   chunks that each fit one Noise message, every chunk encrypted and
//!   authenticated, the first carrying the total length. A tampered or
//!   truncated stream is an error, never a different message.
//! - Messages are MessagePack (named fields, so a field added later reads
//!   as missing in an older version rather than shifting the others).

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use snow::{Builder, HandshakeState, TransportState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::error::{Error, Result};

pub const PURPOSE_SYNC: u8 = b'S';
pub const PURPOSE_PAIR: u8 = b'P';

const SYNC_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const PAIR_PARAMS: &str = "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s";
/// Plaintext per Noise message: 65535 minus the 16-byte tag, rounded down.
const CHUNK: usize = 65_000;
/// The largest Noise message (and so frame) there is.
const NOISE_MAX: usize = 65_535;
/// Nothing a peer sends may make this device hold more than this at once.
pub const MAX_MESSAGE: usize = 256 * 1024 * 1024;
/// A peer that goes quiet this long is gone.
const IO_TIMEOUT: Duration = Duration::from_secs(60);
/// Until the handshake is done the other side has proven nothing: it gets
/// little time to hold a connection open.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

fn noise(e: snow::Error) -> Error {
    Error::msg(format!("encryption: {e}"))
}

fn timed_out(_: tokio::time::error::Elapsed) -> Error {
    Error::msg("timed out")
}

/// A new X25519 static key pair: (secret, public).
pub fn generate_keypair() -> Result<(Vec<u8>, Vec<u8>)> {
    let params = SYNC_PARAMS.parse().map_err(noise)?;
    let kp = Builder::new(params).generate_keypair().map_err(noise)?;
    Ok((kp.private, kp.public))
}

pub struct Channel {
    stream: TcpStream,
    noise: TransportState,
    /// The other side's static public key, as proven by the handshake.
    pub remote_key: Vec<u8>,
}

async fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>> {
    read_frame_within(stream, IO_TIMEOUT).await
}

async fn read_frame_within(stream: &mut TcpStream, limit: Duration) -> Result<Vec<u8>> {
    let mut len = [0u8; 2];
    timeout(limit, stream.read_exact(&mut len)).await.map_err(timed_out)??;
    let mut buf = vec![0u8; usize::from(u16::from_be_bytes(len))];
    timeout(limit, stream.read_exact(&mut buf)).await.map_err(timed_out)??;
    Ok(buf)
}

async fn write_frame(stream: &mut TcpStream, frame: &[u8]) -> Result<()> {
    let len = u16::try_from(frame.len()).map_err(|_| Error::msg("frame too large"))?;
    let mut out = Vec::with_capacity(frame.len() + 2);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(frame);
    timeout(IO_TIMEOUT, stream.write_all(&out)).await.map_err(timed_out)??;
    Ok(())
}

fn handshake(secret: &[u8], psk: Option<&[u8; 32]>, initiator: bool) -> Result<HandshakeState> {
    let params = if psk.is_some() { PAIR_PARAMS } else { SYNC_PARAMS };
    let mut b = Builder::new(params.parse().map_err(noise)?)
        .local_private_key(secret)
        .prologue(super::app::NOISE_PROLOGUE);
    if let Some(psk) = psk {
        b = b.psk(3, psk);
    }
    if initiator {
        b.build_initiator().map_err(noise)
    } else {
        b.build_responder().map_err(noise)
    }
}

fn finish(stream: TcpStream, hs: HandshakeState) -> Result<Channel> {
    let remote_key = hs.get_remote_static().ok_or_else(|| Error::msg("no remote key"))?.to_vec();
    let noise = hs.into_transport_mode().map_err(noise)?;
    Ok(Channel { stream, noise, remote_key })
}

fn failed(_: snow::Error) -> Error {
    Error::msg("handshake failed")
}

impl Channel {
    /// Opens a channel to `stream`'s far end as the initiator.
    pub async fn initiate(mut stream: TcpStream, purpose: u8, secret: &[u8], psk: Option<&[u8; 32]>) -> Result<Channel> {
        stream.set_nodelay(true).ok();
        timeout(IO_TIMEOUT, stream.write_all(&[purpose])).await.map_err(timed_out)??;
        let mut hs = handshake(secret, psk, true)?;
        let mut buf = vec![0u8; NOISE_MAX];
        // -> e
        let n = hs.write_message(&[], &mut buf).map_err(noise)?;
        write_frame(&mut stream, &buf[..n]).await?;
        // <- e, ee, s, es
        let msg = read_frame_within(&mut stream, HANDSHAKE_TIMEOUT).await?;
        hs.read_message(&msg, &mut buf).map_err(failed)?;
        // -> s, se (+ psk)
        let n = hs.write_message(&[], &mut buf).map_err(noise)?;
        write_frame(&mut stream, &buf[..n]).await?;
        finish(stream, hs)
    }

    /// Reads the purpose byte of an incoming connection.
    pub async fn purpose(stream: &mut TcpStream) -> Result<u8> {
        let mut p = [0u8; 1];
        timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut p)).await.map_err(timed_out)??;
        Ok(p[0])
    }

    /// Completes the handshake of an incoming connection, whose purpose byte
    /// has been read.
    pub async fn respond(mut stream: TcpStream, secret: &[u8], psk: Option<&[u8; 32]>) -> Result<Channel> {
        stream.set_nodelay(true).ok();
        let mut hs = handshake(secret, psk, false)?;
        let mut buf = vec![0u8; NOISE_MAX];
        let msg = read_frame_within(&mut stream, HANDSHAKE_TIMEOUT).await?;
        hs.read_message(&msg, &mut buf).map_err(failed)?;
        let n = hs.write_message(&[], &mut buf).map_err(noise)?;
        write_frame(&mut stream, &buf[..n]).await?;
        let msg = read_frame_within(&mut stream, HANDSHAKE_TIMEOUT).await?;
        hs.read_message(&msg, &mut buf).map_err(failed)?;
        finish(stream, hs)
    }

    pub async fn send_bytes(&mut self, plain: &[u8]) -> Result<()> {
        if plain.len() > MAX_MESSAGE {
            return Err(Error::msg("message too large"));
        }
        let total = u32::try_from(plain.len()).map_err(|_| Error::msg("message too large"))?;
        let mut first = total.to_be_bytes().to_vec();
        let head = plain.len().min(CHUNK - 4);
        first.extend_from_slice(&plain[..head]);
        let mut buf = vec![0u8; NOISE_MAX];
        let n = self.noise.write_message(&first, &mut buf).map_err(noise)?;
        write_frame(&mut self.stream, &buf[..n]).await?;
        for chunk in plain[head..].chunks(CHUNK) {
            let n = self.noise.write_message(chunk, &mut buf).map_err(noise)?;
            write_frame(&mut self.stream, &buf[..n]).await?;
        }
        self.stream.flush().await?;
        Ok(())
    }

    pub async fn recv_bytes(&mut self) -> Result<Vec<u8>> {
        self.recv_bytes_limited(MAX_MESSAGE).await
    }

    /// Receives a message of at most `max` bytes; a longer one is refused
    /// before any of it is held. Before the other side has proven it belongs
    /// to the vault, this keeps what it can make this device hold small.
    pub async fn recv_bytes_limited(&mut self, max: usize) -> Result<Vec<u8>> {
        let unauthentic = |_| Error::msg("message failed authentication");
        let mut buf = vec![0u8; NOISE_MAX];
        let frame = read_frame(&mut self.stream).await?;
        let n = self.noise.read_message(&frame, &mut buf).map_err(unauthentic)?;
        if n < 4 {
            return Err(Error::msg("short message"));
        }
        let announced = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let total = usize::try_from(announced).unwrap_or(usize::MAX);
        if total > max.min(MAX_MESSAGE) {
            return Err(Error::msg("message too large"));
        }
        // Grown as chunks arrive, not reserved up front on the sender's word.
        let mut out = Vec::with_capacity(total.min(1 << 20));
        out.extend_from_slice(&buf[4..n]);
        while out.len() < total {
            let frame = read_frame(&mut self.stream).await?;
            let n = self.noise.read_message(&frame, &mut buf).map_err(unauthentic)?;
            out.extend_from_slice(&buf[..n]);
        }
        if out.len() != total {
            return Err(Error::msg("message longer than announced"));
        }
        Ok(out)
    }

    pub async fn send<T: Serialize>(&mut self, msg: &T) -> Result<()> {
        let bytes = rmp_serde::to_vec_named(msg).map_err(|e| Error::msg(format!("unwritable message: {e}")))?;
        self.send_bytes(&bytes).await
    }

    pub async fn recv<T: DeserializeOwned>(&mut self) -> Result<T> {
        let bytes = self.recv_bytes().await?;
        read(&bytes)
    }

    pub async fn recv_limited<T: DeserializeOwned>(&mut self, max: usize) -> Result<T> {
        let bytes = self.recv_bytes_limited(max).await?;
        read(&bytes)
    }
}

fn read<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    rmp_serde::from_slice(bytes).map_err(|e| Error::msg(format!("unreadable message: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    type Pair = (Result<Channel>, Result<Channel>, Vec<u8>, Vec<u8>);

    async fn pair_up(psk_a: Option<[u8; 32]>, psk_b: Option<[u8; 32]>) -> Pair {
        let (sa, pa) = generate_keypair().unwrap();
        let (sb, pb) = generate_keypair().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let _ = Channel::purpose(&mut s).await.unwrap();
            Channel::respond(s, &sb, psk_b.as_ref()).await
        });
        let stream = TcpStream::connect(addr).await.unwrap();
        let purpose = if psk_a.is_some() { PURPOSE_PAIR } else { PURPOSE_SYNC };
        let client = Channel::initiate(stream, purpose, &sa, psk_a.as_ref()).await;
        let server = server.await.unwrap();
        (client, server, pa, pb)
    }

    #[tokio::test]
    async fn both_sides_learn_each_others_keys_and_talk() {
        let (c, s, pa, pb) = pair_up(None, None).await;
        let (mut c, mut s) = (c.unwrap(), s.unwrap());
        assert_eq!(c.remote_key, pb);
        assert_eq!(s.remote_key, pa);
        // Small, empty and multi-chunk messages, both ways.
        let big: Vec<u8> = (0..1_000_000u32).map(|i| u8::try_from(i % 251).unwrap()).collect();
        for m in [b"hello".to_vec(), vec![], big.clone()] {
            c.send_bytes(&m).await.unwrap();
            assert_eq!(s.recv_bytes().await.unwrap(), m);
            s.send_bytes(&m).await.unwrap();
            assert_eq!(c.recv_bytes().await.unwrap(), m);
        }
    }

    #[tokio::test]
    async fn pairing_needs_the_same_token_on_both_sides() {
        let (c, s, ..) = pair_up(Some([7; 32]), Some([7; 32])).await;
        assert!(c.is_ok() && s.is_ok());
        let (c, s, ..) = pair_up(Some([7; 32]), Some([8; 32])).await;
        assert!(c.is_err() || s.is_err(), "a wrong token must not get through");
    }

    #[tokio::test]
    async fn a_message_over_the_limit_is_refused_before_it_is_read() {
        let (c, s, ..) = pair_up(None, None).await;
        let (mut c, mut s) = (c.unwrap(), s.unwrap());
        c.send_bytes(&vec![1u8; 100_000]).await.unwrap();
        let err = s.recv_bytes_limited(64 * 1024).await.unwrap_err();
        assert_eq!(err.to_string(), "message too large");
    }

    #[tokio::test]
    async fn a_tampered_message_is_refused() {
        let (sa, _) = generate_keypair().unwrap();
        let (sb, _) = generate_keypair().unwrap();
        // A man in the middle that flips one bit of the first data frame.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let relay = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = relay.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut inbound, _) = relay.accept().await.unwrap();
            let mut outbound = TcpStream::connect(addr).await.unwrap();
            let mut p = [0u8; 1];
            inbound.read_exact(&mut p).await.unwrap();
            outbound.write_all(&p).await.unwrap();
            // Three handshake frames (client, server, client), then the
            // client's first data frame, tampered.
            for (i, client_to_server) in [true, false, true, true].into_iter().enumerate() {
                let (from, to) = if client_to_server { (&mut inbound, &mut outbound) } else { (&mut outbound, &mut inbound) };
                let mut f = read_frame(from).await.unwrap();
                if i == 3 {
                    let last = f.len() - 1;
                    f[last] ^= 1;
                }
                write_frame(to, &f).await.unwrap();
            }
        });
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            Channel::purpose(&mut s).await.unwrap();
            let mut ch = Channel::respond(s, &sb, None).await.unwrap();
            ch.recv_bytes().await
        });
        let mut c = Channel::initiate(TcpStream::connect(relay_addr).await.unwrap(), PURPOSE_SYNC, &sa, None).await.unwrap();
        c.send_bytes(b"pay me").await.unwrap();
        let got = tokio::time::timeout(Duration::from_secs(5), server).await.expect("refused at once, not timed out");
        assert_eq!(got.unwrap().unwrap_err().to_string(), "message failed authentication");
    }
}
