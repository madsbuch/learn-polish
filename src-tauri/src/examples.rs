//! Example sentences for a word: shown when an answer is wrong, so the word
//! is met again in context, not just as a pair. Made once by the AI and kept;
//! without an API key, sentences from the learner's own courses that use the
//! word stand in.

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::ai::Wire;
use crate::db::{Conn, Db};
use crate::error::Result;
use crate::schema::{sentences, word_examples, words};
#[allow(unused_imports)]
use crate::ids::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Example {
    pub pl: String,
    pub en: String,
}

/// A word's stored examples, if it has any.
fn stored(conn: &mut Conn, word_id: WordId) -> Result<Option<Vec<Example>>> {
    let json: Option<String> = word_examples::table.find(word_id).select(word_examples::examples).first(conn).optional()?;
    Ok(json.and_then(|json| serde_json::from_str::<Vec<Example>>(&json).ok()))
}

/// The stored examples of every word of these lessons.
pub fn of_lessons(conn: &mut Conn, lesson_ids: &[LessonId]) -> Result<Vec<Example>> {
    let rows: Vec<String> = word_examples::table
        .inner_join(words::table)
        .filter(words::lesson_id.eq_any(lesson_ids))
        .order((words::lesson_id, words::ord, words::id))
        .select(word_examples::examples)
        .load(conn)?;
    Ok(rows.into_iter().flat_map(|json| serde_json::from_str::<Vec<Example>>(&json).unwrap_or_default()).collect())
}

/// The words of these lessons that have no stored examples yet.
pub fn missing(conn: &mut Conn, lesson_ids: &[LessonId]) -> Result<Vec<WordId>> {
    let with: Vec<WordId> = word_examples::table.select(word_examples::word_id).load(conn)?;
    Ok(words::table
        .filter(words::lesson_id.eq_any(lesson_ids))
        .order((words::lesson_id, words::ord, words::id))
        .select(words::id)
        .load::<WordId>(conn)?
        .into_iter()
        .filter(|id| !with.contains(id))
        .collect())
}

/// A word's examples: the stored ones, or made now by the AI and stored.
/// `None` when they cannot be made (no key or model, or the reply could not
/// be read): nothing is stored then, so the AI can try next time.
pub async fn make(db: &Db, word_id: WordId) -> Result<Option<Vec<Example>>> {
    let (word, kept, ai) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let word: (String, String, String) =
            words::table.find(word_id).select((words::pl, words::en, words::pos)).first(conn)?;
        let kept = stored(conn, word_id)?;
        let ai = crate::ai::api_key(conn).and_then(|key| Ok((key, crate::ai::partner_model(conn)?))).ok();
        (word, kept, ai)
    };
    if kept.is_some() {
        return Ok(kept);
    }
    let (pl, en, pos) = &word;
    let Some((key, model)) = ai else { return Ok(None) };
    let prompt = format!(
        "A beginner learning Polish just got this word wrong: \"{pl}\" ({pos}) — \"{en}\".\n\
         Write two short, natural, everyday Polish sentences (A1–A2, at most 8 words each) that \
         use it in the meaning \"{en}\", each with its English translation. Use the word in \
         different contexts, in the form shown where natural. Reply with only JSON: \
         {{\"examples\": [{{\"pl\": \"…\", \"en\": \"…\"}}, {{\"pl\": \"…\", \"en\": \"…\"}}]}}"
    );
    let Ok(reply) = crate::ai::complete_json(&key, &model, &[Wire::user(prompt)], 3000).await else { return Ok(None) };
    #[derive(Deserialize)]
    struct Reply {
        examples: Vec<Example>,
    }
    let parsed = crate::ai::json_in::<Reply>(&reply.text).ok();
    if parsed.is_none() {
        crate::diagnostics::unreadable_reply(&format!("Example sentences (from {model})"), &reply.text);
    }
    let mut guard = db.lock();
    let conn = &mut *guard;
    let _ = crate::costs::record_tokens(conn, crate::costs::WORD_CHAT, &model, reply.cost, "examples", reply.tokens);
    let examples: Vec<Example> =
        parsed.map(|reply| reply.examples).unwrap_or_default().into_iter().filter(|e| !e.pl.trim().is_empty()).take(2).collect();
    if examples.is_empty() {
        return Ok(None);
    }
    // The word may have gone while its examples were written.
    if words::table.find(word_id).select(words::id).first::<WordId>(conn).optional()?.is_none() {
        return Ok(None);
    }
    diesel::replace_into(word_examples::table)
        .values((
            word_examples::word_id.eq(word_id),
            word_examples::examples.eq(serde_json::to_string(&examples)?),
            word_examples::created_at.eq(crate::db::now_secs()),
        ))
        .execute(conn)?;
    Ok(Some(examples))
}

/// Two short sentences using the word, with their meaning. Empty when none
/// can be had (no key and no course sentence uses it).
#[tauri::command]
pub async fn word_examples(db: State<'_, Db>, word_id: WordId) -> Result<Vec<Example>> {
    if let Some(examples) = make(&db, word_id).await? {
        return Ok(examples);
    }
    // Offline, or the AI failed: sentences from the learner's own courses.
    let word: String = words::table.find(word_id).select(words::pl).first(&mut *db.lock())?;
    from_courses(&mut db.lock(), &word)
}

/// Up to two course sentences that contain the word as a whole word.
fn from_courses(conn: &mut Conn, word: &str) -> Result<Vec<Example>> {
    let needle = word.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let candidates: Vec<(String, String)> = sentences::table
        .filter(sentences::pl.like(format!("%{}%", word.trim())))
        .select((sentences::pl, sentences::en))
        .limit(50)
        .load(conn)?;
    let mut seen = Vec::<String>::new();
    let mut found = Vec::new();
    for (pl, en) in candidates {
        if contains_word(&pl, &needle) && !seen.contains(&pl) {
            seen.push(pl.clone());
            found.push(Example { pl, en });
            if found.len() == 2 {
                break;
            }
        }
    }
    Ok(found)
}

/// True when `needle` (lowercase, possibly several words) appears in `text`
/// bounded by non-letters on both sides.
fn contains_word(text: &str, needle: &str) -> bool {
    let haystack = text.to_lowercase();
    let mut from = 0;
    while let Some(at) = haystack[from..].find(needle) {
        let start = from + at;
        let end = start + needle.len();
        let before = haystack[..start].chars().next_back();
        let after = haystack[end..].chars().next();
        if !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric) {
            return true;
        }
        from = start + needle.chars().next().map_or(1, char::len_utf8);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_whole_words_only() {
        assert!(contains_word("Jeszcze nie wiem.", "jeszcze"));
        assert!(contains_word("Czy jest jeszcze kawa?", "jeszcze"));
        assert!(!contains_word("Tak, to tak.", "ta"));
        assert!(contains_word("Dzień dobry, pani!", "dzień dobry"));
        assert!(!contains_word("Mamę", "mam"));
    }
}
