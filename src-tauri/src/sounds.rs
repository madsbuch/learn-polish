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
    }
}

/// A words lesson's round of sound pairs.
pub fn round(words: &[&WordRow], lesson_id: LessonId, rng: &mut impl rand::Rng) -> Vec<Exercise> {
    for_words(words, rng).iter().map(|pair| exercise(pair, lesson_id, rng)).collect()
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
}
