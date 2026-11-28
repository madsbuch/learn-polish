//! Grammar tags: the structures of Polish a word or sentence shows — the
//! cases and what triggers them ("w + locative"), tense and aspect, person,
//! agreement, numerals, się, modal verbs, question words, diminutives… A
//! curated list kept here, like the built-in sound pairs: each tag's id is
//! derived from its key (DATA.md), the same on every device, and nothing is
//! written for the list itself.
//!
//! Words and sentences are linked to tags (`word_grammar`,
//! `sentence_grammar`). The tutor tags what it makes (`grammar` on a word or
//! sentence in its tools); older items are tagged only when the learner asks
//! it to ("Tag the grammar in your courses" on the Structures page) — never
//! filled in behind their back (PRINCIPLES.md #4).

use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::db::Conn;
use crate::error::{Error, Result};
#[allow(unused_imports)]
use crate::ids::*;
use crate::schema::{sentence_grammar, word_grammar};

/// One structure: its key (what the tutor writes), its English name, the
/// group it is listed under, and a line on what it is.
#[derive(Debug)]
pub struct Tag {
    pub key: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub explain: &'static str,
}

const fn tag(key: &'static str, name: &'static str, group: &'static str, explain: &'static str) -> Tag {
    Tag { key, name, group, explain }
}

const CASES: &str = "Cases";
const PREPS: &str = "Prepositions and their cases";
const VERBS: &str = "Verbs";
const PERSON: &str = "Person";
const NOUNS: &str = "Nouns and agreement";
const NUMBERS: &str = "Numbers";
const QUESTIONS: &str = "Questions";
const FORMS: &str = "Word formation";
const SENTENCE: &str = "Sentences";

/// Every tag, in the order they are listed.
pub const TAGS: &[Tag] = &[
    tag("nominative", "Nominative", CASES, "The dictionary form: who or what does something (Kot śpi)."),
    tag("genitive", "Genitive", CASES, "Of, some, none: after many prepositions, quantities and negation (filiżanka kawy)."),
    tag("dative", "Dative", CASES, "To or for someone: who receives or benefits (Daję mamie kwiaty)."),
    tag("accusative", "Accusative", CASES, "The direct object of most verbs (Mam kawę, Widzę psa)."),
    tag("instrumental", "Instrumental", CASES, "With or by means of, and after być for what someone is (Jestem nauczycielem)."),
    tag("locative", "Locative", CASES, "Only after a preposition: in, on, about, at (w domu, o Polsce)."),
    tag("vocative", "Vocative", CASES, "Calling someone by name (Pani Anno! Tomku!)."),
    tag("w-locative", "w + locative", PREPS, "In or at a place: w Krakowie, w sklepie."),
    tag("na-locative", "na + locative", PREPS, "On, or at an event or open place: na stole, na poczcie."),
    tag("w-accusative", "w + accusative", PREPS, "On a day, into a game: w poniedziałek, grać w tenisa."),
    tag("na-accusative", "na + accusative", PREPS, "Going to an event, for a time or purpose: na koncert, na tydzień."),
    tag("do-genitive", "do + genitive", PREPS, "To or into a place, until: do sklepu, do Polski."),
    tag("z-genitive", "z + genitive", PREPS, "From, out of, made of: z Polski, z drewna."),
    tag("z-instrumental", "z + instrumental", PREPS, "With someone or something: kawa z mlekiem, z mamą."),
    tag("od-genitive", "od + genitive", PREPS, "From a person, since: od mamy, od rana."),
    tag("bez-genitive", "bez + genitive", PREPS, "Without: bez cukru."),
    tag("dla-genitive", "dla + genitive", PREPS, "For someone: dla ciebie."),
    tag("u-genitive", "u + genitive", PREPS, "At someone's place: u babci, u lekarza."),
    tag("obok-genitive", "obok, koło, blisko + genitive", PREPS, "Next to, near: obok kina, koło domu."),
    tag("o-locative", "o + locative", PREPS, "About: o filmie, o pracy."),
    tag("po-locative", "po + locative", PREPS, "After, around: po obiedzie, po mieście."),
    tag("przy-locative", "przy + locative", PREPS, "By, next to, at: przy oknie, przy stole."),
    tag("place-instrumental", "pod, nad, przed, za, między + instrumental", PREPS, "Where something is: pod stołem, przed domem."),
    tag("motion-accusative", "pod, nad, przed, za + accusative", PREPS, "Where something goes: pod stół, za dom."),
    tag("negation-genitive", "Negation takes the genitive", PREPS, "After nie, the object goes into the genitive: nie mam czasu."),
    tag("present", "Present tense", VERBS, "What happens now or usually: czytam, mieszkam."),
    tag("past", "Past tense", VERBS, "What happened; the ending shows gender: byłem, byłam, było."),
    tag("future-simple", "Simple future", VERBS, "A perfective verb in present form means the future: zrobię, kupię."),
    tag("future-compound", "Compound future", VERBS, "będę with an imperfective verb: będę czytać, będę czytał."),
    tag("imperfective", "Imperfective aspect", VERBS, "A process, a habit, or repeated action: robić, czytać."),
    tag("perfective", "Perfective aspect", VERBS, "One finished act or result: zrobić, przeczytać."),
    tag("aspect-pair", "Aspect pairs", VERBS, "Two verbs for one action, process vs result: kupować / kupić."),
    tag("imperative", "Imperative", VERBS, "Commands and requests: Chodź! Proszę usiąść."),
    tag("conditional", "Conditional", VERBS, "Would: by with the past form: chciałbym, mogłabyś."),
    tag("infinitive", "Infinitive", VERBS, "The dictionary form of a verb, after another verb: lubię pływać."),
    tag("conj-am", "-am, -asz verbs", VERBS, "Conjugation like czytać: czytam, czytasz, czytają."),
    tag("conj-e-esz", "-ę, -esz verbs", VERBS, "Conjugation like pisać, iść: piszę, piszesz, piszą."),
    tag("conj-e-isz", "-ę, -isz / -ysz verbs", VERBS, "Conjugation like robić, mówić: robię, robisz, robią."),
    tag("byc", "być (to be)", VERBS, "jestem, jesteś, jest, jesteśmy, jesteście, są."),
    tag("miec", "mieć (to have)", VERBS, "mam, masz, ma, mamy, macie, mają — and mieć + accusative."),
    tag("reflexive", "Reflexive się", VERBS, "Verbs with się: myć się, nazywać się, uczyć się."),
    tag("modal", "Modal verbs", VERBS, "musieć, móc, chcieć, woleć, umieć, trzeba + infinitive."),
    tag("motion-verbs", "Verbs of motion", VERBS, "iść / chodzić, jechać / jeździć: on foot or by vehicle, once or as a habit."),
    tag("person-1sg", "I (1st person singular)", PERSON, "Verb forms for ja: mam, robię, byłem."),
    tag("person-2sg", "you (2nd person singular)", PERSON, "Verb forms for ty: masz, robisz, byłeś."),
    tag("person-3sg", "he, she, it (3rd person singular)", PERSON, "Verb forms for on, ona, ono: ma, robi, był."),
    tag("person-1pl", "we (1st person plural)", PERSON, "Verb forms for my: mamy, robimy, byliśmy."),
    tag("person-2pl", "you all (2nd person plural)", PERSON, "Verb forms for wy: macie, robicie, byliście."),
    tag("person-3pl", "they (3rd person plural)", PERSON, "Verb forms for oni, one: mają, robią, byli, były."),
    tag("formal-address", "Pan, Pani: polite address", PERSON, "Polite 'you' takes the 3rd person: Czy pan ma czas?"),
    tag("gender", "Noun gender", NOUNS, "Masculine, feminine or neuter, mostly told by the ending: stół, kawa, okno."),
    tag("adjective-agreement", "Adjective agreement", NOUNS, "Adjectives match their noun's gender, number and case: dobra kawa, dobry sok."),
    tag("plural", "Plural nouns", NOUNS, "Plural endings: -y, -i, -e, -a (koty, panie, okna)."),
    tag("masculine-personal", "Masculine personal plural", NOUNS, "Groups with a man take their own forms: studenci, oni byli, ci panowie."),
    tag("animate-accusative", "Animate masculine accusative", NOUNS, "Masculine people and animals take -a in the accusative: mam psa, widzę brata."),
    tag("possessives", "Possessives", NOUNS, "mój, twój, jego, jej, nasz, wasz, ich, swój."),
    tag("pronoun-cases", "Pronoun forms", NOUNS, "Personal pronouns in other cases: mnie, mi, cię, go, jej, nam."),
    tag("demonstratives", "This and that", NOUNS, "ten, ta, to, ci, te — and tamten for that one there."),
    tag("comparison", "Comparative and superlative", FORMS, "lepszy, najlepszy; bardziej, najbardziej."),
    tag("adverbs", "Adverbs", FORMS, "From adjectives: dobry → dobrze, szybki → szybko."),
    tag("diminutives", "Diminutives", FORMS, "Small or affectionate forms: kotek, mamusia, piwko."),
    tag("numbers", "Numbers", NUMBERS, "Counting: jeden, dwa, trzy… and their gender forms (jedna, jedno)."),
    tag("numbers-2-4", "2, 3, 4 + nominative plural", NUMBERS, "After dwa, trzy, cztery: dwa koty, trzy kawy."),
    tag("numbers-5", "5 and up + genitive plural", NUMBERS, "After pięć and up: pięć kotów, dziesięć złotych."),
    tag("ordinals", "Ordinal numbers", NUMBERS, "pierwszy, drugi, trzeci: dates, floors, the time (o piątej)."),
    tag("time-dates", "Time and dates", NUMBERS, "Telling the time and the date: o której? o wpół do ósmej, piątego maja."),
    tag("question-words", "Question words", QUESTIONS, "co, kto, gdzie, kiedy, jak, dlaczego, ile, który."),
    tag("czy-questions", "Yes/no questions with czy", QUESTIONS, "czy turns a statement into a question: Czy masz czas?"),
    tag("negation", "Negation with nie", SENTENCE, "nie before the verb: Nie wiem, Nie ma problemu."),
    tag("word-order", "Word order", SENTENCE, "Endings carry the meaning, so order shifts the emphasis; short words like się sit early."),
    tag("impersonal", "Impersonal forms", SENTENCE, "można, trzeba, warto; się for 'one': Tu się nie pali."),
    tag("subordinate", "Linking clauses", SENTENCE, "że, bo, ale, kiedy, jeśli, żeby: joining what you say."),
    tag("relative", "Relative clauses with który", SENTENCE, "który, która, które, matching the noun: kobieta, która mieszka…"),
    tag("politeness", "Polite phrases", SENTENCE, "proszę, poproszę, dziękuję, przepraszam: asking nicely."),
];

/// A tag's id: derived from its key, so it is the same everywhere.
pub fn id_of(key: &str) -> GrammarTagId {
    GrammarTagId::derived("grammar-tag", key)
}

/// Every tag's key, for the tutor's tool schemas (an enum).
pub fn keys() -> Vec<&'static str> {
    TAGS.iter().map(|t| t.key).collect()
}

pub fn by_key(key: &str) -> Option<&'static Tag> {
    let key = key.trim().to_lowercase();
    TAGS.iter().find(|t| t.key == key)
}

pub fn by_id(id: GrammarTagId) -> Option<&'static Tag> {
    TAGS.iter().find(|t| id_of(t.key) == id)
}

/// A tag as the screens and the tutor see it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TagInfo {
    pub id: GrammarTagId,
    pub key: String,
    pub name: String,
    pub group: String,
    pub explain: String,
}

impl From<&Tag> for TagInfo {
    fn from(t: &Tag) -> Self {
        TagInfo { id: id_of(t.key), key: t.key.into(), name: t.name.into(), group: t.group.into(), explain: t.explain.into() }
    }
}

/// The tags given, checked: every key must be one of the list (the error
/// names the unknown ones, so the tutor can correct itself); duplicates
/// are dropped. At most eight: a tag says what the item is *for*.
pub fn validate(keys: &[String]) -> Result<Vec<&'static Tag>> {
    let mut kept: Vec<&'static Tag> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    for key in keys {
        match by_key(key) {
            Some(tag) if !kept.iter().any(|k| k.key == tag.key) => kept.push(tag),
            Some(_) => {}
            None => unknown.push(key.clone()),
        }
    }
    if !unknown.is_empty() {
        return Err(Error::msg(format!(
            "Unknown grammar tags: {}. Use only keys from the list (e.g. \"w-locative\", \"accusative\", \"past\").",
            unknown.join(", ")
        )));
    }
    kept.truncate(8);
    Ok(kept)
}

/// Gives a word its tags (replacing any it had).
pub fn set_word(conn: &mut Conn, word_id: WordId, tags: &[&Tag]) -> Result<()> {
    diesel::delete(word_grammar::table.filter(word_grammar::word_id.eq(word_id))).execute(conn)?;
    for t in tags {
        diesel::insert_or_ignore_into(word_grammar::table)
            .values((word_grammar::word_id.eq(word_id), word_grammar::tag_id.eq(id_of(t.key))))
            .execute(conn)?;
    }
    Ok(())
}

/// Gives a sentence its tags (replacing any it had).
pub fn set_sentence(conn: &mut Conn, sentence_id: SentenceId, tags: &[&Tag]) -> Result<()> {
    diesel::delete(sentence_grammar::table.filter(sentence_grammar::sentence_id.eq(sentence_id))).execute(conn)?;
    for t in tags {
        diesel::insert_or_ignore_into(sentence_grammar::table)
            .values((sentence_grammar::sentence_id.eq(sentence_id), sentence_grammar::tag_id.eq(id_of(t.key))))
            .execute(conn)?;
    }
    Ok(())
}

/// Every tagged item's tags: word or sentence id (as text) → tag ids.
pub fn all_links(conn: &mut Conn) -> Result<std::collections::HashMap<String, Vec<GrammarTagId>>> {
    let mut out: std::collections::HashMap<String, Vec<GrammarTagId>> = std::collections::HashMap::new();
    for (word, tag) in word_grammar::table.select((word_grammar::word_id, word_grammar::tag_id)).load::<(WordId, GrammarTagId)>(conn)? {
        out.entry(word.to_string()).or_default().push(tag);
    }
    for (sentence, tag) in sentence_grammar::table
        .select((sentence_grammar::sentence_id, sentence_grammar::tag_id))
        .load::<(SentenceId, GrammarTagId)>(conn)?
    {
        out.entry(sentence.to_string()).or_default().push(tag);
    }
    Ok(out)
}

/// One item the tutor tags (`tag_grammar`): a word or a sentence by id.
#[derive(Debug, Deserialize)]
pub struct TagItem {
    pub id: String,
    #[serde(default)]
    pub grammar: Vec<String>,
}

/// Tags existing words and sentences (the tutor, asked by the learner).
/// Every id and tag is checked first; one bad one fails the whole call (it
/// runs in the tool's transaction), with a message saying which.
pub fn tag_items(conn: &mut Conn, items: &[TagItem]) -> Result<(usize, usize)> {
    if items.len() > 200 {
        return Err(Error::msg("At most 200 items per call: tag the rest in another call."));
    }
    let (mut words, mut sentences) = (0, 0);
    for item in items {
        let tags = validate(&item.grammar)?;
        let id = item.id.trim();
        if id.starts_with("wd_") {
            let word = WordId::parse(id).map_err(|e| Error::msg(e.to_string()))?;
            let exists: i64 = crate::schema::words::table.find(word).count().get_result(conn)?;
            if exists == 0 {
                return Err(Error::msg(format!("No word {id}.")));
            }
            set_word(conn, word, &tags)?;
            words += 1;
        } else {
            let sentence = SentenceId::parse(id).map_err(|_| Error::msg(format!("{id}: give a word (wd_…) or a sentence (sn_…) id.")))?;
            let exists: i64 = crate::schema::sentences::table.find(sentence).count().get_result(conn)?;
            if exists == 0 {
                return Err(Error::msg(format!("No sentence {id}.")));
            }
            set_sentence(conn, sentence, &tags)?;
            sentences += 1;
        }
    }
    Ok((words, sentences))
}

/// How many words and sentences in open courses have no tag yet: what the
/// Structures page's "Tag the grammar" button offers to do.
pub fn untagged_count(conn: &mut Conn) -> Result<i64> {
    let words: i64 = diesel::sql_query(
        "SELECT count(DISTINCT w.id) AS n FROM words w
           JOIN course_lessons cl ON cl.lesson_id = w.lesson_id
           JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
          WHERE NOT EXISTS (SELECT 1 FROM word_grammar g WHERE g.word_id = w.id)",
    )
    .get_result::<Count>(conn)?
    .n;
    let sentences: i64 = diesel::sql_query(
        "SELECT count(DISTINCT s.id) AS n FROM sentences s
           JOIN course_lessons cl ON cl.lesson_id = s.lesson_id
           JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
          WHERE NOT EXISTS (SELECT 1 FROM sentence_grammar g WHERE g.sentence_id = s.id)",
    )
    .get_result::<Count>(conn)?
    .n;
    Ok(words + sentences)
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    n: i64,
}

/// Untagged words and sentences, a page of them, for the tutor to tag: open
/// courses only, one course when asked.
pub fn untagged(conn: &mut Conn, course_id: Option<CourseId>, limit: i64) -> Result<Vec<(String, String, String, String)>> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Text)]
        id: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        pl: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        en: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        course: String,
    }
    let course = course_id.map(|c| c.to_string()).unwrap_or_default();
    let rows: Vec<Row> = diesel::sql_query(
        "SELECT DISTINCT w.id AS id, w.pl AS pl, w.en AS en, c.title_en AS course FROM words w
           JOIN course_lessons cl ON cl.lesson_id = w.lesson_id
           JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
          WHERE NOT EXISTS (SELECT 1 FROM word_grammar g WHERE g.word_id = w.id) AND (?1 = '' OR c.id = ?1)
         UNION
         SELECT DISTINCT s.id, s.pl, s.en, c.title_en FROM sentences s
           JOIN course_lessons cl ON cl.lesson_id = s.lesson_id
           JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
          WHERE NOT EXISTS (SELECT 1 FROM sentence_grammar g WHERE g.sentence_id = s.id) AND (?1 = '' OR c.id = ?1)
         ORDER BY course, id LIMIT ?2",
    )
    .bind::<diesel::sql_types::Text, _>(course)
    .bind::<diesel::sql_types::BigInt, _>(limit.clamp(1, 200))
    .load(conn)?;
    Ok(rows.into_iter().map(|r| (r.id, r.pl, r.en, r.course)).collect())
}

/// The vocabulary as the tutor's prompt lists it: key — name, grouped.
pub fn for_tutor() -> String {
    let mut out = String::new();
    let mut group = "";
    for t in TAGS {
        if t.group != group {
            group = t.group;
            out.push_str(&format!("\n  {group}: "));
        } else {
            out.push_str(", ");
        }
        out.push_str(&format!("{} ({})", t.key, t.name));
    }
    out
}

// ---------------------------------------------------------------------------
// Sentences word by word (schema 15)
// ---------------------------------------------------------------------------
//
// The tutor annotates each new sentence token by token: its dictionary
// form (and the learner's word, when they have it), part of speech, case,
// number, gender, person, tense, aspect, mood, and why it has that form
// (`reason`). Every value comes from the lists below; an unknown one fails
// the call by name. Reasons that are grammar tags too ("z-instrumental",
// "negation-genitive") let the Structures page say which words the learner
// gets right or wrong in that structure.

pub const POS: &[(&str, &str)] = &[
    ("noun", "noun"),
    ("verb", "verb"),
    ("adj", "adjective"),
    ("adv", "adverb"),
    ("pron", "pronoun"),
    ("prep", "preposition"),
    ("conj", "conjunction"),
    ("part", "particle"),
    ("num", "numeral"),
    ("interj", "interjection"),
];
pub const CASES_LIST: &[(&str, &str)] = &[
    ("nominative", "nominative"),
    ("genitive", "genitive"),
    ("dative", "dative"),
    ("accusative", "accusative"),
    ("instrumental", "instrumental"),
    ("locative", "locative"),
    ("vocative", "vocative"),
];
pub const NUMBERS_LIST: &[(&str, &str)] = &[("sg", "singular"), ("pl", "plural")];
pub const GENDERS: &[(&str, &str)] = &[
    ("m-pers", "masculine personal"),
    ("m-anim", "masculine animate"),
    ("m-inan", "masculine inanimate"),
    ("f", "feminine"),
    ("n", "neuter"),
    ("non-m-pers", "non-masculine-personal (plural)"),
];
pub const PERSONS: &[(&str, &str)] = &[("1", "1st person"), ("2", "2nd person"), ("3", "3rd person")];
pub const TENSES: &[(&str, &str)] = &[("present", "present"), ("past", "past"), ("future", "future")];
pub const ASPECTS: &[(&str, &str)] = &[("imperfective", "imperfective"), ("perfective", "perfective")];
pub const MOODS: &[(&str, &str)] = &[
    ("indicative", "indicative"),
    ("imperative", "imperative"),
    ("conditional", "conditional"),
    ("infinitive", "infinitive"),
    ("participle", "participle"),
];

/// Why a token has its form: the key, how it reads ("z → instrumental"),
/// and the case it calls for, when it calls for one.
pub struct Reason {
    pub key: &'static str,
    pub label: &'static str,
    pub case: Option<&'static str>,
}

const fn r(key: &'static str, label: &'static str, case: Option<&'static str>) -> Reason {
    Reason { key, label, case }
}

const NOM: Option<&str> = Some("nominative");
const GEN: Option<&str> = Some("genitive");
const DAT: Option<&str> = Some("dative");
const ACC: Option<&str> = Some("accusative");
const INS: Option<&str> = Some("instrumental");
const LOC: Option<&str> = Some("locative");
const VOC: Option<&str> = Some("vocative");

pub const REASONS: &[Reason] = &[
    r("subject", "subject → nominative", NOM),
    r("naming", "to jest… → nominative", NOM),
    r("direct-object", "object → accusative", ACC),
    r("animate-accusative", "animate object → accusative (-a)", ACC),
    r("negation-genitive", "after negation → genitive", GEN),
    r("indirect-object", "to or for someone → dative", DAT),
    r("possession", "of someone → genitive", GEN),
    r("quantity-genitive", "an amount → genitive", GEN),
    r("numbers-2-4", "after 2, 3, 4 → nominative plural", NOM),
    r("numbers-5", "after 5 and up → genitive plural", GEN),
    r("predicate-instrumental", "być + noun → instrumental", INS),
    r("verb-government", "the verb takes this case", None),
    r("address", "calling someone → vocative", VOC),
    r("time-expression", "a time expression", None),
    r("agreement", "agrees with its noun", None),
    r("infinitive-complement", "after a verb → infinitive", None),
    r("dictionary-form", "dictionary form", None),
    r("w-locative", "w (in) → locative", LOC),
    r("na-locative", "na (on, at) → locative", LOC),
    r("w-accusative", "w (on a day, into) → accusative", ACC),
    r("na-accusative", "na (to, for) → accusative", ACC),
    r("do-genitive", "do → genitive", GEN),
    r("z-genitive", "z (from) → genitive", GEN),
    r("z-instrumental", "z (with) → instrumental", INS),
    r("od-genitive", "od → genitive", GEN),
    r("bez-genitive", "bez → genitive", GEN),
    r("dla-genitive", "dla → genitive", GEN),
    r("u-genitive", "u → genitive", GEN),
    r("obok-genitive", "obok, koło, blisko → genitive", GEN),
    r("o-locative", "o (about) → locative", LOC),
    r("po-locative", "po (after, around) → locative", LOC),
    r("przy-locative", "przy → locative", LOC),
    r("place-instrumental", "pod, nad, przed, za, między → instrumental", INS),
    r("motion-accusative", "pod, nad, przed, za (motion) → accusative", ACC),
    r("przez-accusative", "przez → accusative", ACC),
    r("dative-preposition", "dzięki, ku, wbrew → dative", DAT),
];

pub fn reason(key: &str) -> Option<&'static Reason> {
    REASONS.iter().find(|r| r.key == key)
}

/// How a token value reads, from any of the lists ("instrumental",
/// "z (with) → instrumental", "plural").
pub fn value_label(field: &str, key: &str) -> String {
    if field == "reason" {
        return reason(key).map_or(key, |r| r.label).to_string();
    }
    let list = match field {
        "pos" => POS,
        "case" => CASES_LIST,
        "number" => NUMBERS_LIST,
        "gender" => GENDERS,
        "person" => PERSONS,
        "tense" => TENSES,
        "aspect" => ASPECTS,
        "mood" => MOODS,
        _ => &[],
    };
    list.iter().find(|(k, _)| *k == key).map_or(key, |(_, l)| l).to_string()
}

/// A sentence's words as tokens are numbered: split at spaces, the
/// punctuation around each taken off, empty ones left out. The same
/// numbering for annotations, the learning log's links (wordlinks.rs) and
/// what the screens show. Each as written, and lowercased.
pub fn words_of(text: &str) -> Vec<(String, String)> {
    text.split_whitespace()
        .map(|t| crate::session::split_punctuation(t).1)
        .filter(|w| !w.is_empty())
        .map(|w| (w.to_string(), w.to_lowercase()))
        .collect()
}

/// One token as the tutor gives it.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct TokenIn {
    pub form: String,
    #[serde(default)]
    pub lemma: String,
    /// The learner's word it is, when the tutor knows it (wd_…).
    #[serde(default)]
    pub word_id: Option<String>,
    #[serde(default)]
    pub pos: String,
    #[serde(default)]
    pub case: String,
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub person: String,
    #[serde(default)]
    pub tense: String,
    #[serde(default)]
    pub aspect: String,
    #[serde(default)]
    pub mood: String,
    #[serde(default)]
    pub reason: String,
}

/// A token as kept and read back.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Queryable, Selectable)]
#[diesel(table_name = crate::schema::sentence_tokens)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub position: i64,
    pub form: String,
    pub lemma: String,
    pub word_id: Option<WordId>,
    pub pos: String,
    #[diesel(column_name = gram_case)]
    pub case: String,
    pub number: String,
    pub gender: String,
    pub person: String,
    pub tense: String,
    pub aspect: String,
    pub mood: String,
    pub reason: String,
}

/// Checks a sentence's tokens: one per word, in order, each form the
/// sentence's word there; every value from its list (named when it is
/// not); a reason that calls for a case agrees with the token's case; a
/// word id is a word's. Values are lowercased and trimmed.
pub fn validate_tokens(conn: &mut Conn, pl: &str, tokens: &[TokenIn]) -> Result<Vec<Token>> {
    let words = words_of(pl);
    if tokens.len() != words.len() {
        let listed: Vec<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
        return Err(Error::msg(format!(
            "\"{pl}\" has {} words ({}), but {} tokens were given: give one token per word, in order, without punctuation.",
            words.len(),
            listed.join(", "),
            tokens.len()
        )));
    }
    let lists: [(&str, &[(&str, &str)]); 8] = [
        ("pos", POS),
        ("case", CASES_LIST),
        ("number", NUMBERS_LIST),
        ("gender", GENDERS),
        ("person", PERSONS),
        ("tense", TENSES),
        ("aspect", ASPECTS),
        ("mood", MOODS),
    ];
    let mut out = Vec::with_capacity(tokens.len());
    for (i, (tok, (written, lower))) in tokens.iter().zip(&words).enumerate() {
        let n = i + 1;
        if crate::session::split_punctuation(tok.form.trim()).1.to_lowercase() != *lower {
            return Err(Error::msg(format!("Token {n} of \"{pl}\" is \"{}\", but the word there is \"{written}\".", tok.form)));
        }
        let clean = |v: &str| v.trim().to_lowercase();
        let values = [&tok.pos, &tok.case, &tok.number, &tok.gender, &tok.person, &tok.tense, &tok.aspect, &tok.mood].map(|v| clean(v));
        for ((field, list), value) in lists.iter().zip(&values) {
            if !value.is_empty() && !list.iter().any(|(k, _)| k == value) {
                let allowed: Vec<&str> = list.iter().map(|(k, _)| *k).collect();
                return Err(Error::msg(format!(
                    "Unknown {field} \"{value}\" on token {n} ({written}). Use one of: {}.",
                    allowed.join(", ")
                )));
            }
        }
        let why = clean(&tok.reason);
        if !why.is_empty() {
            let Some(found) = reason(&why) else {
                return Err(Error::msg(format!(
                    "Unknown reason \"{why}\" on token {n} ({written}). Use one of: {}.",
                    REASONS.iter().map(|r| r.key).collect::<Vec<_>>().join(", ")
                )));
            };
            if let Some(case) = found.case {
                if !values[1].is_empty() && values[1] != case {
                    return Err(Error::msg(format!(
                        "Token {n} ({written}): the reason \"{why}\" calls for the {case}, but its case is {}.",
                        values[1]
                    )));
                }
            }
        }
        let word_id = match tok.word_id.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
            Some(id) => {
                let word = WordId::parse(id).map_err(|e| Error::msg(format!("Token {n} ({written}): {e}")))?;
                if crate::schema::words::table.find(word).count().get_result::<i64>(conn)? == 0 {
                    return Err(Error::msg(format!("Token {n} ({written}): no word {id}.")));
                }
                Some(word)
            }
            None => None,
        };
        let [pos, case, number, gender, person, tense, aspect, mood] = values;
        out.push(Token {
            position: crate::convert::count(i),
            form: written.clone(),
            lemma: tok.lemma.trim().to_string(),
            word_id,
            pos,
            case: case.clone(),
            number,
            gender,
            person,
            tense,
            aspect,
            mood,
            reason: why,
        });
    }
    Ok(out)
}

/// Keeps a sentence's tokens (replacing any it had). A token without a word
/// id is linked to the learner's word with its lemma, when there is one.
pub fn set_tokens(conn: &mut Conn, sentence_id: SentenceId, tokens: &[Token]) -> Result<()> {
    use crate::schema::sentence_tokens as st;
    diesel::delete(st::table.filter(st::sentence_id.eq(sentence_id))).execute(conn)?;
    let lemmas: Vec<String> = tokens.iter().filter(|t| t.word_id.is_none() && !t.lemma.is_empty()).map(|t| t.lemma.to_lowercase()).collect();
    let mut by_lemma: std::collections::HashMap<String, WordId> = std::collections::HashMap::new();
    if !lemmas.is_empty() {
        // The sentence's own lesson's words first, then any.
        let lesson: Option<LessonId> =
            crate::schema::sentences::table.find(sentence_id).select(crate::schema::sentences::lesson_id).first(conn).optional()?;
        let rows: Vec<(WordId, String, LessonId)> = crate::schema::words::table
            .select((crate::schema::words::id, crate::schema::words::pl, crate::schema::words::lesson_id))
            .load(conn)?;
        for (id, pl, word_lesson) in rows {
            let key = pl.to_lowercase();
            if lemmas.contains(&key) && (Some(word_lesson) == lesson || !by_lemma.contains_key(&key)) {
                by_lemma.insert(key, id);
            }
        }
    }
    for t in tokens {
        let word = t.word_id.or_else(|| by_lemma.get(&t.lemma.to_lowercase()).copied());
        diesel::insert_into(st::table)
            .values((
                st::sentence_id.eq(sentence_id),
                st::position.eq(t.position),
                st::form.eq(&t.form),
                st::lemma.eq(&t.lemma),
                st::word_id.eq(word),
                st::pos.eq(&t.pos),
                st::gram_case.eq(&t.case),
                st::number.eq(&t.number),
                st::gender.eq(&t.gender),
                st::person.eq(&t.person),
                st::tense.eq(&t.tense),
                st::aspect.eq(&t.aspect),
                st::mood.eq(&t.mood),
                st::reason.eq(&t.reason),
            ))
            .execute(conn)?;
    }
    Ok(())
}

/// A sentence's tokens, in order (none when it was never annotated).
pub fn tokens_of(conn: &mut Conn, sentence_id: SentenceId) -> Result<Vec<Token>> {
    use crate::schema::sentence_tokens as st;
    Ok(st::table.filter(st::sentence_id.eq(sentence_id)).order(st::position).select(Token::as_select()).load(conn)?)
}

/// One sentence the tutor annotates (`annotate_sentences`).
#[derive(Debug, Deserialize)]
pub struct AnnotateItem {
    pub id: String,
    pub tokens: Vec<TokenIn>,
}

/// Annotates existing sentences (the tutor, asked by the learner): each
/// checked against its own words; one bad one fails the call, saying which.
pub fn annotate(conn: &mut Conn, items: &[AnnotateItem]) -> Result<usize> {
    if items.len() > 60 {
        return Err(Error::msg("At most 60 sentences per call: annotate the rest in another call."));
    }
    for item in items {
        let id = SentenceId::parse(item.id.trim()).map_err(|e| Error::msg(e.to_string()))?;
        let pl: String = crate::schema::sentences::table
            .find(id)
            .select(crate::schema::sentences::pl)
            .first(conn)
            .optional()?
            .ok_or_else(|| Error::msg(format!("No sentence {}.", item.id)))?;
        let tokens = validate_tokens(conn, &pl, &item.tokens)?;
        set_tokens(conn, id, &tokens)?;
    }
    Ok(items.len())
}

/// Sentences in open courses with no tokens yet.
pub fn unannotated_count(conn: &mut Conn) -> Result<i64> {
    Ok(diesel::sql_query(
        "SELECT count(DISTINCT s.id) AS n FROM sentences s
           JOIN course_lessons cl ON cl.lesson_id = s.lesson_id
           JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
          WHERE NOT EXISTS (SELECT 1 FROM sentence_tokens t WHERE t.sentence_id = s.id)",
    )
    .get_result::<Count>(conn)?
    .n)
}

/// Unannotated sentences, a page of them, for the tutor: (id, Polish,
/// English, course).
pub fn unannotated(conn: &mut Conn, course_id: Option<CourseId>, limit: i64) -> Result<Vec<(String, String, String, String)>> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Text)]
        id: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        pl: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        en: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        course: String,
    }
    let course = course_id.map(|c| c.to_string()).unwrap_or_default();
    let rows: Vec<Row> = diesel::sql_query(
        "SELECT DISTINCT s.id AS id, s.pl AS pl, s.en AS en, c.title_en AS course FROM sentences s
           JOIN course_lessons cl ON cl.lesson_id = s.lesson_id
           JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
          WHERE NOT EXISTS (SELECT 1 FROM sentence_tokens t WHERE t.sentence_id = s.id) AND (?1 = '' OR c.id = ?1)
         ORDER BY course, id LIMIT ?2",
    )
    .bind::<diesel::sql_types::Text, _>(course)
    .bind::<diesel::sql_types::BigInt, _>(limit.clamp(1, 200))
    .load(conn)?;
    Ok(rows.into_iter().map(|r| (r.id, r.pl, r.en, r.course)).collect())
}

/// The token vocabulary as the tutor's tool schema describes it.
pub fn token_schema() -> serde_json::Value {
    let keys = |list: &[(&'static str, &'static str)]| list.iter().map(|(k, _)| *k).collect::<Vec<_>>();
    serde_json::json!({
        "type": "array",
        "description": "The sentence word by word: one token per word, in order, punctuation left out.",
        "items": {
            "type": "object",
            "properties": {
                "form": { "type": "string", "description": "The word as written in the sentence" },
                "lemma": { "type": "string", "description": "Its dictionary form" },
                "word_id": { "type": "string", "pattern": "^wd_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The learner's word it is, when known" },
                "pos": { "type": "string", "enum": keys(POS) },
                "case": { "type": "string", "enum": keys(CASES_LIST) },
                "number": { "type": "string", "enum": keys(NUMBERS_LIST) },
                "gender": { "type": "string", "enum": keys(GENDERS) },
                "person": { "type": "string", "enum": keys(PERSONS) },
                "tense": { "type": "string", "enum": keys(TENSES) },
                "aspect": { "type": "string", "enum": keys(ASPECTS) },
                "mood": { "type": "string", "enum": keys(MOODS) },
                "reason": { "type": "string", "enum": REASONS.iter().map(|r| r.key).collect::<Vec<_>>(), "description": "Why it has this form" }
            },
            "required": ["form", "lemma", "pos"]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vocabulary_is_well_formed() {
        assert!(TAGS.len() >= 60, "{}", TAGS.len());
        let keys: std::collections::HashSet<_> = TAGS.iter().map(|t| t.key).collect();
        assert_eq!(keys.len(), TAGS.len(), "no two tags share a key");
        for t in TAGS {
            assert!(t.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{}", t.key);
            assert!(!t.name.is_empty() && t.explain.len() > 10, "{}", t.key);
            assert_eq!(by_id(id_of(t.key)).map(|x| x.key), Some(t.key));
        }
        assert_eq!(id_of("w-locative"), id_of("w-locative"));
        assert!(id_of("w-locative").to_string().starts_with("gr_"));
    }

    #[test]
    fn unknown_tags_are_named_and_duplicates_dropped() {
        let ok = validate(&["W-Locative".into(), "accusative".into(), "w-locative".into()]).unwrap();
        assert_eq!(ok.iter().map(|t| t.key).collect::<Vec<_>>(), ["w-locative", "accusative"]);
        let err = validate(&["accusative".into(), "ablative".into()]).unwrap_err().to_string();
        assert!(err.contains("ablative") && !err.contains("accusative,"), "{err}");
    }

    #[test]
    fn reasons_and_values_are_well_formed() {
        let keys: std::collections::HashSet<_> = REASONS.iter().map(|r| r.key).collect();
        assert_eq!(keys.len(), REASONS.len());
        for r in REASONS {
            if let Some(case) = r.case {
                assert!(CASES_LIST.iter().any(|(k, _)| *k == case), "{}", r.key);
            }
        }
        // The preposition reasons are grammar tags too (the Structures page
        // links them).
        assert!(by_key("z-instrumental").is_some() && reason("z-instrumental").is_some());
        assert_eq!(words_of("Kawa, z mlekiem!").iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>(), ["Kawa", "z", "mlekiem"]);
        assert_eq!(value_label("reason", "z-instrumental"), "z (with) → instrumental");
    }
}
