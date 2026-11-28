//! Tagged ids (DATA.md): every row's id is a TypeID — a prefix that says
//! what it is, an underscore, and a UUIDv7 in 26 characters of lowercase
//! base32: `ls_01jd6x7k2m8q9r4t5v6w7x8y9z`. Made where the row is written,
//! they sort by when they were made, never collide between devices, and a
//! wrong or made-up one fails instead of naming some other row.
//!
//! Each kind has its own type (`LessonId`, `CourseId`, …): only an id with the
//! right prefix parses into it, so passing one kind where another belongs is a
//! compile error in Rust, and a clear error for anything parsed (the tutor's
//! tool calls, the screens' commands).

use std::fmt;

use diesel::backend::Backend;
use diesel::deserialize::{self, FromSql};
use diesel::serialize::{self, IsNull, Output, ToSql};
use diesel::sql_types::Text;
use diesel::sqlite::Sqlite;
use sha2::{Digest, Sha256};

/// Crockford-style base32, lowercase, as TypeID writes it.
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Every kind of id: its prefix and what to call it in a message.
pub const KINDS: &[(&str, &str)] = &[
    ("crs", "course"),
    ("ls", "lesson"),
    ("wd", "word"),
    ("sn", "sentence"),
    ("dl", "dialogue line"),
    ("sg", "song"),
    ("tk", "take"),
    ("pl", "playlist"),
    ("pe", "playlist entry"),
    ("fl", "file"),
    ("ci", "check-in"),
    ("cm", "check-in message"),
    ("cv", "tutor conversation"),
    ("msg", "tutor message"),
    ("mem", "memory"),
    ("src", "source"),
    ("xp", "XP entry"),
    ("cost", "AI cost"),
    ("bench", "benchmark run"),
    ("dev", "device"),
    ("sp", "sound pair"),
    ("spot", "scene spot"),
    ("pnl", "story panel"),
    ("att", "answer"),
    ("ev", "learning event"),
    ("mst", "mastery entry"),
    ("gr", "grammar tag"),
    ("tp", "topic"),
    ("brg", "radio bridge"),
];

/// What an id names, by its prefix ("a lesson"), if it is a known kind.
pub fn kind_of(id: &str) -> Option<&'static str> {
    let prefix = id.split_once('_')?.0;
    KINDS.iter().find(|(p, _)| *p == prefix).map(|(_, name)| *name)
}

/// 128 bits as 26 base32 characters (the first carries the top 3 bits).
fn encode(value: u128) -> String {
    (0..26)
        .map(|i| {
            let shift = 5 * (25 - i);
            char::from(ALPHABET[usize::from(u8::try_from((value >> shift) & 31).unwrap_or(0))])
        })
        .collect()
}

/// The reverse of [`encode`]: exactly 26 characters of the alphabet, the
/// first no more than `7`.
fn decode(text: &str) -> Option<u128> {
    if text.len() != 26 || !text.starts_with(['0', '1', '2', '3', '4', '5', '6', '7']) {
        return None;
    }
    text.bytes().try_fold(0u128, |acc, byte| {
        let digit = ALPHABET.iter().position(|&c| c == byte)?;
        Some((acc << 5) | u128::from(u8::try_from(digit).ok()?))
    })
}

/// A UUIDv7: 48 bits of milliseconds, the version, 74 random bits.
fn uuid_v7(ms: u64, random: u128) -> u128 {
    let time = u128::from(ms & 0xffff_ffff_ffff) << 80;
    let rand_a = (random >> 62) & 0xfff;
    let rand_b = random & ((1u128 << 62) - 1);
    time | (0x7u128 << 76) | (rand_a << 64) | (0b10u128 << 62) | rand_b
}

/// The last id made here, so the next is always greater (RFC 9562's
/// monotonic method): rows made in the same millisecond keep the order they
/// were made in, which lists ordered by id rely on (the XP log, messages).
static LAST: std::sync::Mutex<u128> = std::sync::Mutex::new(0);

/// A new time-ordered id, greater than every id made before it here.
fn fresh_v7() -> u128 {
    let candidate = uuid_v7(now_ms(), rand::random());
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let next = if candidate > *last { candidate } else { successor(*last) };
    *last = next;
    next
}

/// The next id after `id`: one more in its random part, or the next
/// millisecond when that part is used up.
fn successor(id: u128) -> u128 {
    const RAND_B: u128 = (1 << 62) - 1;
    if id & RAND_B < RAND_B {
        id + 1
    } else {
        let ms = u64::try_from(id >> 80).unwrap_or(0);
        uuid_v7(ms + 1, 0)
    }
}

/// A name-based UUID (version 8) from SHA-256: the same input, the same id.
fn uuid_derived(namespace: &str, key: &str) -> u128 {
    let digest = Sha256::digest(format!("{namespace}\u{0}{key}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    let value = u128::from_be_bytes(bytes);
    let value = (value & !(0xfu128 << 76)) | (0x8u128 << 76);
    (value & !(0b11u128 << 62)) | (0b10u128 << 62)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// An id that did not parse as the kind wanted.
#[derive(Debug, Clone, PartialEq)]
pub struct IdError(pub String);

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IdError {}

/// Checks `text` is an id of the kind with `prefix`, saying what it got.
fn check(text: &str, prefix: &str) -> Result<(), IdError> {
    let wanted = KINDS.iter().find(|(p, _)| *p == prefix).map_or("thing", |(_, n)| n);
    match text.split_once('_') {
        Some((p, rest)) if p == prefix && decode(rest).is_some() => Ok(()),
        Some((p, rest)) if p != prefix && decode(rest).is_some() => Err(IdError(match kind_of(text) {
            Some(got) => format!("expected a {wanted} id ({prefix}_…), got a {got} id: {text}"),
            None => format!("expected a {wanted} id ({prefix}_…), got {text}"),
        })),
        _ => Err(IdError(format!("expected a {wanted} id like {prefix}_01jd6x7k2m8q9r4t5v6w7x8y9z, got {text:?}"))),
    }
}

/// Defines a typed id for one prefix.
macro_rules! typed_id {
    ($(#[$doc:meta])* $name:ident, $sql:ident, $prefix:literal) => {
        /// Its column type: text in SQLite, but only this kind of id goes in
        /// it — a query that mixes kinds does not compile.
        #[derive(Debug, Clone, Copy, Default, diesel::sql_types::SqlType, diesel::query_builder::QueryId)]
        #[diesel(sqlite_type(name = "Text"))]
        pub struct $sql;

        $(#[$doc])*
        ///
        /// Held as its 128 bits (so it is `Copy`, like the integer ids it
        /// replaced, and orders by time); written with its prefix.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, diesel::AsExpression, diesel::FromSqlRow)]
        #[diesel(sql_type = $sql)]
        pub struct $name(u128);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            /// A new id, made now.
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                Self(fresh_v7())
            }

            /// An id for something made at `ms` (milliseconds since 1970),
            /// with `random` filling the rest: upgrades keep the old order.
            pub fn at(ms: u64, random: u128) -> Self {
                Self(uuid_v7(ms, random))
            }

            /// The same id for the same namespace and key, on every device
            /// (built-in content).
            pub fn derived(namespace: &str, key: &str) -> Self {
                Self(uuid_derived(namespace, key))
            }

            /// Reads an id of this kind; anything else is an error that says
            /// what it was.
            pub fn parse(text: &str) -> Result<Self, IdError> {
                let text = text.trim();
                check(text, $prefix)?;
                let bits = text.split_once('_').and_then(|(_, rest)| decode(rest));
                bits.map(Self).ok_or_else(|| IdError(format!("not an id: {text}")))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", $prefix, encode(self.0))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = IdError;
            fn from_str(text: &str) -> Result<Self, IdError> {
                Self::parse(text)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                Self::parse(&text).map_err(serde::de::Error::custom)
            }
        }

        impl ToSql<$sql, Sqlite> for $name {
            fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
                out.set_value(self.to_string());
                Ok(IsNull::No)
            }
        }

        impl FromSql<$sql, Sqlite> for $name {
            fn from_sql(value: <Sqlite as Backend>::RawValue<'_>) -> deserialize::Result<Self> {
                let text = <String as FromSql<Text, Sqlite>>::from_sql(value)?;
                Self::parse(&text).map_err(|e| e.into())
            }
        }
    };
}

typed_id!(/// A course.
    CourseId, CourseIdSql, "crs");
typed_id!(/// A lesson.
    LessonId, LessonIdSql, "ls");
typed_id!(/// A word, owned by a lesson.
    WordId, WordIdSql, "wd");
typed_id!(/// A sentence, owned by a lesson.
    SentenceId, SentenceIdSql, "sn");
typed_id!(/// A line of a lesson's dialogue.
    LineId, LineIdSql, "dl");
typed_id!(/// A song or a podcast episode.
    SongId, SongIdSql, "sg");
typed_id!(/// One take of a song.
    TrackId, TrackIdSql, "tk");
typed_id!(/// A playlist.
    PlaylistId, PlaylistIdSql, "pl");
typed_id!(/// An entry in a playlist.
    EntryId, EntryIdSql, "pe");
typed_id!(/// A stored file.
    FileId, FileIdSql, "fl");
typed_id!(/// A check-in conversation.
    CheckinId, CheckinIdSql, "ci");
typed_id!(/// A message in a check-in.
    CheckinMessageId, CheckinMessageIdSql, "cm");
typed_id!(/// A tutor conversation.
    ConversationId, ConversationIdSql, "cv");
typed_id!(/// A message in a tutor conversation.
    MessageId, MessageIdSql, "msg");
typed_id!(/// A memory the agents keep.
    MemoryId, MemoryIdSql, "mem");
typed_id!(/// A Wikipedia source.
    SourceId, SourceIdSql, "src");
typed_id!(/// An entry in the XP log.
    XpEntryId, XpEntryIdSql, "xp");
typed_id!(/// What one AI request cost.
    CostId, CostIdSql, "cost");
typed_id!(/// A model benchmark run.
    BenchRunId, BenchRunIdSql, "bench");
typed_id!(/// One of the learner's devices, for sync (sync_state, day_log).
    DeviceId, DeviceIdSql, "dev");
typed_id!(/// A sound pair that ships with the app (sounds.rs): built-in content,
    /// so its id is derived from its words, the same on every device. Not a
    /// table: the list lives in the code.
    SoundPairId, SoundPairIdSql, "sp");
typed_id!(/// Where one thing is in a scene lesson's picture (scenes.rs).
    SpotId, SpotIdSql, "spot");
typed_id!(/// One panel of a story lesson: its picture and its sentences (stories.rs).
    PanelId, PanelIdSql, "pnl");
typed_id!(/// One graded answer in the learning log (learnlog.rs): append-only,
    /// written on the device that graded it.
    AttemptId, AttemptIdSql, "att");
typed_id!(/// One thing the learner did that is not an answer (a lesson opened,
    /// a word's meaning tapped): the learning log's events (learnlog.rs).
    LearningEventId, LearningEventIdSql, "ev");
typed_id!(/// One change to a lesson's mastery points, and why (masterylog.rs):
    /// append-only, written on the device that made the change.
    MasteryEntryId, MasteryEntryIdSql, "mst");
typed_id!(/// A grammar structure from the built-in vocabulary (grammar.rs):
    /// derived from its key, the same on every device. Not a table.
    GrammarTagId, GrammarTagIdSql, "gr");
typed_id!(/// A topic from the built-in ontology (topics.rs): derived from its key,
    /// the same on every device. Not a table.
    TopicId, TopicIdSql, "tp");
typed_id!(/// A spoken bridge between two items of the radio's programme (radio.rs).
    BridgeId, BridgeIdSql, "brg");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_keep_the_order_they_were_made_in() {
        let ids: Vec<LessonId> = (0..10_000).map(|_| LessonId::new()).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]));
        let text: Vec<String> = ids.iter().map(ToString::to_string).collect();
        assert!(text.windows(2).all(|w| w[0] < w[1]), "and so does their text");
        assert!(text.iter().all(|t| LessonId::parse(t).is_ok()));
        let full = uuid_v7(5, u128::MAX);
        assert_eq!(successor(full) >> 80, 6, "a used-up millisecond moves to the next");
    }

    #[test]
    fn ids_round_trip_sort_by_time_and_know_their_kind() {
        let id = LessonId::new();
        assert!(id.to_string().starts_with("ls_") && id.to_string().len() == 3 + 26, "{id}");
        assert_eq!(LessonId::parse(&id.to_string()), Ok(id));
        let earlier = LessonId::at(1_700_000_000_000, u128::MAX);
        let later = LessonId::at(1_700_000_000_001, 0);
        assert!(earlier < later, "ordered by time: {earlier} {later}");
        assert!(earlier.to_string() < later.to_string(), "as text too");
        // Decoding gives back the bits.
        let value = uuid_v7(1_700_000_000_000, 12345);
        assert_eq!(decode(&encode(value)), Some(value));
    }

    #[test]
    fn a_wrong_kind_or_a_made_up_id_says_what_it_was() {
        let course = CourseId::new();
        let err = LessonId::parse(&course.to_string()).unwrap_err().to_string();
        assert!(err.contains("expected a lesson id (ls_…), got a course id"), "{err}");
        let err = LessonId::parse("42").unwrap_err().to_string();
        assert!(err.contains("expected a lesson id like ls_"), "{err}");
        assert!(LessonId::parse("ls_zzzzzzzzzzzzzzzzzzzzzzzzzz").is_err(), "first character above 7");
        assert_eq!(kind_of(&course.to_string()), Some("course"));
        let parsed: Result<LessonId, _> = serde_json::from_str("\"crs_01jd6x7k2m8q9r4t5v6w7x8y9z\"");
        assert!(parsed.is_err());
    }

    #[test]
    fn derived_ids_are_the_same_everywhere() {
        assert_eq!(CourseId::derived("demo", "first-steps"), CourseId::derived("demo", "first-steps"));
        assert_ne!(CourseId::derived("demo", "first-steps"), CourseId::derived("demo", "other"));
        assert!(CourseId::parse(&CourseId::derived("demo", "x").to_string()).is_ok());
    }
}
