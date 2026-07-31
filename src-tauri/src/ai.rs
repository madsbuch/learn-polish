//! OpenRouter access. Every request is made here, in Rust, for two reasons:
//! the webview never issues a cross-origin request (so CORS never applies), and
//! the API key never has to be handed to JavaScript.

use std::sync::OnceLock;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db;
use crate::error::{Error, Result};
use crate::models::{AiModel, ChatMessage};

const BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_MODEL: &str = "anthropic/claude-3.5-haiku";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("learn-polish/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("failed to build HTTP client")
    })
}

pub fn api_key(conn: &Connection) -> Result<String> {
    db::get_setting(conn, "openrouter_api_key")?
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| {
            Error::msg("No OpenRouter API key yet. Add one in Settings to turn on the AI tutor.")
        })
}

pub fn model(conn: &Connection) -> String {
    db::get_setting(conn, "ai_model")
        .ok()
        .flatten()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Wire],
    temperature: f32,
    max_tokens: u32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Wire {
    pub role: String,
    pub content: String,
}

impl Wire {
    pub fn system(content: impl Into<String>) -> Self {
        Wire { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Wire { role: "user".into(), content: content.into() }
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct Choice {
    message: Wire,
}

#[derive(Deserialize)]
struct ApiError {
    message: String,
}

/// Sends a completion request and returns the assistant's text.
pub async fn complete(key: &str, model: &str, messages: &[Wire]) -> Result<String> {
    let response = client()
        .post(format!("{BASE_URL}/chat/completions"))
        .bearer_auth(key)
        // OpenRouter uses these for attribution on its dashboard.
        .header("HTTP-Referer", "https://github.com/madsbuch/learn-polish-")
        .header("X-Title", "Polski")
        .json(&ChatRequest { model, messages, temperature: 0.4, max_tokens: 1200 })
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    let parsed: ChatResponse = serde_json::from_str(&body).map_err(|_| {
        Error::msg(format!(
            "OpenRouter returned an unexpected response ({status}): {}",
            body.chars().take(300).collect::<String>()
        ))
    })?;

    if let Some(error) = parsed.error {
        return Err(Error::msg(format!("OpenRouter: {}", error.message)));
    }
    if !status.is_success() {
        return Err(Error::msg(format!("OpenRouter returned HTTP {status}")));
    }

    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| Error::msg("OpenRouter returned an empty answer."))
}

#[derive(Deserialize)]
struct ModelList {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    context_length: Option<i64>,
    #[serde(default)]
    pricing: Option<Pricing>,
}

#[derive(Deserialize)]
struct Pricing {
    #[serde(default)]
    prompt: Option<String>,
}

pub async fn list_models() -> Result<Vec<AiModel>> {
    // The model catalogue is public, so this works before a key is entered.
    let response = client().get(format!("{BASE_URL}/models")).send().await?;
    if !response.status().is_success() {
        return Err(Error::msg(format!(
            "Could not load the model list (HTTP {}).",
            response.status()
        )));
    }
    let list: ModelList = response.json().await?;
    let mut models: Vec<AiModel> = list
        .data
        .into_iter()
        .map(|m| AiModel {
            name: m.name.clone().unwrap_or_else(|| m.id.clone()),
            context_length: m.context_length.unwrap_or(0),
            prompt_price: m.pricing.and_then(|p| p.prompt).unwrap_or_default(),
            id: m.id,
        })
        .collect();
    models.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(models)
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

fn language_name(code: &str) -> &'static str {
    if code == "da" {
        "Danish"
    } else {
        "English"
    }
}

pub fn tutor_system_prompt(source: &str, topic_title: &str, vocabulary: &[String]) -> String {
    format!(
        "You are a warm, patient Polish tutor for an adult beginner whose language is {lang}.\n\
         Today's topic is \"{topic}\".\n\
         Rules:\n\
         - Reply in Polish first, then give a short {lang} translation in brackets.\n\
         - Keep sentences short and use the present tense unless the learner asks otherwise.\n\
         - Prefer these words when they fit: {vocab}.\n\
         - If the learner writes Polish with mistakes, gently show the corrected sentence \
           before continuing the conversation.\n\
         - Never write more than four sentences per turn. Always end with a question so the \
           conversation keeps going.",
        lang = language_name(source),
        topic = topic_title,
        vocab = vocabulary.join(", ")
    )
}

pub fn explain_prompt(source: &str, polish: &str, meaning: &str) -> Vec<Wire> {
    let lang = language_name(source);
    vec![
        Wire::system(format!(
            "You explain Polish to a {lang}-speaking beginner. Answer in {lang}. \
             Use short markdown sections and never exceed 200 words."
        )),
        Wire::user(format!(
            "Explain the Polish item below.\n\n\
             Polish: {polish}\n{lang}: {meaning}\n\n\
             Cover, briefly:\n\
             1. What it literally means, word by word if it is a phrase.\n\
             2. The grammar a learner needs here (case, aspect, gender) — only what applies.\n\
             3. Pronunciation written for a {lang} speaker.\n\
             4. One extra example sentence with its translation."
        )),
    ]
}

pub fn feedback_prompt(source: &str, target_polish: &str, attempt: &str) -> Vec<Wire> {
    let lang = language_name(source);
    vec![
        Wire::system(format!(
            "You are a Polish teacher marking one short translation. \
             Answer in {lang}, in under 120 words, and be encouraging but precise."
        )),
        Wire::user(format!(
            "The learner was asked to produce this Polish sentence:\n{target_polish}\n\n\
             They wrote:\n{attempt}\n\n\
             Say whether it is acceptable, point out any mistake and why it is a mistake, \
             then give the most natural version."
        )),
    ]
}

pub fn drill_prompt(source: &str, words: &[String]) -> Vec<Wire> {
    let lang = language_name(source);
    vec![
        Wire::system(format!(
            "You write practice material for a Polish beginner who speaks {lang}. \
             Output nothing but the requested lines."
        )),
        Wire::user(format!(
            "Write five short Polish sentences (max 8 words each) that use these words: {}.\n\
             Put each sentence on its own line as: Polish sentence — {lang} translation.\n\
             No numbering, no extra commentary.",
            words.join(", ")
        )),
    ]
}

// ---------------------------------------------------------------------------
// Chat history
// ---------------------------------------------------------------------------

/// How much of the conversation we replay to the model. Older turns are kept in
/// the database for the learner but dropped from the request.
const HISTORY_LIMIT: i64 = 20;

pub fn load_history(conn: &Connection, topic_id: i64) -> Result<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(
        "SELECT role, content FROM chat_messages WHERE topic_id = ?1 ORDER BY id",
    )?;
    let messages = stmt
        .query_map([topic_id], |r| {
            Ok(ChatMessage { role: r.get(0)?, content: r.get(1)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(messages)
}

pub fn recent_history(conn: &Connection, topic_id: i64) -> Result<Vec<Wire>> {
    let mut stmt = conn.prepare(
        "SELECT role, content FROM (
             SELECT id, role, content FROM chat_messages
             WHERE topic_id = ?1 ORDER BY id DESC LIMIT ?2
         ) ORDER BY id",
    )?;
    let history = stmt
        .query_map(rusqlite::params![topic_id, HISTORY_LIMIT], |r| {
            Ok(Wire { role: r.get(0)?, content: r.get(1)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(history)
}

pub fn append_message(conn: &Connection, topic_id: i64, role: &str, content: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO chat_messages (topic_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![topic_id, role, content, db::now_secs()],
    )?;
    Ok(())
}

pub fn clear_history(conn: &Connection, topic_id: i64) -> Result<()> {
    conn.execute("DELETE FROM chat_messages WHERE topic_id = ?1", [topic_id])?;
    Ok(())
}
