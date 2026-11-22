//! Sentence tiles with endings: building a sentence from its words when some
//! of them must change their ending. A word that stands in the sentence in
//! another form than its dictionary one ("kawę" for "kawa") is laid out as
//! its dictionary form, and the learner picks the ending ("kawę / kawy /
//! kawą") on the tile once it is placed.
//!
//! The forms come from the tutor, which writes them with the sentence when
//! it makes a lesson (`endings` on a sentence: courses::NewSentence). They
//! are kept with the sentence (`sentences.endings`, a JSON list). Sentences
//! made before this, or without any, are built by plain ordering — nothing
//! is filled in for them afterwards.

use serde::{Deserialize, Serialize};

use crate::session::split_punctuation;

/// One word of a sentence in another form than its dictionary one, as the
/// tutor gives it and as it is kept.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Ending {
    /// The word as it stands in the sentence ("kawę").
    pub form: String,
    /// Its dictionary form ("kawa"): what the tile shows until an ending is picked.
    pub base: String,
    /// Other forms of the same word that do not fit here ("kawy", "kawą").
    #[serde(default)]
    pub wrong: Vec<String>,
}

/// The most choices a tile offers: the right form, the dictionary form, and
/// two others.
pub const MAX_CHOICES: usize = 4;

/// Lower case, without punctuation, with Polish letters kept: how tile words
/// are told apart ("kawa" and "kawą" are different answers here, unlike in a
/// typed one, where missing diacritics are forgiven).
pub fn fold(text: &str) -> String {
    text.split_whitespace()
        .map(|word| word.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Keeps the endings that make sense for this sentence: a single word that
/// is in it, whose dictionary form differs, with wrong forms that differ
/// from both; each word of the sentence at most once. Anything else is
/// dropped quietly — the tile then simply shows the word as it is.
pub fn clean_endings(pl: &str, endings: &[Ending]) -> Vec<Ending> {
    let words: Vec<String> = pl.split_whitespace().map(|t| fold(split_punctuation(t).1)).collect();
    let mut kept: Vec<Ending> = Vec::new();
    for ending in endings {
        let form = split_punctuation(ending.form.trim()).1.to_string();
        let base = split_punctuation(ending.base.trim()).1.to_string();
        let (form_key, base_key) = (fold(&form), fold(&base));
        if form_key.is_empty() || base_key.is_empty() || form_key == base_key || form.contains(' ') || base.contains(' ') {
            continue;
        }
        if !words.contains(&form_key) || kept.iter().any(|k| fold(&k.form) == form_key) {
            continue;
        }
        let mut wrong: Vec<String> = Vec::new();
        for other in &ending.wrong {
            let other = split_punctuation(other.trim()).1.to_string();
            let key = fold(&other);
            if key.is_empty() || other.contains(' ') || key == form_key || key == base_key || wrong.iter().any(|w| fold(w) == key) {
                continue;
            }
            wrong.push(other);
        }
        wrong.truncate(MAX_CHOICES - 2);
        kept.push(Ending { form, base, wrong });
    }
    kept
}

/// The endings kept with a sentence, from its column (empty when it has none
/// or they cannot be read).
pub fn parse(json: &str) -> Vec<Ending> {
    serde_json::from_str(json).unwrap_or_default()
}

/// A tile of a word-order card whose ending is to be picked.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TileEnding {
    /// Which tile: its index in the card's `tokens`.
    pub token: usize,
    /// What the tile shows until an ending is picked: the dictionary form,
    /// with the word's punctuation and capital.
    pub shown: String,
    /// The forms to pick from, shuffled, the right one among them (with
    /// the same punctuation and capital).
    pub choices: Vec<String>,
}

/// `word` written like `like`: a capital where it has one.
fn same_case(like: &str, word: &str) -> String {
    let capital = like.chars().next().is_some_and(char::is_uppercase);
    let mut chars = word.chars();
    match chars.next() {
        Some(first) if capital => first.to_uppercase().chain(chars).collect(),
        _ => word.to_string(),
    }
}

/// The tiles among a card's (already shuffled) tokens whose ending is to be
/// picked: each ending to the first token that is its form, with at most
/// `choices` forms to pick from — the right one first, then the word's other
/// stored forms, then its dictionary form (two: the right one and the
/// likeliest other).
// Spec: DRILL-10
pub fn tile_endings(tokens: &[String], endings: &[Ending], choices: usize, rng: &mut impl rand::Rng) -> Vec<TileEnding> {
    use rand::seq::SliceRandom;
    let mut out: Vec<TileEnding> = Vec::new();
    for ending in endings {
        let key = fold(&ending.form);
        let found = tokens
            .iter()
            .enumerate()
            .find(|(i, t)| fold(split_punctuation(t).1) == key && !out.iter().any(|e| e.token == *i));
        let Some((index, token)) = found else { continue };
        let (before, word, after) = split_punctuation(token);
        let dress = |w: &str| format!("{before}{}{after}", same_case(word, w));
        // Four: the right form, the dictionary form and two others, as
        // before; fewer: the others first, the likelier slip.
        let others: Vec<String> = if choices >= MAX_CHOICES {
            std::iter::once(ending.base.clone()).chain(ending.wrong.iter().cloned()).collect()
        } else {
            ending.wrong.iter().cloned().chain(std::iter::once(ending.base.clone())).collect()
        };
        let mut picks: Vec<String> = std::iter::once(word.to_string())
            .chain(others)
            .take(choices.clamp(2, MAX_CHOICES))
            .map(|w| dress(&w))
            .collect();
        picks.shuffle(rng);
        out.push(TileEnding { token: index, shown: dress(&ending.base), choices: picks });
    }
    out.sort_by_key(|e| e.token);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn ending(form: &str, base: &str, wrong: &[&str]) -> Ending {
        Ending { form: form.into(), base: base.into(), wrong: wrong.iter().map(|w| w.to_string()).collect() }
    }

    #[test]
    fn tiles_keep_polish_letters_apart() {
        assert_eq!(fold("Poproszę kawę."), "poproszę kawę");
        assert_ne!(fold("kawa"), fold("kawą"));
        assert_eq!(fold("  Kawę, "), "kawę");
    }

    #[test]
    fn only_endings_that_fit_the_sentence_are_kept() {
        let kept = clean_endings(
            "Poproszę kawę z mlekiem.",
            &[
                ending("kawę", "kawa", &["kawy", "kawą", "kawę", "kawa", "kawy", "kawie"]),
                // Already in its dictionary form: nothing to pick.
                ending("z", "z", &[]),
                // Not in the sentence.
                ending("herbatę", "herbata", &["herbaty"]),
                // Punctuation the tutor left on is taken off.
                ending("mlekiem.", "mleko", &["mleka"]),
                // The same word twice: the first counts.
                ending("kawę", "kawka", &[]),
                // Two words are not one tile.
                ending("z mlekiem", "mleko", &[]),
            ],
        );
        assert_eq!(kept, vec![ending("kawę", "kawa", &["kawy", "kawą"]), ending("mlekiem", "mleko", &["mleka"])]);
    }

    #[test]
    fn a_tile_shows_the_dictionary_form_and_offers_the_right_one() {
        let tokens: Vec<String> = ["mlekiem.", "Kawę", "z", "piję"].iter().map(|t| t.to_string()).collect();
        let endings = vec![ending("kawę", "kawa", &["kawy", "kawą"]), ending("mlekiem", "mleko", &["mleka"])];
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let tiles = tile_endings(&tokens, &endings, MAX_CHOICES, &mut rng);
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].token, 0);
        assert_eq!(tiles[0].shown, "mleko.");
        assert!(tiles[0].choices.contains(&"mlekiem.".to_string()));
        assert_eq!(tiles[1].token, 1);
        assert_eq!(tiles[1].shown, "Kawa");
        let mut choices = tiles[1].choices.clone();
        choices.sort();
        assert_eq!(choices, vec!["Kawa", "Kawy", "Kawą", "Kawę"]);

        // Two to choose from: the right form and the word's likeliest other.
        let two = tile_endings(&tokens, &endings, 2, &mut rng);
        let mut choices = two[1].choices.clone();
        choices.sort();
        assert_eq!(choices, vec!["Kawy", "Kawę"]);
        assert_eq!(two[0].choices.len(), 2);
    }

    #[test]
    fn endings_from_an_older_sentence_read_as_none() {
        assert!(parse("").is_empty());
        assert!(parse("[]").is_empty());
        assert_eq!(parse(r#"[{"form":"kawę","base":"kawa"}]"#), vec![ending("kawę", "kawa", &[])]);
    }
}
