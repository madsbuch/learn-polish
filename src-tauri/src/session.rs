//! Builds the daily check-in: one conversation, the words that belong to it,
//! and its sentences — mixed with whatever else is due from earlier topics.

use rand::seq::SliceRandom;
use rusqlite::Connection;

use crate::db;
use crate::error::Result;
use crate::models::{DialogueLine, Exercise, SessionPlan, Topic};

const DEFAULT_NEW_WORDS: i64 = 12;
const DEFAULT_NEW_SENTENCES: i64 = 3;
const REVIEW_WORD_CAP: i64 = 20;
const REVIEW_SENTENCE_CAP: i64 = 6;
const DIALOGUE_GAPS: usize = 3;
/// Once an item has survived this many correct reps we stop showing multiple
/// choice and make the learner produce it from memory.
const TYPING_THRESHOLD: i64 = 3;

struct WordRow {
    id: i64,
    pl: String,
    meaning: String,
    pos: String,
    reps: i64,
}

struct SentenceRow {
    id: i64,
    pl: String,
    meaning: String,
}

pub fn topic_for_day(conn: &Connection) -> Result<Topic> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM topics", [], |r| r.get(0))?;
    let count = count.max(1);
    let lesson_index = db::get_setting_i64(conn, "lesson_index", 0);
    let ord = lesson_index.rem_euclid(count) + 1;
    load_topic(conn, db::require_topic_id(conn, ord)?)
}

pub fn load_topic(conn: &Connection, topic_id: i64) -> Result<Topic> {
    let lang = db::source_language(conn);
    let (title, summary) = if lang == "da" {
        ("title_da", "summary_da")
    } else {
        ("title_en", "summary_en")
    };
    let sql = format!(
        "SELECT id, ord, slug, title_pl, {title}, {summary} FROM topics WHERE id = ?1"
    );
    Ok(conn.query_row(&sql, [topic_id], |r| {
        Ok(Topic {
            id: r.get(0)?,
            ord: r.get(1)?,
            slug: r.get(2)?,
            title_pl: r.get(3)?,
            title: r.get(4)?,
            summary: r.get(5)?,
        })
    })?)
}

pub fn dialogue_lines(conn: &Connection, topic_id: i64) -> Result<Vec<DialogueLine>> {
    let meaning = meaning_column(conn);
    let sql = format!(
        "SELECT ord, speaker, gender, pl, {meaning} FROM conversation_lines
         WHERE topic_id = ?1 ORDER BY ord"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([topic_id], |r| {
            Ok(DialogueLine {
                ord: r.get(0)?,
                speaker: r.get(1)?,
                gender: r.get(2)?,
                pl: r.get(3)?,
                meaning: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn meaning_column(conn: &Connection) -> &'static str {
    if db::source_language(conn) == "da" {
        "da"
    } else {
        "en"
    }
}

pub fn build(conn: &Connection) -> Result<SessionPlan> {
    let today = db::today(conn);
    let topic = topic_for_day(conn)?;
    let meaning = meaning_column(conn);
    let new_word_budget = db::get_setting_i64(conn, "new_words_per_day", DEFAULT_NEW_WORDS).max(1);

    // Due reviews first — they come from any topic the learner has touched.
    let mut stmt = conn.prepare(&format!(
        "SELECT w.id, w.pl, w.{meaning}, w.pos, r.reps
         FROM reviews r JOIN words w ON w.id = r.item_id
         WHERE r.kind = 'word' AND r.due_day <= ?1
         ORDER BY r.due_day, r.interval LIMIT ?2"
    ))?;
    let review_words = stmt
        .query_map(rusqlite::params![today, REVIEW_WORD_CAP], |r| {
            Ok(WordRow {
                id: r.get(0)?,
                pl: r.get(1)?,
                meaning: r.get(2)?,
                pos: r.get(3)?,
                reps: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    let mut stmt = conn.prepare(&format!(
        "SELECT w.id, w.pl, w.{meaning}, w.pos
         FROM words w
         WHERE w.topic_id = ?1
           AND NOT EXISTS (SELECT 1 FROM reviews r WHERE r.kind = 'word' AND r.item_id = w.id)
         ORDER BY w.ord LIMIT ?2"
    ))?;
    let new_words = stmt
        .query_map(rusqlite::params![topic.id, new_word_budget], |r| {
            Ok(WordRow {
                id: r.get(0)?,
                pl: r.get(1)?,
                meaning: r.get(2)?,
                pos: r.get(3)?,
                reps: 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    let mut stmt = conn.prepare(&format!(
        "SELECT s.id, s.pl, s.{meaning}
         FROM reviews r JOIN sentences s ON s.id = r.item_id
         WHERE r.kind = 'sentence' AND r.due_day <= ?1
         ORDER BY r.due_day LIMIT ?2"
    ))?;
    let review_sentences = stmt
        .query_map(rusqlite::params![today, REVIEW_SENTENCE_CAP], |r| {
            Ok(SentenceRow { id: r.get(0)?, pl: r.get(1)?, meaning: r.get(2)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    let mut stmt = conn.prepare(&format!(
        "SELECT s.id, s.pl, s.{meaning}
         FROM sentences s
         WHERE s.topic_id = ?1
           AND NOT EXISTS (SELECT 1 FROM reviews r WHERE r.kind = 'sentence' AND r.item_id = s.id)
         ORDER BY s.ord LIMIT ?2"
    ))?;
    let new_sentences = stmt
        .query_map(rusqlite::params![topic.id, DEFAULT_NEW_SENTENCES], |r| {
            Ok(SentenceRow { id: r.get(0)?, pl: r.get(1)?, meaning: r.get(2)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    // Distractors are drawn from the same topic so the choices stay plausible.
    let pool = distractor_pool(conn, topic.id, meaning)?;
    let rng = &mut rand::rng();

    let mut word_blocks: Vec<Vec<Exercise>> = Vec::new();

    for w in &new_words {
        let mut block = vec![Exercise {
            key: format!("w{}-intro", w.id),
            kind: "intro".into(),
            item_kind: "word".into(),
            item_id: w.id,
            is_new: true,
            prompt: w.pl.clone(),
            hint: pretty_pos(&w.pos),
            answer: w.meaning.clone(),
            options: vec![],
            tokens: vec![],
            pl: w.pl.clone(),
            meaning: w.meaning.clone(),
        }];
        block.push(recognise(w, &pool, rng));
        word_blocks.push(block);
    }

    for w in &review_words {
        let exercise = if w.reps >= TYPING_THRESHOLD {
            Exercise {
                key: format!("w{}-type", w.id),
                kind: "type".into(),
                item_kind: "word".into(),
                item_id: w.id,
                is_new: false,
                prompt: w.meaning.clone(),
                hint: pretty_pos(&w.pos),
                answer: w.pl.clone(),
                options: vec![],
                tokens: vec![],
                pl: w.pl.clone(),
                meaning: w.meaning.clone(),
            }
        } else if w.reps >= 1 {
            produce(w, &pool, rng)
        } else {
            recognise(w, &pool, rng)
        };
        word_blocks.push(vec![exercise]);
    }

    word_blocks.shuffle(rng);

    let mut sentence_blocks: Vec<Vec<Exercise>> = Vec::new();
    for s in &new_sentences {
        sentence_blocks.push(vec![
            Exercise {
                key: format!("s{}-intro", s.id),
                kind: "sentenceIntro".into(),
                item_kind: "sentence".into(),
                item_id: s.id,
                is_new: true,
                prompt: s.pl.clone(),
                hint: String::new(),
                answer: s.meaning.clone(),
                options: vec![],
                tokens: vec![],
                pl: s.pl.clone(),
                meaning: s.meaning.clone(),
            },
            word_order(s),
        ]);
    }
    for s in &review_sentences {
        sentence_blocks.push(vec![word_order(s)]);
    }
    sentence_blocks.shuffle(rng);

    let mut exercises: Vec<Exercise> = Vec::new();
    let dialogue = dialogue_lines(conn, topic.id)?;

    // Read the conversation first: it frames every word and sentence that follows.
    if !dialogue.is_empty() {
        exercises.push(Exercise {
            key: format!("d{}-read", topic.id),
            kind: "dialogueRead".into(),
            item_kind: "dialogue".into(),
            item_id: topic.id,
            is_new: true,
            prompt: topic.title.clone(),
            hint: topic.summary.clone(),
            answer: String::new(),
            options: vec![],
            tokens: vec![],
            pl: topic.title_pl.clone(),
            meaning: topic.summary.clone(),
        });
    }

    // Interleave words and sentences so the drill does not feel like two lists.
    let mut words_iter = word_blocks.into_iter();
    let mut sentences_iter = sentence_blocks.into_iter();
    loop {
        let mut progressed = false;
        for _ in 0..4 {
            if let Some(block) = words_iter.next() {
                exercises.extend(block);
                progressed = true;
            }
        }
        if let Some(block) = sentences_iter.next() {
            exercises.extend(block);
            progressed = true;
        }
        if !progressed {
            break;
        }
    }

    exercises.extend(dialogue_gaps(&dialogue, topic.id, rng));

    let already_completed = conn
        .query_row(
            "SELECT completed FROM day_log WHERE day = ?1",
            [today],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        == 1;

    Ok(SessionPlan {
        day: today,
        topic,
        dialogue,
        new_words: new_words.len(),
        review_words: review_words.len(),
        new_sentences: new_sentences.len(),
        review_sentences: review_sentences.len(),
        exercises,
        already_completed,
    })
}

struct Distractors {
    meanings: Vec<String>,
    polish: Vec<String>,
}

fn distractor_pool(conn: &Connection, topic_id: i64, meaning: &str) -> Result<Distractors> {
    let mut stmt = conn.prepare(&format!(
        "SELECT pl, {meaning} FROM words WHERE topic_id = ?1"
    ))?;
    let rows = stmt
        .query_map([topic_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Distractors {
        polish: rows.iter().map(|(pl, _)| pl.clone()).collect(),
        meanings: rows.into_iter().map(|(_, m)| m).collect(),
    })
}

/// Builds a four-way choice: the answer plus three distinct decoys.
fn options(answer: &str, pool: &[String], rng: &mut impl rand::Rng) -> Vec<String> {
    let mut decoys: Vec<String> = pool
        .iter()
        .filter(|candidate| candidate.as_str() != answer)
        .cloned()
        .collect();
    decoys.shuffle(rng);
    decoys.truncate(3);

    let mut all = vec![answer.to_string()];
    all.extend(decoys);
    all.shuffle(rng);
    all
}

fn recognise(w: &WordRow, pool: &Distractors, rng: &mut impl rand::Rng) -> Exercise {
    Exercise {
        key: format!("w{}-recognise", w.id),
        kind: "recognise".into(),
        item_kind: "word".into(),
        item_id: w.id,
        is_new: false,
        prompt: w.pl.clone(),
        hint: pretty_pos(&w.pos),
        answer: w.meaning.clone(),
        options: options(&w.meaning, &pool.meanings, rng),
        tokens: vec![],
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
    }
}

fn produce(w: &WordRow, pool: &Distractors, rng: &mut impl rand::Rng) -> Exercise {
    Exercise {
        key: format!("w{}-produce", w.id),
        kind: "produce".into(),
        item_kind: "word".into(),
        item_id: w.id,
        is_new: false,
        prompt: w.meaning.clone(),
        hint: pretty_pos(&w.pos),
        answer: w.pl.clone(),
        options: options(&w.pl, &pool.polish, rng),
        tokens: vec![],
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
    }
}

fn word_order(s: &SentenceRow) -> Exercise {
    let mut tokens: Vec<String> = s.pl.split_whitespace().map(str::to_string).collect();
    tokens.shuffle(&mut rand::rng());
    Exercise {
        key: format!("s{}-order", s.id),
        kind: "wordOrder".into(),
        item_kind: "sentence".into(),
        item_id: s.id,
        is_new: false,
        prompt: s.meaning.clone(),
        hint: String::new(),
        answer: s.pl.clone(),
        options: vec![],
        tokens,
        pl: s.pl.clone(),
        meaning: s.meaning.clone(),
    }
}

/// Blanks out a few lines of the conversation and asks which one belongs.
fn dialogue_gaps(
    dialogue: &[DialogueLine],
    topic_id: i64,
    rng: &mut impl rand::Rng,
) -> Vec<Exercise> {
    if dialogue.len() < 5 {
        return vec![];
    }
    let mut indices: Vec<usize> = (1..dialogue.len()).collect();
    indices.shuffle(rng);
    indices.truncate(DIALOGUE_GAPS);
    indices.sort_unstable();

    let all_polish: Vec<String> = dialogue.iter().map(|l| l.pl.clone()).collect();

    indices
        .into_iter()
        .map(|i| {
            let line = &dialogue[i];
            let previous = &dialogue[i - 1];
            Exercise {
                key: format!("d{topic_id}-gap{i}"),
                kind: "dialogueGap".into(),
                item_kind: "dialogue".into(),
                item_id: topic_id,
                is_new: false,
                prompt: previous.pl.clone(),
                hint: format!("{} — {}", previous.speaker, line.speaker),
                answer: line.pl.clone(),
                options: options(&line.pl, &all_polish, rng),
                tokens: vec![],
                pl: line.pl.clone(),
                meaning: line.meaning.clone(),
            }
        })
        .collect()
}

fn pretty_pos(code: &str) -> String {
    match code {
        "n-m" => "noun, masculine",
        "n-f" => "noun, feminine",
        "n-n" => "noun, neuter",
        "n-pl" => "noun, plural",
        "v" => "verb",
        "adj" => "adjective",
        "adv" => "adverb",
        "num" => "number",
        "pron" => "pronoun",
        "prep" => "preposition",
        "conj" => "conjunction",
        "part" => "particle",
        "interj" => "interjection",
        "phr" => "phrase",
        other => other,
    }
    .to_string()
}
