//! Dictation: the app says a sentence, the learner writes what they heard.
//!
//! Checking is as forgiving as typing a word (practice::normalize): case,
//! punctuation and spacing never count, and Polish letters typed without
//! their marks ("prosze" for "proszę") are accepted — but pointed out, word
//! by word, so the learner sees which letters they missed. The answer comes
//! back as a word-by-word comparison: right words, wrong ones with the right
//! form, words left out, and words that were not said.

use serde::Serialize;

use crate::practice::normalize;
use crate::session::split_punctuation;

/// One word of a dictation, compared.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiffWord {
    /// The word as the sentence has it, without the punctuation around it;
    /// empty for a word typed that was not said.
    pub expected: String,
    /// What was typed in its place; empty for a word left out.
    pub given: String,
    /// right | accents (right, but its Polish letters were left off or
    /// wrong: accepted, and shown) | wrong | missing | extra
    pub status: String,
}

/// A dictation checked: right when every word was heard (Polish letters
/// forgiven), with the comparison to show.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Checked {
    pub correct: bool,
    pub words: Vec<DiffWord>,
}

/// The words of a text, without their punctuation; dashes and other marks
/// standing alone are not words.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|token| split_punctuation(token).1.to_string())
        .filter(|word| !normalize(word).is_empty())
        .collect()
}

fn word(expected: &str, given: &str, status: &str) -> DiffWord {
    DiffWord { expected: expected.into(), given: given.into(), status: status.into() }
}

/// Compares what was typed with the sentence, word by word. The words in
/// common are lined up first (the longest common run, letters folded), so
/// one word left out doesn't make every word after it wrong; between them,
/// words typed in place of others are wrong, and the rest missing or extra.
pub fn check(expected: &str, given: &str) -> Checked {
    let (said, typed) = (words(expected), words(given));
    let said_keys: Vec<String> = said.iter().map(|w| normalize(w)).collect();
    let typed_keys: Vec<String> = typed.iter().map(|w| normalize(w)).collect();
    let (n, m) = (said.len(), typed.len());
    // common[i][j]: the longest run in common of said[i..] and typed[j..].
    let mut common = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            common[i][j] = if said_keys[i] == typed_keys[j] {
                common[i + 1][j + 1] + 1
            } else {
                common[i + 1][j].max(common[i][j + 1])
            };
        }
    }
    let mut matched = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if said_keys[i] == typed_keys[j] {
            matched.push((i, j));
            i += 1;
            j += 1;
        } else if common[i + 1][j] >= common[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }

    let mut out = Vec::new();
    let (mut from_said, mut from_typed) = (0, 0);
    for (at_said, at_typed) in matched.into_iter().chain(std::iter::once((n, m))) {
        let left: Vec<&String> = said[from_said..at_said].iter().collect();
        let put: Vec<&String> = typed[from_typed..at_typed].iter().collect();
        for k in 0..left.len().max(put.len()) {
            out.push(match (left.get(k), put.get(k)) {
                (Some(e), Some(g)) => word(e, g, "wrong"),
                (Some(e), None) => word(e, "", "missing"),
                (None, Some(g)) => word("", g, "extra"),
                (None, None) => continue,
            });
        }
        if at_said < n {
            let (e, g) = (&said[at_said], &typed[at_typed]);
            let status = if e.to_lowercase() == g.to_lowercase() { "right" } else { "accents" };
            out.push(word(e, g, status));
        }
        from_said = at_said + 1;
        from_typed = at_typed + 1;
    }
    let correct = n > 0 && out.iter().all(|w| w.status == "right" || w.status == "accents");
    Checked { correct, words: out }
}

/// Checks a dictation: for the quick game, which checks as it goes.
#[tauri::command]
pub fn dictation_check(expected: String, given: String) -> Checked {
    check(&expected, &given)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statuses(checked: &Checked) -> Vec<(&str, &str, &str)> {
        checked.words.iter().map(|w| (w.expected.as_str(), w.given.as_str(), w.status.as_str())).collect()
    }

    #[test]
    fn case_punctuation_and_polish_letters_are_forgiven_but_shown() {
        let checked = check("Poproszę kawę z mlekiem.", "poprosze kawę, z MLEKIEM");
        assert!(checked.correct);
        assert_eq!(
            statuses(&checked),
            [("Poproszę", "poprosze", "accents"), ("kawę", "kawę", "right"), ("z", "z", "right"), ("mlekiem", "MLEKIEM", "right")]
        );
        // A wrong Polish letter is a letter to point out, not a wrong word.
        assert_eq!(check("Mam kota.", "Mam kotą").words[1].status, "accents");
    }

    #[test]
    fn a_word_left_out_does_not_make_the_rest_wrong() {
        let checked = check("Gdzie jest dworzec?", "gdzie dworzec");
        assert!(!checked.correct);
        assert_eq!(statuses(&checked), [("Gdzie", "gdzie", "right"), ("jest", "", "missing"), ("dworzec", "dworzec", "right")]);
    }

    #[test]
    fn a_wrong_word_shows_the_right_one_and_extras_are_marked() {
        let checked = check("Kupuję chleb w piekarni.", "kupuje chleba w w piekarni");
        assert!(!checked.correct);
        assert_eq!(
            statuses(&checked),
            [
                ("Kupuję", "kupuje", "accents"),
                ("chleb", "chleba", "wrong"),
                ("w", "w", "right"),
                ("", "w", "extra"),
                ("piekarni", "piekarni", "right"),
            ]
        );
    }

    #[test]
    fn nothing_typed_is_every_word_missing() {
        let checked = check("Nie rozumiem — przepraszam.", "  ");
        assert!(!checked.correct);
        assert_eq!(checked.words.len(), 3, "the dash is not a word");
        assert!(checked.words.iter().all(|w| w.status == "missing"));
    }
}
