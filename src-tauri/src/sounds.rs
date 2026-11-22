//! Sound pairs: Polish sounds an English ear runs together — sz and ś, cz
//! and ć, ż and sz, ą and o, ę and e, y and i, ł and l — heard in pairs of
//! real words that differ only there ("proszę" / "prosię"). The app says
//! one; the learner picks which it was; then the other is played, so the
//! two are heard side by side.
//!
//! Pairs come from two places:
//! - a word's own near neighbour, which the tutor gives when it makes the
//!   word (`words.near_pl`, `near_en`: schema 10) — the lesson's own sounds;
//! - the built-in list below, the core contrasts, which ships with the app
//!   like the demo course. Its pairs have ids derived from their words
//!   (DATA.md), the same on every device; nothing is written for them.
//!
//! A words lesson ends with a short round of them when its words have the
//! sounds (see [`round`]); the quick game "Sound pairs" plays the built-in
//! list. Older words have no neighbour and are not filled in: their lessons
//! use the built-in pairs.

use rand::seq::{IndexedRandom, SliceRandom};
use serde::Serialize;

use crate::models::{Exercise, Pair};
use crate::session::WordRow;
#[allow(unused_imports)]
use crate::ids::*;

/// Sound pairs in a words lesson's round: a short one.
pub const ROUND: usize = 2;

/// One contrast: what to listen for, the letters that make it, and real word
/// pairs that differ only there.
struct Contrast {
    /// What the card says to listen for.
    label: &'static str,
    /// A word with any of these has the sound.
    letters: &'static [&'static str],
    /// (Polish, English) and (Polish, English): the pair.
    pairs: &'static [(&'static str, &'static str, &'static str, &'static str)],
}

const CONTRASTS: &[Contrast] = &[
    Contrast {
        label: "sz or ś",
        letters: &["ś", "si"],
        pairs: &[("proszę", "please", "prosię", "piglet"), ("kasza", "groats", "Kasia", "Kasia (a name)")],
    },
    Contrast {
        label: "cz or ć",
        letters: &["ć", "ci"],
        pairs: &[("czy", "whether", "ci", "to you"), ("płacz", "crying", "płać", "pay!")],
    },
    Contrast {
        label: "ż or sz",
        letters: &["ż", "rz"],
        pairs: &[("żyć", "to live", "szyć", "to sew"), ("żal", "regret", "szal", "shawl")],
    },
    Contrast { label: "rz or r", letters: &["rz"], pairs: &[("Rzym", "Rome", "rym", "rhyme")] },
    Contrast {
        label: "ą or o, a",
        letters: &["ą"],
        pairs: &[("kąt", "corner", "kot", "cat"), ("wąs", "moustache", "was", "you (plural)")],
    },
    Contrast {
        label: "ę or e",
        letters: &["ę"],
        pairs: &[("chcę", "I want", "chce", "he wants"), ("tę", "this (one)", "te", "these")],
    },
    Contrast {
        label: "y or i",
        letters: &["y"],
        pairs: &[("być", "to be", "bić", "to beat"), ("my", "we", "mi", "to me")],
    },
    Contrast {
        label: "ł or l",
        letters: &["ł"],
        pairs: &[("łata", "patch", "lata", "years"), ("łaska", "grace", "laska", "walking stick")],
    },
    Contrast {
        label: "s or ś",
        letters: &["ś", "si"],
        pairs: &[("kasa", "till", "Kasia", "Kasia (a name)"), ("nos", "nose", "noś", "carry!")],
    },
    Contrast {
        label: "sz or s",
        letters: &["sz"],
        pairs: &[("kosz", "basket", "kos", "blackbird"), ("szum", "rustle", "sum", "catfish")],
    },
    Contrast { label: "c or cz", letters: &["cz"], pairs: &[("cysta", "cyst", "czysta", "clean")] },
];

/// A pair of words to tell apart by ear.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SoundPair {
    /// A built-in pair's derived id; None for a word's own neighbour.
    pub id: Option<SoundPairId>,
    /// What to listen for: "sz or ś".
    pub label: String,
    pub a: String,
    pub a_en: String,
    pub b: String,
    pub b_en: String,
}

/// A built-in pair's id: from its words, so it is the same everywhere.
fn pair_id(a: &str, b: &str) -> SoundPairId {
    SoundPairId::derived("sound-pair", &format!("{a}/{b}"))
}

/// The built-in pairs, every one.
pub fn built_in() -> Vec<SoundPair> {
    CONTRASTS
        .iter()
        .flat_map(|c| {
            c.pairs.iter().map(|(a, a_en, b, b_en)| SoundPair {
                id: Some(pair_id(a, b)),
                label: c.label.into(),
                a: (*a).into(),
                a_en: (*a_en).into(),
                b: (*b).into(),
                b_en: (*b_en).into(),
            })
        })
        .collect()
}

/// What to listen for between two words: the first contrast whose letters
/// one word has and the other hasn't.
pub fn label_for(a: &str, b: &str) -> String {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    CONTRASTS
        .iter()
        .find(|c| c.letters.iter().any(|l| a.contains(l) != b.contains(l)))
        .map_or_else(|| "Listen closely".into(), |c| c.label.into())
}

/// The built-in contrasts a lesson's words have: one pair of each, in a
/// random order.
fn heard_in(words: &[&WordRow], rng: &mut impl rand::Rng) -> Vec<SoundPair> {
    let text: Vec<String> = words.iter().map(|w| w.pl.to_lowercase()).collect();
    let mut found: Vec<SoundPair> = Vec::new();
    for contrast in CONTRASTS {
        if !contrast.letters.iter().any(|l| text.iter().any(|t| t.contains(l))) {
            continue;
        }
        let pairs: Vec<SoundPair> = built_in().into_iter().filter(|p| p.label == contrast.label).collect();
        if let Some(pair) = pairs.choose(rng) {
            found.push(pair.clone());
        }
    }
    found.shuffle(rng);
    found
}

/// Every built-in word a round of sound pairs could say for these words:
/// both words of every pair of each contrast the words have. For keeping a
/// course's audio ready (offline.rs): which pair a round picks is chance.
pub fn spoken_for(words: &[String]) -> Vec<String> {
    let text: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    CONTRASTS
        .iter()
        .filter(|contrast| contrast.letters.iter().any(|l| text.iter().any(|t| t.contains(l))))
        .flat_map(|contrast| contrast.pairs.iter().flat_map(|(a, _, b, _)| [(*a).to_string(), (*b).to_string()]))
        .collect()
}

/// The pairs for a words lesson: its words' own neighbours first, then the
/// built-in pairs for the sounds its words have; at most [`ROUND`].
pub fn for_words(words: &[&WordRow], rng: &mut impl rand::Rng) -> Vec<SoundPair> {
    let mut own: Vec<SoundPair> = words
        .iter()
        .filter_map(|w| {
            let near = w.near.as_ref()?;
            Some(SoundPair { id: None, label: label_for(&w.pl, &near.pl), a: w.pl.clone(), a_en: w.meaning.clone(), b: near.pl.clone(), b_en: near.en.clone() })
        })
        .collect();
    own.shuffle(rng);
    let mut pairs = own;
    for pair in heard_in(words, rng) {
        if !pairs.iter().any(|p| p.label == pair.label || p.a == pair.a) {
            pairs.push(pair);
        }
    }
    pairs.truncate(ROUND);
    pairs
}

/// One card: the app says one of the pair; which was it? The two words are
/// the options, in the pair's order (so the card reads the same each time);
/// `pairs` carries both with their meanings, for after.
pub fn exercise(pair: &SoundPair, lesson_id: LessonId, rng: &mut impl rand::Rng) -> Exercise {
    let (said, said_en) = if rng.random_bool(0.5) { (&pair.a, &pair.a_en) } else { (&pair.b, &pair.b_en) };
    Exercise {
        key: format!("p{lesson_id}-{}-{}-sound", pair.a, pair.b),
        kind: "soundPair".into(),
        item_kind: "set".into(),
        item_id: lesson_id.to_string(),
        is_new: false,
        prompt: pair.label.clone(),
        hint: String::new(),
        answer: said.clone(),
        options: vec![pair.a.clone(), pair.b.clone()],
        tokens: vec![],
        pl: said.clone(),
        meaning: said_en.clone(),
        gaps: vec![],
        pairs: vec![
            Pair { pl: pair.a.clone(), en: pair.a_en.clone(), image: None },
            Pair { pl: pair.b.clone(), en: pair.b_en.clone(), image: None },
        ],
        image: None,
        option_images: vec![],
        context: vec![],
        endings: vec![],
        scene: None,
        story: None,
        reply: None,
        explanation: String::new(),
    }
}

/// A words lesson's round of sound pairs.
pub fn round(words: &[&WordRow], lesson_id: LessonId, rng: &mut impl rand::Rng) -> Vec<Exercise> {
    for_words(words, rng).iter().map(|pair| exercise(pair, lesson_id, rng)).collect()
}

/// What to do to make a sound: its spellings, and a word of advice for when
/// it was wanted but something else was heard.
struct Sound {
    spellings: &'static [&'static str],
    advice: &'static str,
}

/// Two sounds a learner's mouth runs together, as a speaking card hears
/// them: the same contrasts as the pairs above (and their labels, so a tip
/// can play the pair), plus a few that only matter when speaking.
struct Confusion {
    label: &'static str,
    a: Sound,
    b: Sound,
}

const SHARP_SZ: &str = "try a sharper sz: the tongue tip up and back, the lips pushed forward";
const SOFT_S: &str = "try a softer ś: the tongue up behind the teeth, the lips spread as in a smile";
const HARD_CZ: &str = "try a harder cz: the tongue tip up and back, like the ch in “church”";
const SOFT_C: &str = "try a softer ć: a light ch with the lips spread";
const BUZZ_Z: &str = "let the ż buzz: like the s in “pleasure”";
const PLAIN_S: &str = "try a plain s, as in “sun”";
const PLAIN_C: &str = "try a plain c, like the ts in “cats”";
const PLAIN_Z: &str = "try a plain z, as in “zoo”";

const CONFUSIONS: &[Confusion] = &[
    Confusion { label: "sz or ś", a: Sound { spellings: &["sz"], advice: SHARP_SZ }, b: Sound { spellings: &["ś", "si"], advice: SOFT_S } },
    Confusion { label: "cz or ć", a: Sound { spellings: &["cz"], advice: HARD_CZ }, b: Sound { spellings: &["ć", "ci"], advice: SOFT_C } },
    Confusion {
        label: "ż or sz",
        a: Sound { spellings: &["ż", "rz"], advice: BUZZ_Z },
        b: Sound { spellings: &["sz"], advice: "try a sharper sz, with no buzz in the throat" },
    },
    Confusion {
        label: "ż or ź",
        a: Sound { spellings: &["ż", "rz"], advice: "try a harder ż: the tongue back, the lips forward" },
        b: Sound { spellings: &["ź", "zi"], advice: "try a softer ź, with the lips spread" },
    },
    Confusion {
        label: "rz or r",
        a: Sound { spellings: &["rz"], advice: "rz sounds like ż: let it buzz, with no roll" },
        b: Sound { spellings: &["r"], advice: "try a rolled r: one quick tap of the tongue" },
    },
    Confusion { label: "s or ś", a: Sound { spellings: &["s"], advice: PLAIN_S }, b: Sound { spellings: &["ś", "si"], advice: SOFT_S } },
    Confusion { label: "sz or s", a: Sound { spellings: &["sz"], advice: SHARP_SZ }, b: Sound { spellings: &["s"], advice: PLAIN_S } },
    Confusion { label: "c or cz", a: Sound { spellings: &["c"], advice: PLAIN_C }, b: Sound { spellings: &["cz"], advice: HARD_CZ } },
    Confusion { label: "c or ć", a: Sound { spellings: &["c"], advice: PLAIN_C }, b: Sound { spellings: &["ć", "ci"], advice: SOFT_C } },
    Confusion { label: "z or ż", a: Sound { spellings: &["z"], advice: PLAIN_Z }, b: Sound { spellings: &["ż", "rz"], advice: BUZZ_Z } },
    Confusion {
        label: "ą or o, a",
        a: Sound { spellings: &["ą"], advice: "let the ą ring through the nose, like a nasal o" },
        b: Sound { spellings: &["o", "a", "om", "on"], advice: "keep the vowel plain, not through the nose" },
    },
    Confusion {
        label: "ę or e",
        a: Sound { spellings: &["ę"], advice: "let the ę ring through the nose" },
        b: Sound { spellings: &["e", "em", "en"], advice: "keep the e plain, not through the nose" },
    },
    Confusion {
        label: "y or i",
        a: Sound { spellings: &["y"], advice: "try a darker y: the tongue further back, like the i in “bit”" },
        b: Sound { spellings: &["i"], advice: "try a clear i, like the ee in “see”" },
    },
    Confusion {
        label: "ł or l",
        a: Sound { spellings: &["ł"], advice: "ł is like the English w" },
        b: Sound { spellings: &["l"], advice: "try a clear l, the tongue on the ridge behind the teeth" },
    },
];

/// A word said that was heard as another, and the sound that likely did it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SoundTip {
    /// The word as it should be: "szyć".
    pub word: String,
    /// What was heard: "żyć".
    pub heard: String,
    /// The contrast: "ż or sz".
    pub label: String,
    /// What to try: "try a sharper sz, with no buzz in the throat".
    pub advice: String,
    /// A built-in pair of the contrast, to hear side by side; None when the
    /// contrast has none.
    pub pair: Option<SoundPair>,
}

/// Whether `heard` is `word` with one sound of `want` said as `got`.
fn swapped(word: &str, heard: &str, want: &Sound, got: &Sound) -> bool {
    want.spellings.iter().any(|w| {
        word.match_indices(w).any(|(at, _)| {
            got.spellings.iter().any(|g| {
                let candidate = format!("{}{g}{}", &word[..at], &word[at + w.len()..]);
                candidate == heard
            })
        })
    })
}

/// Why `word` may have been heard as `heard`: the first known contrast that
/// turns the one into the other by a single sound, with the advice for the
/// sound that was wanted. None when the two differ otherwise (a different
/// word, an ending, more than one sound).
pub fn misheard(word: &str, heard: &str) -> Option<SoundTip> {
    let (w, h) = (word.to_lowercase(), heard.to_lowercase());
    if w == h || w.is_empty() || h.is_empty() {
        return None;
    }
    CONFUSIONS.iter().find_map(|c| {
        let want = if swapped(&w, &h, &c.a, &c.b) {
            &c.a
        } else if swapped(&w, &h, &c.b, &c.a) {
            &c.b
        } else {
            return None;
        };
        Some(SoundTip {
            word: word.into(),
            heard: heard.into(),
            label: c.label.into(),
            advice: want.advice.into(),
            pair: built_in().into_iter().find(|p| p.label == c.label),
        })
    })
}

/// Which known contrast tells `word` from `heard`, and which way: the
/// contrast's label, the spelling in the word and the one heard in its
/// place ("sz or ś", "ś", "sz" for "prosię" heard as "proszę"). For the
/// learning log (mistakes.rs): a sound pair missed, a word misheard.
pub fn swap(word: &str, heard: &str) -> Option<Swap> {
    let (w, h) = (word.to_lowercase(), heard.to_lowercase());
    if w == h || w.is_empty() || h.is_empty() {
        return None;
    }
    let find = |want: &Sound, got: &Sound| -> Option<(String, String)> {
        want.spellings.iter().find_map(|ws| {
            w.match_indices(ws).find_map(|(at, _)| {
                got.spellings.iter().find_map(|gs| {
                    let candidate = format!("{}{gs}{}", &w[..at], &w[at + ws.len()..]);
                    (candidate == h).then(|| ((*ws).to_string(), (*gs).to_string()))
                })
            })
        })
    };
    CONFUSIONS.iter().find_map(|c| {
        let (said, got) = find(&c.a, &c.b).or_else(|| find(&c.b, &c.a))?;
        Some(Swap { label: c.label.into(), said, heard: got })
    })
}

/// A contrast and which way it went (see [`swap`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Swap {
    pub label: String,
    pub said: String,
    pub heard: String,
}

/// Whether two spellings sound the same: rz and ż, ch and h, ó and u are
/// one sound each, so a word heard with the other spelling was said right.
pub fn sound_alike(a: &str, b: &str) -> bool {
    let fold = |s: &str| s.to_lowercase().replace("rz", "ż").replace("ch", "h").replace('ó', "u");
    fold(a) == fold(b)
}

/// A word's near neighbour as the tutor gave it, cleaned: one real word,
/// not the word itself (letters folded). None to leave it out.
pub fn clean_near(pl: &str, near: &str) -> Option<String> {
    let near = crate::text::clean(near);
    let single = !near.is_empty() && !near.contains(char::is_whitespace);
    (single && near.to_lowercase() != pl.to_lowercase()).then_some(near)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn row(pl: &str, near: Option<(&str, &str)>) -> WordRow {
        WordRow {
            id: WordId::new(),
            pl: pl.into(),
            meaning: format!("{pl} in English"),
            pos: "n-f".into(),
            reps: 0,
            image: None,
            near: near.map(|(pl, en)| crate::session::Near { pl: pl.into(), en: en.into() }),
            familiarity: 0,
        }
    }

    #[test]
    fn built_in_pairs_are_real_pairs_with_ids_the_same_everywhere() {
        let pairs = built_in();
        assert!(pairs.len() >= 15);
        for p in &pairs {
            assert_ne!(p.a.to_lowercase(), p.b.to_lowercase());
            assert!(!p.a_en.is_empty() && !p.b_en.is_empty());
            assert_eq!(p.id, Some(pair_id(&p.a, &p.b)));
        }
        let ids: std::collections::HashSet<_> = pairs.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), pairs.len(), "no two pairs share an id");
    }

    #[test]
    fn the_contrast_is_named() {
        assert_eq!(label_for("proszę", "prosię"), "sz or ś");
        assert_eq!(label_for("być", "bić"), "y or i");
        assert_eq!(label_for("łata", "lata"), "ł or l");
        assert_eq!(label_for("dom", "tom"), "Listen closely");
    }

    #[test]
    fn a_lesson_hears_its_own_pairs_first_then_its_sounds() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let words = [row("kawa", None), row("proszę", Some(("prosię", "piglet"))), row("mleko", None)];
        let refs: Vec<&WordRow> = words.iter().collect();
        let pairs = for_words(&refs, &mut rng);
        assert_eq!(pairs[0].a, "proszę");
        assert_eq!(pairs[0].b, "prosię");
        assert_eq!(pairs[0].id, None);
        assert!(pairs.len() <= ROUND);
        // Words without the sounds: no round at all.
        let plain = [row("dom", None), row("kot", None)];
        let refs: Vec<&WordRow> = plain.iter().collect();
        assert!(round(&refs, LessonId::new(), &mut rng).is_empty());
        // Words with ł: a built-in ł/l pair.
        let soft = [row("stół", None)];
        let refs: Vec<&WordRow> = soft.iter().collect();
        let pairs = for_words(&refs, &mut rng);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].label, "ł or l");
        let card = exercise(&pairs[0], LessonId::new(), &mut rng);
        assert!(card.options.contains(&card.answer));
        assert_eq!(card.options, [pairs[0].a.clone(), pairs[0].b.clone()]);
    }

    #[test]
    fn a_near_word_is_one_other_word() {
        assert_eq!(clean_near("proszę", " prosię "), Some("prosię".into()));
        assert_eq!(clean_near("proszę", "Proszę"), None);
        assert_eq!(clean_near("proszę", "two words"), None);
        assert_eq!(clean_near("proszę", ""), None);
    }

    #[test]
    fn a_word_heard_as_another_names_the_sound() {
        let tip = misheard("szyć", "żyć").expect("sz heard as ż");
        assert_eq!(tip.label, "ż or sz");
        assert!(tip.advice.contains("sharper sz"), "{}", tip.advice);
        assert!(tip.pair.is_some(), "the contrast has a pair to hear");
        let tip = misheard("żyć", "szyć").unwrap();
        assert!(tip.advice.contains("buzz"));
        assert_eq!(misheard("proszę", "prosię").unwrap().label, "sz or ś");
        assert!(misheard("prosię", "proszę").unwrap().advice.contains("softer ś"));
        assert_eq!(misheard("być", "bić").unwrap().label, "y or i");
        assert_eq!(misheard("łata", "lata").unwrap().label, "ł or l");
        assert_eq!(misheard("kąt", "kot").unwrap().label, "ą or o, a");
        assert_eq!(misheard("chcę", "chce").unwrap().label, "ę or e");
        assert_eq!(misheard("czysta", "cysta").unwrap().label, "c or cz");
        assert_eq!(misheard("Kasia", "kasa").unwrap().label, "s or ś");
        // Not a sound: another word, another ending, or the same word.
        assert_eq!(misheard("kawa", "herbata"), None);
        assert_eq!(misheard("kawę", "kawa"), None);
        assert_eq!(misheard("dom", "Dom"), None);
    }

    #[test]
    fn spellings_of_one_sound_sound_alike() {
        assert!(sound_alike("może", "morze"));
        assert!(sound_alike("Chleb", "hleb"));
        assert!(sound_alike("góra", "gura"));
        assert!(!sound_alike("szyć", "żyć"));
    }
}
