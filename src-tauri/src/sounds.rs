//! Sound pairs: Polish sounds an English ear runs together — sz and ś, cz
//! and ć, ż and sz, ą and o, ę and e, y and i, ł and l — heard in pairs of
//! real words that differ only there ("proszę" / "prosię"). The app says
//! one; the learner picks which it was; then the other is played, so the
//! two are heard side by side.
//!
//! Pairs come from two places:
//! - a word's own near neighbour, which the tutor gives when it makes the
//!   word (`words.near_pl`, `near_en`: schema 10);
//! - the built-in list below, the core contrasts, which ships with the app
//!   like the demo course. Its pairs have ids derived from their words
//!   (DATA.md), the same on every device; nothing is written for them.
//!
//! A words lesson ends with a short round of them, and a sentences lesson
//! has one before its gaps, but only of pairs with the lesson's own words
//! (see [`for_words`], [`for_sentences`]): a pair that merely shares a sound
//! with them is not played. The quick game "Sound pairs" plays the whole
//! built-in list.

use std::collections::HashSet;

use rand::seq::SliceRandom;
use serde::Serialize;

use crate::models::{Exercise, Pair};
use crate::session::{split_punctuation, SentenceRow, WordRow};
#[allow(unused_imports)]
use crate::ids::*;

/// Sound pairs in a lesson's round: a short one.
// Spec: SPK-9, SPK-11
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
        pairs: &[("kąt", "corner", "kot", "cat"), ("wąs", "moustache", "was", "you (plural)"), ("tą", "with this (one)", "ta", "this (one)")],
    },
    Contrast {
        label: "ę or e",
        letters: &["ę"],
        pairs: &[("chcę", "I want", "chce", "he wants"), ("tę", "this (one)", "te", "these")],
    },
    Contrast {
        label: "y or i",
        letters: &["y"],
        pairs: &[("być", "to be", "bić", "to beat"), ("my", "we", "mi", "to me"), ("było", "it was", "biło", "it beat")],
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
        pairs: &[("kosz", "basket", "kos", "blackbird"), ("szum", "rustle", "sum", "catfish"), ("nasz", "our", "nas", "us")],
    },
    Contrast { label: "c or cz", letters: &["cz"], pairs: &[("cysta", "cyst", "czysta", "clean")] },
    Contrast { label: "cz or trz", letters: &["trz"], pairs: &[("czy", "whether", "trzy", "three")] },
    Contrast { label: "z or ż", letters: &["ż"], pairs: &[("że", "that", "ze", "with")] },
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

/// A word as pairs are matched on: lower case, without punctuation, its
/// Polish letters kept ("kąt" is not "kot", nor "kat").
fn key(word: &str) -> String {
    crate::tiles::fold(word)
}

/// The built-in pairs with one of these words (as [`key`]s).
fn built_in_with(keys: &HashSet<String>) -> Vec<SoundPair> {
    built_in().into_iter().filter(|p| keys.contains(&key(&p.a)) || keys.contains(&key(&p.b))).collect()
}

/// A word and its own near neighbour, as a pair; None without one.
fn own_pair(w: &WordRow) -> Option<SoundPair> {
    let near = w.near.as_ref()?;
    Some(SoundPair { id: None, label: label_for(&w.pl, &near.pl), a: w.pl.clone(), a_en: w.meaning.clone(), b: near.pl.clone(), b_en: near.en.clone() })
}

/// A round from the pairs found: own neighbours first, then built-in pairs,
/// each in a random order, the same two words once; at most [`ROUND`].
fn pick(mut own: Vec<SoundPair>, mut built: Vec<SoundPair>, rng: &mut impl rand::Rng) -> Vec<SoundPair> {
    own.shuffle(rng);
    built.shuffle(rng);
    let mut pairs: Vec<SoundPair> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for pair in own.into_iter().chain(built) {
        let (a, b) = (key(&pair.a), key(&pair.b));
        if seen.insert(if a <= b { (a, b) } else { (b, a) }) {
            pairs.push(pair);
        }
    }
    pairs.truncate(ROUND);
    pairs
}

/// The whole words of some sentences, as [`key`]s.
fn words_in(sentences: &[&str]) -> HashSet<String> {
    sentences
        .iter()
        .flat_map(|s| s.split_whitespace())
        .map(|token| key(split_punctuation(token).1))
        .filter(|word| !word.is_empty())
        .collect()
}

/// Every built-in word a lesson's round could say, for words and sentences:
/// both words of each built-in pair with one of the words, or with a word
/// of one of the sentences. For keeping a course's audio ready (offline.rs).
// Spec: OFF-3
pub fn spoken_for(words: &[String], sentences: &[String]) -> Vec<String> {
    let texts: Vec<&str> = sentences.iter().map(String::as_str).collect();
    let mut keys = words_in(&texts);
    keys.extend(words.iter().map(|w| key(w)));
    built_in_with(&keys).into_iter().flat_map(|p| [p.a, p.b]).collect()
}

/// The pairs for a words lesson, each with one of its words: a word's own
/// near neighbour, or a built-in pair that has the word itself; at most
/// [`ROUND`], none when nothing matches.
// Spec: SPK-9
pub fn for_words(words: &[&WordRow], rng: &mut impl rand::Rng) -> Vec<SoundPair> {
    let keys: HashSet<String> = words.iter().map(|w| key(&w.pl)).collect();
    pick(words.iter().filter_map(|w| own_pair(w)).collect(), built_in_with(&keys), rng)
}

/// The pairs for a sentences lesson, each with a whole word of one of its
/// sentences: a built-in pair that has the word, or the near neighbour of
/// one of `words` (its course's) where that word stands in a sentence, as
/// it is or in a form the sentence's endings give for it; at most
/// [`ROUND`], none when nothing matches.
// Spec: SPK-11
pub fn for_sentences(sentences: &[&SentenceRow], words: &[WordRow], rng: &mut impl rand::Rng) -> Vec<SoundPair> {
    let texts: Vec<&str> = sentences.iter().map(|s| s.pl.as_str()).collect();
    let tokens = words_in(&texts);
    // Each sentence as its words joined by one space, padded, so a word of
    // several ("dzień dobry") is found whole.
    let lines: Vec<String> = texts.iter().map(|t| format!(" {} ", key(t))).collect();
    let bases: HashSet<String> = sentences
        .iter()
        .flat_map(|s| &s.endings)
        .filter(|e| tokens.contains(&key(&e.form)))
        .map(|e| key(&e.base))
        .collect();
    let stands = |w: &WordRow| {
        let k = key(&w.pl);
        !k.is_empty() && (bases.contains(&k) || lines.iter().any(|l| l.contains(&format!(" {k} "))))
    };
    let own = words.iter().filter(|w| stands(w)).filter_map(own_pair).collect();
    pick(own, built_in_with(&tokens), rng)
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
// Spec: SPK-9
pub fn round(words: &[&WordRow], lesson_id: LessonId, rng: &mut impl rand::Rng) -> Vec<Exercise> {
    for_words(words, rng).iter().map(|pair| exercise(pair, lesson_id, rng)).collect()
}

/// A sentences lesson's round of sound pairs.
// Spec: SPK-11
pub fn sentence_round(sentences: &[&SentenceRow], words: &[WordRow], lesson_id: LessonId, rng: &mut impl rand::Rng) -> Vec<Exercise> {
    for_sentences(sentences, words, rng).iter().map(|pair| exercise(pair, lesson_id, rng)).collect()
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

    fn sentence(pl: &str, endings: &[(&str, &str)]) -> SentenceRow {
        SentenceRow {
            id: SentenceId::new(),
            pl: pl.into(),
            meaning: String::new(),
            endings: endings.iter().map(|(form, base)| crate::tiles::Ending { form: (*form).into(), base: (*base).into(), wrong: vec![] }).collect(),
            familiarity: 0,
        }
    }

    // Spec: SPK-9
    #[test]
    fn a_words_lesson_plays_only_pairs_with_its_own_words() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        // "szkoła" shares sounds with built-in pairs but is in none, and has
        // no neighbour: no round.
        let plain = [row("szkoła", None), row("dom", None)];
        let refs: Vec<&WordRow> = plain.iter().collect();
        assert!(for_words(&refs, &mut rng).is_empty());
        assert!(round(&refs, LessonId::new(), &mut rng).is_empty());
        // A word that is itself in a built-in pair gets that pair.
        let own = [row("szkoła", None), row("Kot", None)];
        let refs: Vec<&WordRow> = own.iter().collect();
        let pairs = for_words(&refs, &mut rng);
        assert_eq!(pairs.len(), 1);
        assert_eq!((pairs[0].a.as_str(), pairs[0].b.as_str()), ("kąt", "kot"));
        assert!(pairs[0].id.is_some());
        let card = exercise(&pairs[0], LessonId::new(), &mut rng);
        assert!(card.options.contains(&card.answer));
        assert_eq!(card.options, [pairs[0].a.clone(), pairs[0].b.clone()]);
        // A near neighbour is used, first, and not again as the built-in pair
        // of the same two words.
        let near = [row("kawa", None), row("proszę", Some(("prosię", "piglet"))), row("mleko", None)];
        let refs: Vec<&WordRow> = near.iter().collect();
        let pairs = for_words(&refs, &mut rng);
        assert_eq!(pairs.len(), 1);
        assert_eq!((pairs[0].a.as_str(), pairs[0].b.as_str(), pairs[0].id), ("proszę", "prosię", None));
    }

    // Spec: SPK-9, SPK-11
    #[test]
    fn a_round_has_at_most_round_pairs() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let many = [row("czy", None), row("kot", None), row("my", None), row("łata", Some(("lata", "years")))];
        let refs: Vec<&WordRow> = many.iter().collect();
        assert_eq!(for_words(&refs, &mut rng).len(), ROUND);
        let s = [sentence("Czy my mamy kota? Nasz kot i trzy psy.", &[])];
        let refs: Vec<&SentenceRow> = s.iter().collect();
        assert_eq!(for_sentences(&refs, &[], &mut rng).len(), ROUND);
    }

    // Spec: SPK-11
    #[test]
    fn a_sentences_lesson_plays_pairs_with_words_of_its_sentences() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        // "Czy" stands in a sentence: a built-in pair with "czy".
        let s = [sentence("Czy to jest szkoła?", &[])];
        let refs: Vec<&SentenceRow> = s.iter().collect();
        let pairs = for_sentences(&refs, &[], &mut rng);
        assert!(!pairs.is_empty() && pairs.len() <= ROUND);
        assert!(pairs.iter().all(|p| p.a == "czy"), "{pairs:?}");
        // Whole words only: "czysta" is not "czy", and shared sounds bring nothing.
        let none = [sentence("Szkoła jest czysta, dobra.", &[])];
        let refs: Vec<&SentenceRow> = none.iter().collect();
        assert!(for_sentences(&refs, &[], &mut rng).is_empty());
        let plain = [sentence("Mam dom.", &[])];
        let refs: Vec<&SentenceRow> = plain.iter().collect();
        assert!(sentence_round(&refs, &[], LessonId::new(), &mut rng).is_empty());
        // A course word's near neighbour, where the word stands in a sentence,
        // as it is or in a form its endings give; not where it doesn't.
        let words = [row("proszę", Some(("prosię", "piglet"))), row("łata", Some(("lata", "years"))), row("kasza", Some(("Kasia", "Kasia")))];
        let said = |pl: &str, endings: &[(&str, &str)], rng: &mut rand::rngs::StdRng| {
            let s = [sentence(pl, endings)];
            let refs: Vec<&SentenceRow> = s.iter().collect();
            for_sentences(&refs, &words, rng).into_iter().map(|p| format!("{}/{}", p.a, p.b)).collect::<Vec<_>>()
        };
        assert_eq!(said("Proszę, mam łatę.", &[("łatę", "łata")], &mut rng).len(), 2);
        assert_eq!(said("Mam łatę.", &[("łatę", "łata")], &mut rng), ["łata/lata"]);
        assert!(said("Mam łatę.", &[], &mut rng).is_empty(), "no form given: no match");
        assert_eq!(said("Proszę bardzo!", &[], &mut rng), ["proszę/prosię"]);
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
