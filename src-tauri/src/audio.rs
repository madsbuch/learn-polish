//! Audio formats. The speech model sends raw samples, which ai.rs wraps as
//! WAV; kept that way a minute of speech is nearly 3 MB. Everything spoken is
//! therefore stored as MP3 — about an eighth of the size — and a podcast,
//! which is clips joined sample by sample, reads its stored clips back.
//!
//! Encoding is LAME (compiled in, like SQLite); decoding is Symphonia.
//!
//! Speech stored as WAV before this is made smaller once, in the background
//! ([`compress_stored`]): the same sound, re-encoded, under a new file id.

use diesel::connection::SimpleConnection;
use mp3lame_encoder::{Bitrate, Builder, FlushGap, InterleavedPcm, MonoPcm, Quality};
use tauri::{AppHandle, Manager};

use crate::db::Db;
use crate::error::{Error, Result};
use crate::files;

pub const MP3: &str = "audio/mpeg";
pub const WAV: &str = "audio/wav";

/// The names WAV goes by.
pub const WAV_TYPES: &[&str] = &["audio/wav", "audio/x-wav", "audio/wave"];

/// Samples an MP3 starts with that were never in the sound: LAME's delay
/// (576) and the decoder's (529).
const MP3_DELAY: usize = 1105;

pub fn is_wav(mime: &str) -> bool {
    WAV_TYPES.contains(&mime)
}

/// The samples of a 16-bit PCM WAV clip: its rate, channels and data.
pub fn pcm_of(wav: &[u8]) -> Result<(u32, u16, &[u8])> {
    let bad = || Error::msg("A speech clip was not readable audio.");
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err(bad());
    }
    let mut at = 12;
    let mut format: Option<(u32, u16)> = None;
    while at + 8 <= wav.len() {
        let id = &wav[at..at + 4];
        let size = usize::try_from(u32::from_le_bytes([wav[at + 4], wav[at + 5], wav[at + 6], wav[at + 7]])).unwrap_or(usize::MAX);
        let body = at + 8;
        let end = body.saturating_add(size).min(wav.len());
        match id {
            b"fmt " if end - body >= 16 => {
                let tag = u16::from_le_bytes([wav[body], wav[body + 1]]);
                let channels = u16::from_le_bytes([wav[body + 2], wav[body + 3]]);
                let rate = u32::from_le_bytes([wav[body + 4], wav[body + 5], wav[body + 6], wav[body + 7]]);
                let bits = u16::from_le_bytes([wav[body + 14], wav[body + 15]]);
                if tag != 1 || bits != 16 || channels == 0 || rate == 0 {
                    return Err(Error::msg("A speech clip was not 16-bit PCM audio."));
                }
                format = Some((rate, channels));
            }
            b"data" => {
                let (rate, channels) = format.ok_or_else(bad)?;
                let frame = 2 * usize::from(channels);
                let data = &wav[body..end];
                return Ok((rate, channels, &data[..data.len() - data.len() % frame]));
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        at = body.saturating_add(size).saturating_add(size & 1);
    }
    Err(bad())
}

/// A WAV clip as MP3: 48 kbit/s for one channel, which speech at the model's
/// 24 kHz does not need more than, at a constant rate so that seeking by
/// byte lands where it should.
pub fn mp3_from_wav(wav: &[u8]) -> Result<Vec<u8>> {
    let (rate, channels, data) = pcm_of(wav)?;
    let failed = |what: &dyn std::fmt::Display| Error::msg(format!("Could not make an MP3 of the audio: {what}"));
    let (count, bitrate) = match channels {
        1 => (1u8, Bitrate::Kbps48),
        2 => (2u8, Bitrate::Kbps96),
        n => return Err(failed(&format!("{n} channels"))),
    };
    let samples: Vec<i16> = data.chunks_exact(2).map(|pair| i16::from_le_bytes([pair[0], pair[1]])).collect();
    if samples.is_empty() {
        return Err(failed(&"it is empty"));
    }
    let mut builder = Builder::new().ok_or_else(|| failed(&"no encoder"))?;
    builder.set_num_channels(count).map_err(|e| failed(&e))?;
    builder.set_sample_rate(rate).map_err(|e| failed(&e))?;
    builder.set_brate(bitrate).map_err(|e| failed(&e))?;
    builder.set_quality(Quality::Good).map_err(|e| failed(&e))?;
    // No info frame: it is written last, into a file, and without it filled
    // in it is only an empty frame at the start.
    builder.set_to_write_vbr_tag(false).map_err(|e| failed(&e))?;
    let mut encoder = builder.build().map_err(|e| failed(&e))?;

    let mut out = Vec::with_capacity(wav.len() / 6);
    // A second at a time. The room is reserved before every call: the
    // encoder writes into the vector's spare capacity and must never be
    // handed none.
    let second = usize::try_from(rate).unwrap_or(48_000).max(1) * usize::from(channels);
    for chunk in samples.chunks(second) {
        out.reserve(mp3lame_encoder::max_required_buffer_size(chunk.len()));
        if count == 1 {
            encoder.encode_to_vec(MonoPcm(chunk), &mut out).map_err(|e| failed(&e))?;
        } else {
            encoder.encode_to_vec(InterleavedPcm(chunk), &mut out).map_err(|e| failed(&e))?;
        }
    }
    out.reserve(mp3lame_encoder::max_required_buffer_size(0));
    encoder.flush_to_vec::<FlushGap>(&mut out).map_err(|e| failed(&e))?;
    if out.is_empty() {
        return Err(failed(&"nothing came out"));
    }
    Ok(out)
}

/// An MP3 clip's samples, as a WAV clip — for joining stored clips into a
/// podcast episode.
pub fn wav_from_mp3(mp3: &[u8]) -> Result<Vec<u8>> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::errors::Error as Trouble;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let failed = |what: &dyn std::fmt::Display| Error::msg(format!("A stored speech clip could not be read: {what}"));
    let stream = MediaSourceStream::new(Box::new(std::io::Cursor::new(mp3.to_vec())), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| failed(&e))?
        .format;
    let track = format.default_track().ok_or_else(|| failed(&"no audio in it"))?.clone();
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| failed(&e))?;

    let mut pcm: Vec<u8> = Vec::new();
    let mut shape: Option<(u32, u16)> = None;
    // Until the packets run out, which is how the end is told.
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track.id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // One bad frame is skipped, not the clip.
            Err(Trouble::DecodeError(_)) => continue,
            Err(error) => return Err(failed(&error)),
        };
        let spec = *decoded.spec();
        let channels = u16::try_from(spec.channels.count()).map_err(|e| failed(&e))?;
        match shape {
            None => shape = Some((spec.rate, channels)),
            Some(known) if known != (spec.rate, channels) => return Err(failed(&"its format changes midway")),
            Some(_) => {}
        }
        let mut samples = SampleBuffer::<i16>::new(crate::convert::bytes(decoded.capacity()), spec);
        samples.copy_interleaved_ref(decoded);
        for sample in samples.samples() {
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
    }
    let (rate, channels) = shape.ok_or_else(|| failed(&"no audio in it"))?;
    let lead = (MP3_DELAY * usize::from(channels) * 2).min(pcm.len());
    Ok(crate::ai::wav_from_pcm(&pcm[lead..], rate, channels))
}

/// Audio as it is stored: WAV becomes MP3, anything else stays what it is —
/// as does a WAV clip that cannot be encoded.
pub fn compact(mime: String, bytes: Vec<u8>) -> (String, Vec<u8>) {
    if is_wav(&mime) {
        if let Ok(mp3) = mp3_from_wav(&bytes) {
            return (MP3.to_string(), mp3);
        }
    }
    (mime, bytes)
}

/// Makes the speech stored as WAV — every clip and podcast episode from
/// before speech was kept as MP3 — into MP3, one file at a time, then gives
/// the freed space back. Returns how many files were made smaller.
///
/// Runs at startup, in the background, and finds nothing to do once it has
/// finished; stopped halfway, it carries on next time. The database is only
/// held to read a file and to put its replacement in place, never while
/// encoding.
pub fn compress_stored(app: &AppHandle) -> Result<usize> {
    let db = app.state::<Db>();
    if files::with_types(&mut db.lock(), WAV_TYPES)?.is_empty() {
        return Ok(0);
    }
    // Minutes of work on a phone with hours of speech: keep the app alive.
    let _busy = crate::work::begin(app, "Making stored audio smaller…");
    compress_in(&db, &files::media_dir(app)?)
}

/// [`compress_stored`], given where song files keep their copies.
pub fn compress_in(db: &Db, dir: &std::path::Path) -> Result<usize> {
    let ids = files::with_types(&mut db.lock(), WAV_TYPES)?;
    let mut done = 0;
    for id in ids {
        // Gone meanwhile (deleted, or replaced by another device): skip it.
        let Ok((_, wav)) = files::read(&mut db.lock(), id) else { continue };
        // Not audio that can be encoded: it stays as it is.
        let Ok(mp3) = mp3_from_wav(&wav) else { continue };
        drop(wav);
        if files::replace_audio(&mut db.lock(), Some(dir), id, MP3, &mp3)?.is_some() {
            done += 1;
        }
    }
    if done > 0 {
        // The pages the WAV files were on go back to the system.
        if let Err(error) = db.lock().batch_execute("VACUUM") {
            crate::diagnostics::note("background", format!("Giving back the space of the old audio failed: {error}"));
        }
    }
    Ok(done)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Ten seconds of what the speech model sends, for tests elsewhere.
    pub fn speech_wav() -> Vec<u8> {
        tone(24_000, 1, 10.0)
    }

    /// A tone that starts after a quarter of a second of silence.
    fn tone(rate: u32, channels: u16, seconds: f64) -> Vec<u8> {
        let frames = crate::convert::round_len(seconds * f64::from(rate));
        let mut pcm = Vec::with_capacity(frames * usize::from(channels) * 2);
        for frame in 0..frames {
            let t = crate::convert::real_len(frame) / f64::from(rate);
            let level = if t < 0.25 { 0.0 } else { (t * 440.0 * std::f64::consts::TAU).sin() * 8000.0 };
            let sample = i16::try_from(crate::convert::round(level)).unwrap();
            for _ in 0..channels {
                pcm.extend_from_slice(&sample.to_le_bytes());
            }
        }
        crate::ai::wav_from_pcm(&pcm, rate, channels)
    }

    /// The first frame louder than a quarter of the tone, in seconds.
    fn onset(wav: &[u8]) -> f64 {
        let (rate, channels, data) = pcm_of(wav).unwrap();
        let frame = 2 * usize::from(channels);
        let at = data.chunks_exact(frame).position(|f| i16::from_le_bytes([f[0], f[1]]).abs() > 2000).unwrap();
        crate::convert::real_len(at) / f64::from(rate)
    }

    #[test]
    fn speech_is_an_eighth_the_size_as_mp3_and_reads_back_in_time() {
        // What the speech model sends: 24 kHz, one channel.
        let wav = tone(24_000, 1, 10.0);
        let mp3 = mp3_from_wav(&wav).unwrap();
        assert!(mp3.len() * 7 < wav.len(), "{} bytes of WAV became {}", wav.len(), mp3.len());
        // An MPEG audio frame, no tag before it.
        assert_eq!((mp3[0], mp3[1] & 0xE0), (0xFF, 0xE0));

        let back = wav_from_mp3(&mp3).unwrap();
        let (rate, channels, data) = pcm_of(&back).unwrap();
        assert_eq!((rate, channels), (24_000, 1));
        let seconds = crate::convert::real_len(data.len() / 2) / 24_000.0;
        assert!((10.0..10.1).contains(&seconds), "all of it, and no more than a frame of padding: {seconds}");
        // The encoder's delay is taken off again, so joined clips keep time.
        assert!((onset(&back) - 0.25).abs() < 0.005, "the tone starts at {}", onset(&back));
    }

    #[test]
    fn two_channels_and_short_clips_encode_too() {
        let stereo = wav_from_mp3(&mp3_from_wav(&tone(24_000, 2, 1.0)).unwrap()).unwrap();
        assert_eq!(pcm_of(&stereo).unwrap().1, 2);
        // A single word: a fraction of a second.
        let word = tone(24_000, 1, 0.3);
        let back = wav_from_mp3(&mp3_from_wav(&word).unwrap()).unwrap();
        assert!(pcm_of(&back).unwrap().2.len() >= pcm_of(&word).unwrap().2.len());
    }

    #[test]
    fn only_wav_is_made_into_mp3() {
        let (mime, bytes) = compact(WAV.into(), tone(24_000, 1, 1.0));
        assert_eq!(mime, MP3);
        assert!(wav_from_mp3(&bytes).is_ok());
        // Already MP3, or not audio that can be read: kept as it came.
        assert_eq!(compact(MP3.into(), vec![1, 2, 3]), (MP3.to_string(), vec![1, 2, 3]));
        assert_eq!(compact(WAV.into(), b"not a wav file".to_vec()), (WAV.to_string(), b"not a wav file".to_vec()));
        assert!(mp3_from_wav(&crate::ai::wav_from_pcm(&[], 24_000, 1)).is_err());
        assert!(wav_from_mp3(b"not an mp3 file").is_err());
    }
}
