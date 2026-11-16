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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Example {
    pub pl: String,
    pub en: String,
}

/// Two short sentences using the word, with their meaning. Empty when none
/// can be had (no key and no course sentence uses it).
#[tauri::command]
pub async fn word_examples(db: State<'_, Db>, word_id: i64) -> Result<Vec<Example>> {
    let (word, stored, ai) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let word: (String, String, String) =
            words::table.find(word_id).select((words::pl, words::en, words::pos)).first(conn)?;
        let stored: Option<String> = word_examples::table
            .find(word_id)
            .select(word_examples::examples)
            .first(conn)
            .optional()?;
        let ai = crate::ai::api_key(conn).and_then(|key| Ok((key, crate::ai::partner_model(conn)?))).ok();
        (word, stored, ai)
    };
    if let Some(examples) = stored.and_then(|json| serde_json::from_str::<Vec<Example>>(&json).ok()) {
        return Ok(examples);
    }
    let (pl, en, pos) = &word;
    if let Some((key, model)) = ai {
        let prompt = format!(
            "A beginner learning Polish just got this word wrong: \"{pl}\" ({pos}) — \"{en}\".\n\
             Write two short, natural, everyday Polish sentences (A1–A2, at most 8 words each) that \
             use it in the meaning \"{en}\", each with its English translation. Use the word in \
             different contexts, in the form shown where natural. Reply with only JSON: \
             {{\"examples\": [{{\"pl\": \"…\", \"en\": \"…\"}}, {{\"pl\": \"…\", \"en\": \"…\"}}]}}"
        );
        if let Ok(reply) = crate::ai::complete(&key, &model, &[Wire::user(prompt)]).await {
            #[derive(Deserialize)]
            struct Reply {
                examples: Vec<Example>,
            }
            let parsed = json_in(&reply.text).and_then(|json| serde_json::from_str::<Reply>(json).ok());
            let mut guard = db.lock();
            let conn = &mut *guard;
            let _ = crate::costs::record_tokens(conn, crate::costs::WORD_CHAT, &model, reply.cost, "examples", reply.tokens);
            if let Some(Reply { examples }) = parsed {
                let examples: Vec<Example> = examples
                    .into_iter()
                    .filter(|e| !e.pl.trim().is_empty())
                    .take(2)
                    .collect();
                if !examples.is_empty() {
                    diesel::replace_into(word_examples::table)
                        .values((
                            word_examples::word_id.eq(word_id),
                            word_examples::examples.eq(serde_json::to_string(&examples)?),
                            word_examples::created_at.eq(crate::db::now_secs()),
                        ))
                        .execute(conn)?;
                    return Ok(examples);
                }
            }
        }
    }
    // Offline, or the AI failed: not stored, so the AI can try next time.
    from_courses(&mut db.lock(), pl)
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

fn json_in(text: &str) -> Option<&str> {
    let (start, end) = (text.find('{')?, text.rfind('}')?);
    (start < end).then(|| &text[start..=end])
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
