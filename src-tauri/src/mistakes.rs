//! What kind of mistake an answer was: worked out in Rust when the answer
//! is graded, and written with it to the learning log (learnlog.rs), so the
//! screens and the tutor can say *what* the learner finds hard — the Polish
//! letters, the endings, the order, a sound, a preposition — not just how
//! often they miss.
//!
//! One class per answer, the most telling one that fits, in this order:
//! a sound contrast or a preposition (the card was about exactly that), the
//! Polish letters alone, the order alone, an ending, a typo, some words
//! right (a sentence), and otherwise a word not known.

use serde::{Deserialize, Serialize};

use crate::dictation::DiffWord;
use crate::practice::normalize;
use crate::session::split_punctuation;
use crate::tiles::{self, Ending};

/// Every class, with what the screens call it.
pub const CLASSES: &[(&str, &str)] = &[
    ("diacritics", "Polish letters"),
    ("ending", "Endings"),
    ("word_order", "Word order"),
    ("sound_contrast", "Sounds"),
    ("preposition", "Prepositions"),
    ("typo", "Typos"),
    ("lookalike", "Look-alike words"),
    ("partial", "Some words wrong"),
    ("unknown_word", "Didn't know it"),
];

pub fn label(class: &str) -> &'static str {
    CLASSES.iter().find(|(c, _)| *c == class).map_or("", |(_, l)| l)
}

/// Exercises where the answer is picked from options, not typed: a pick that
/// looks like the answer is a look-alike, never a typo.
pub(crate) const PICKED: &[&str] = &[
    "recognise", "produce", "wordPick", "picturePick", "dialogueGap", "storyQuestion", "sceneFind", "sceneName", "sceneWhere",
    "soundPair", "game:sounds", "game:match", "memory", "gapFill",
];

/// Prepositions of place, for a scene's statements.
const PREPOSITIONS: &[&str] = &["pod", "na", "w", "we", "nad", "przed", "za", "obok", "przy", "między", "koło", "u", "do", "z", "od", "naprzeciwko", "wokół"];

/// An answer, as graded: everything the classes are told apart by.
#[derive(Debug, Default)]
pub struct Graded<'a> {
    pub exercise: &'a str,
    pub expected: &'a str,
    pub given: &'a str,
    pub correct: bool,
    /// A dictation's or a spoken answer's word-by-word comparison.
    pub diff: &'a [DiffWord],
    /// The options shown (a sound pair: both words).
    pub options: &'a [String],
    /// What the card showed (a true-or-false scene card: the statement).
    pub prompt: &'a str,
    /// The Polish the card is about (a scene card: the true sentence).
    pub pl: &'a str,
    /// A sentence's endings (word-order tiles).
    pub endings: &'a [Ending],
}

/// One word of a sentence, compared: what was expected, what was given,
/// and how it went (r right, a Polish letters off, w wrong, e wrong ending,
/// m missing, x extra, o out of order).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WordResult {
    pub e: String,
    pub g: String,
    pub s: char,
}

/// A mistake: its class ('' when the answer was right and clean), what
/// exactly, a sound contrast when there was one, and word-by-word results
/// for a sentence.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mistake {
    pub class: &'static str,
    pub detail: String,
    pub contrast: String,
    pub words: Vec<WordResult>,
}

impl Mistake {
    fn of(class: &'static str, detail: impl Into<String>) -> Self {
        Mistake { class, detail: detail.into(), ..Mistake::default() }
    }
}

/// Lower case, letters kept as written (unlike [`normalize`], which folds
/// the Polish letters away).
fn plain(text: &str) -> String {
    tiles::fold(text)
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(|t| split_punctuation(t).1.to_lowercase()).filter(|w| !normalize(w).is_empty()).collect()
}

/// Edits (insert, delete, change a letter) from one to the other.
pub fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(row[j + 1] + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// The same word with another ending: a shared stem of at least three
/// letters (Polish letters folded), and short different tails ("kawę" and
/// "kawą", "dom" and "domu", "czytam" and "czytasz").
pub fn same_stem(a: &str, b: &str) -> bool {
    let (a, b) = (normalize(a), normalize(b));
    let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let shared = ac.iter().zip(&bc).take_while(|(x, y)| x == y).count();
    let raw_differ = a != b || ac.len() != bc.len();
    let (ta, tb): (Vec<char>, Vec<char>) = (ac[shared..].to_vec(), bc[shared..].to_vec());
    // Letters swapped round ("dziekjue" for "dziekuje") are a slip of the
    // fingers, not an ending.
    let mut sa = ta.clone();
    let mut sb = tb.clone();
    sa.sort_unstable();
    sb.sort_unstable();
    let swapped = !ta.is_empty() && sa == sb;
    // So is a letter dropped or added before an ending that is still there
    // ("jablo" for "jabłko").
    let (short, long) = if ta.len() <= tb.len() { (&ta, &tb) } else { (&tb, &ta) };
    let dropped = !short.is_empty() && long.ends_with(short);
    shared >= 3 && ta.len() <= 4 && tb.len() <= 4 && !swapped && !dropped && (raw_differ || shared < ac.len())
}

/// Only the Polish letters differ ("czesc" for "cześć", "zolw" for "żółw").
fn only_diacritics(expected: &str, given: &str) -> bool {
    let (e, g) = (plain(expected), plain(given));
    e != g && normalize(&e) == normalize(&g) && !normalize(&e).is_empty()
}

/// The first word of each, when they differ: the preposition of a place
/// phrase ("pod stołem" → "pod").
fn first_word(text: &str) -> String {
    words(text).into_iter().next().unwrap_or_default()
}

/// Classifies one graded answer.
pub fn classify(g: &Graded) -> Mistake {
    let given = g.given.trim();
    // A sound pair: which sound was heard as which, right or wrong.
    if g.exercise == "soundPair" || g.exercise == "game:sounds" {
        return sound_pair(g);
    }
    if g.exercise == "sceneWhere" {
        if let Some(m) = preposition(g) {
            return m;
        }
    }
    if !g.diff.is_empty() {
        return sentence(g);
    }
    if g.exercise == "gapFill" {
        return gaps(g);
    }
    if g.correct {
        // Right, but with Polish letters left off (typing forgives them).
        return if only_diacritics(g.expected, given) { Mistake::of("diacritics", format!("{}→{}", plain(g.expected), plain(given))) } else { Mistake::default() };
    }
    if normalize(given).is_empty() {
        return Mistake::of("unknown_word", "no answer");
    }
    if g.exercise == "wordOrder" {
        return tiles_order(g);
    }
    one_answer(g.exercise, g.expected, given)
}

/// A word or phrase given for another: the classes in order.
fn one_answer(exercise: &str, expected: &str, given: &str) -> Mistake {
    // Glosses carry alternatives ("piece, bit"): the nearest one counts.
    let expected = expected
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .min_by_key(|e| distance(&normalize(e), &normalize(given)))
        .unwrap_or(expected);
    let what = format!("{}→{}", plain(expected), plain(given));
    if only_diacritics(expected, given) {
        return Mistake::of("diacritics", what);
    }
    let (ew, gw) = (words(expected), words(given));
    // Picking an English meaning: endings and sounds are Polish matters.
    let polish = exercise != "recognise";
    if ew.len() > 1 && ew.len() == gw.len() {
        let (mut a, mut b) = (ew.iter().map(|w| normalize(w)).collect::<Vec<_>>(), gw.iter().map(|w| normalize(w)).collect::<Vec<_>>());
        a.sort();
        b.sort();
        if a == b {
            return Mistake::of("word_order", what);
        }
    }
    // One word each: a sound heard (or written) as its neighbour.
    if polish && ew.len() == 1 && gw.len() == 1 {
        if let Some(swap) = crate::sounds::swap(&ew[0], &gw[0]) {
            return Mistake { class: "sound_contrast", detail: format!("{}→{}", swap.said, swap.heard), contrast: swap.label, words: vec![] };
        }
    }
    if polish && ew.len() == gw.len() && !ew.is_empty() {
        let changed: Vec<(&String, &String)> = ew.iter().zip(&gw).filter(|(e, g)| normalize(e) != normalize(g)).collect();
        if !changed.is_empty() && changed.iter().all(|(e, g)| same_stem(e, g)) {
            let detail: Vec<String> = changed.iter().map(|(e, g)| format!("{e}→{g}")).collect();
            return Mistake::of("ending", detail.join(", "));
        }
    }
    let edits = distance(&normalize(expected), &normalize(given));
    if (1..=2).contains(&edits) && normalize(expected).chars().count() >= 3 {
        return Mistake::of(if PICKED.contains(&exercise) { "lookalike" } else { "typo" }, what);
    }
    if ew.len() > 1 && gw.iter().any(|g| ew.iter().any(|e| normalize(e) == normalize(g))) {
        return Mistake::of("partial", what);
    }
    Mistake::of("unknown_word", what)
}

fn sound_pair(g: &Graded) -> Mistake {
    let other = g.options.iter().find(|o| o.trim().to_lowercase() != g.expected.trim().to_lowercase()).cloned().unwrap_or_default();
    if g.correct {
        // Heard right: which sound it was, so the Sounds page counts hits.
        let said = crate::sounds::swap(g.expected, &other);
        let label = said.as_ref().map_or_else(|| crate::sounds::label_for(g.expected, &other), |s| s.label.clone());
        return Mistake { class: "", detail: said.map(|s| s.said).unwrap_or_default(), contrast: label, words: vec![] };
    }
    match crate::sounds::swap(g.expected, g.given) {
        Some(swap) => Mistake { class: "sound_contrast", detail: format!("{}→{}", swap.said, swap.heard), contrast: swap.label, words: vec![] },
        None => Mistake {
            class: "sound_contrast",
            detail: format!("{}→{}", plain(g.expected), plain(g.given)),
            contrast: crate::sounds::label_for(g.expected, g.given),
            words: vec![],
        },
    }
}

/// A scene's "Where is it?": which preposition was taken for which. The
/// choice card gives the phrase picked; a true-or-false card, the statement
/// shown against the true one.
fn preposition(g: &Graded) -> Option<Mistake> {
    use crate::lesson_types::scene::{FALSE, TRUE};
    let is_tf = g.expected == TRUE || g.expected == FALSE;
    if !is_tf {
        if g.correct {
            return Some(Mistake::default());
        }
        let (right, picked) = (first_word(g.expected), first_word(g.given));
        if right != picked && !right.is_empty() && !picked.is_empty() {
            return Some(Mistake::of("preposition", format!("{right}→{picked}")));
        }
        return None;
    }
    if g.correct {
        return Some(Mistake::default());
    }
    // The words where the statement shown and the true sentence part.
    let (shown, truth) = (words(g.prompt), words(g.pl));
    let start = shown.iter().zip(&truth).take_while(|(a, b)| a == b).count();
    let right = truth.get(start).cloned().unwrap_or_default();
    if g.expected == FALSE {
        // A wrong place believed: the true preposition taken for the shown one.
        let believed = shown.get(start).cloned().unwrap_or_default();
        return Some(Mistake::of("preposition", format!("{right}→{believed}")));
    }
    // The truth doubted: its preposition was not trusted.
    let right = if right.is_empty() { truth.iter().find(|w| PREPOSITIONS.contains(&w.as_str())).cloned().unwrap_or_default() } else { right };
    Some(Mistake::of("preposition", format!("{right}→?")))
}

/// A dictation or something said: word by word.
fn sentence(g: &Graded) -> Mistake {
    let results: Vec<WordResult> = g
        .diff
        .iter()
        .map(|w| {
            let s = match w.status.as_str() {
                "right" => 'r',
                "accents" => 'a',
                "missing" => 'm',
                "extra" => 'x',
                _ if same_stem(&w.expected, &w.given) => 'e',
                _ => 'w',
            };
            WordResult { e: w.expected.clone(), g: w.given.clone(), s }
        })
        .collect();
    let code: String = results.iter().map(|r| r.s).collect();
    let off: Vec<&WordResult> = results.iter().filter(|r| r.s != 'r').collect();
    let with = |class: &'static str, contrast: String| Mistake { class, detail: code.clone(), contrast, words: results.clone() };
    if off.is_empty() {
        return Mistake { words: results.clone(), ..Mistake::default() };
    }
    if off.iter().all(|r| r.s == 'a') {
        return with("diacritics", String::new());
    }
    if g.correct {
        // Accepted (a dictation forgives the letters; speech, a word the ear
        // can't tell apart): kept with the words, but no mistake.
        return Mistake { words: results.clone(), detail: code.clone(), ..Mistake::default() };
    }
    let wrong: Vec<&&WordResult> = off.iter().filter(|r| r.s != 'a').collect();
    let swaps: Vec<crate::sounds::Swap> = wrong.iter().filter_map(|r| crate::sounds::swap(&r.e, &r.g)).collect();
    if !swaps.is_empty() && swaps.len() == wrong.len() {
        return with("sound_contrast", swaps[0].label.clone());
    }
    if wrong.iter().all(|r| r.s == 'e') {
        return with("ending", String::new());
    }
    if results.iter().any(|r| r.s == 'r' || r.s == 'a') {
        return with("partial", swaps.first().map(|s| s.label.clone()).unwrap_or_default());
    }
    with("unknown_word", String::new())
}

/// Fill the gaps: each gap on its own, the class of the worst.
fn gaps(g: &Graded) -> Mistake {
    let expected: Vec<&str> = g.expected.split(" | ").collect();
    let given: Vec<&str> = g.given.split(" | ").collect();
    let mut results = Vec::new();
    let mut classes: Vec<Mistake> = Vec::new();
    for (i, e) in expected.iter().enumerate() {
        let gv = given.get(i).copied().unwrap_or("");
        let right = normalize(e) == normalize(gv);
        let m = if right { Mistake::default() } else { one_answer("gapFill", e, gv) };
        results.push(WordResult { e: (*e).into(), g: gv.into(), s: if right { 'r' } else if m.class == "ending" { 'e' } else { 'w' } });
        if !right {
            classes.push(m);
        }
    }
    let code: String = results.iter().map(|r| r.s).collect();
    match classes.len() {
        0 => Mistake { words: results, ..Mistake::default() },
        1 => Mistake { words: results, ..classes.remove(0) },
        n if n < expected.len() => Mistake { class: "partial", detail: code, contrast: String::new(), words: results },
        _ => {
            let class = if classes.iter().all(|c| c.class == "ending") { "ending" } else { "unknown_word" };
            Mistake { class, detail: code, contrast: String::new(), words: results }
        }
    }
}

/// Word-order tiles: the order, and the endings picked on tiles.
fn tiles_order(g: &Graded) -> Mistake {
    let (ew, gw) = (words(g.expected), words(g.given));
    let mut results: Vec<WordResult> = Vec::new();
    let mut endings: Vec<String> = Vec::new();
    let mut misplaced = false;
    for (i, e) in ew.iter().enumerate() {
        let given = gw.get(i).cloned().unwrap_or_default();
        let s = if plain(e) == plain(&given) {
            'r'
        } else if g.endings.iter().any(|end| plain(&end.form) == plain(e) && (plain(&end.base) == plain(&given) || end.wrong.iter().any(|w| plain(w) == plain(&given))))
            || (same_stem(e, &given) && !gw.iter().any(|x| plain(x) == plain(e)))
        {
            endings.push(format!("{e}→{given}"));
            'e'
        } else {
            misplaced = true;
            'o'
        };
        results.push(WordResult { e: e.clone(), g: given, s });
    }
    let code: String = results.iter().map(|r| r.s).collect();
    if !endings.is_empty() && !misplaced {
        return Mistake { class: "ending", detail: endings.join(", "), contrast: String::new(), words: results };
    }
    if !endings.is_empty() {
        // Both: the endings are the finer point, the order shows in the words.
        return Mistake { class: "ending", detail: format!("{} · order {code}", endings.join(", ")), contrast: String::new(), words: results };
    }
    Mistake { class: "word_order", detail: code, contrast: String::new(), words: results }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrong(exercise: &str, expected: &str, given: &str) -> Mistake {
        classify(&Graded { exercise, expected, given, ..Graded::default() })
    }

    #[test]
    fn polish_letters_left_off() {
        assert_eq!(wrong("type", "żółw", "zolw").class, "diacritics");
        // Typing forgives them: right, but noted.
        let right = classify(&Graded { exercise: "type", expected: "cześć", given: "czesc", correct: true, ..Graded::default() });
        assert_eq!((right.class, right.detail.as_str()), ("diacritics", "cześć→czesc"));
        let clean = classify(&Graded { exercise: "type", expected: "cześć", given: "Cześć!", correct: true, ..Graded::default() });
        assert_eq!(clean.class, "");
    }

    #[test]
    fn endings_word_order_typos_and_unknown_words() {
        let ending = wrong("type", "kawę", "kawy");
        assert_eq!((ending.class, ending.detail.as_str()), ("ending", "kawę→kawy"));
        assert_eq!(wrong("type", "w domu", "w dom").class, "ending");
        assert_eq!(wrong("type", "czytam", "czytasz").class, "ending");
        assert_eq!(wrong("type", "Mam kota", "Kota mam").class, "word_order");
        assert_eq!(wrong("type", "dziękuję", "dziekjue").class, "typo");
        assert_eq!(wrong("type", "jabłko", "jablo").class, "typo");
        assert_eq!(wrong("produce", "kot", "kod").class, "lookalike", "a pick is never a typo");
        assert_eq!(wrong("type", "pies", "samochód").class, "unknown_word");
        assert_eq!(wrong("type", "pies", "").detail, "no answer");
        // A gloss with alternatives: the nearest counts.
        assert_eq!(wrong("recognise", "piece, bit", "bite").class, "lookalike");
    }

    #[test]
    fn sounds_heard_as_their_neighbour() {
        let m = wrong("type", "proszę", "prosię");
        assert_eq!((m.class, m.contrast.as_str(), m.detail.as_str()), ("sound_contrast", "sz or ś", "sz→si"));
        let options = vec!["kąt".to_string(), "kot".to_string()];
        let missed = classify(&Graded { exercise: "soundPair", expected: "kąt", given: "kot", options: &options, ..Graded::default() });
        assert_eq!((missed.class, missed.contrast.as_str(), missed.detail.as_str()), ("sound_contrast", "ą or o, a", "ą→o"));
        let heard = classify(&Graded { exercise: "soundPair", expected: "kot", given: "kot", correct: true, options: &options, ..Graded::default() });
        assert_eq!((heard.class, heard.contrast.as_str(), heard.detail.as_str()), ("", "ą or o, a", "o"));
    }

    #[test]
    fn prepositions_in_a_scene() {
        let pick = wrong("sceneWhere", "pod stołem", "na stole");
        assert_eq!((pick.class, pick.detail.as_str()), ("preposition", "pod→na"));
        let believed = classify(&Graded {
            exercise: "sceneWhere",
            expected: "Fałsz",
            given: "Prawda",
            prompt: "Kot jest na stole.",
            pl: "Kot jest pod stołem.",
            ..Graded::default()
        });
        assert_eq!((believed.class, believed.detail.as_str()), ("preposition", "pod→na"));
        let doubted = classify(&Graded { exercise: "sceneWhere", expected: "Prawda", given: "Fałsz", prompt: "Kot jest pod stołem.", pl: "Kot jest pod stołem.", ..Graded::default() });
        assert_eq!(doubted.detail, "pod→?");
    }

    #[test]
    fn sentences_word_by_word() {
        let checked = crate::dictation::check("Poproszę kawę z mlekiem.", "Poprosze kawy z mlekiem");
        let m = classify(&Graded { exercise: "dictation", expected: "Poproszę kawę z mlekiem.", given: "Poprosze kawy z mlekiem", correct: checked.correct, diff: &checked.words, ..Graded::default() });
        assert_eq!((m.class, m.detail.as_str()), ("ending", "aerr"));
        let checked = crate::dictation::check("Mieszkam w Krakowie.", "Mieszkam w domu.");
        let m = classify(&Graded { exercise: "dictation", expected: "", given: "", correct: checked.correct, diff: &checked.words, ..Graded::default() });
        assert_eq!((m.class, m.words.len()), ("partial", 3));
        let spoken = crate::speaking::check("Chcę żyć", "Chcę szyć");
        let m = classify(&Graded { exercise: "sayIt", correct: spoken.correct, diff: &spoken.words, ..Graded::default() });
        assert_eq!((m.class, m.contrast.as_str()), ("sound_contrast", "ż or sz"));
        let letters = crate::dictation::check("Dzień dobry", "Dzien dobry");
        let m = classify(&Graded { exercise: "dictation", correct: letters.correct, diff: &letters.words, ..Graded::default() });
        assert_eq!(m.class, "diacritics");
    }

    #[test]
    fn tiles_tell_endings_from_order() {
        let endings = vec![Ending { form: "kawę".into(), base: "kawa".into(), wrong: vec!["kawy".into(), "kawą".into()] }];
        let m = classify(&Graded { exercise: "wordOrder", expected: "Poproszę kawę.", given: "Poproszę kawą.", endings: &endings, ..Graded::default() });
        assert_eq!((m.class, m.detail.as_str()), ("ending", "kawę→kawą"));
        assert_eq!(m.words.iter().map(|w| w.s).collect::<String>(), "re");
        let m = classify(&Graded { exercise: "wordOrder", expected: "Ja mam kota.", given: "Mam ja kota.", endings: &endings, ..Graded::default() });
        assert_eq!((m.class, m.detail.as_str()), ("word_order", "oor"));
    }

    #[test]
    fn gaps_one_by_one() {
        let m = wrong("gapFill", "kawę | mleka | herbatę", "kawę | mleko | herbatę");
        assert_eq!((m.class, m.words.iter().map(|w| w.s).collect::<String>().as_str()), ("ending", "rer"));
        let m = wrong("gapFill", "kawę | mleka | herbatę", "kawę | pies | dom");
        assert_eq!(m.class, "partial");
    }
}
