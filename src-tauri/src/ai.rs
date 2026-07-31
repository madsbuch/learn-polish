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

/// The chosen chat model, or an empty string. Model ids go stale, so the app
/// never ships one — the learner loads the live catalogue and picks.
pub fn stored_model(conn: &Connection, key: &str) -> String {
    db::get_setting(conn, key)
        .ok()
        .flatten()
        .map(|m| m.trim().to_string())
        .unwrap_or_default()
}

pub fn require_model(conn: &Connection) -> Result<String> {
    let model = stored_model(conn, "ai_model");
    if model.is_empty() {
        return Err(Error::msg(
            "No AI model chosen yet. Open Settings, load the models from OpenRouter and pick one.",
        ));
    }
    Ok(model)
}

/// Speech is pinned to Gemini: it handles Polish well, it is the cheapest of
/// the models that do, and pinning it means the voice list can ship with the
/// app instead of needing a network round trip to populate a dropdown.
pub const SPEECH_MODEL: &str = "google/gemini-3.1-flash-tts-preview";
pub const DEFAULT_FEMALE_VOICE: &str = "Kore";
pub const DEFAULT_MALE_VOICE: &str = "Puck";

/// Gemini's voices, with the gender and character Google publishes for each.
pub const GEMINI_VOICES: &[(&str, &str, &str)] = &[
    ("Achernar", "f", "soft"),
    ("Achird", "m", "friendly"),
    ("Algenib", "m", "gravelly"),
    ("Algieba", "m", "smooth"),
    ("Alnilam", "m", "firm"),
    ("Aoede", "f", "breezy"),
    ("Autonoe", "f", "bright"),
    ("Callirrhoe", "f", "easy-going"),
    ("Charon", "m", "informative"),
    ("Despina", "f", "smooth"),
    ("Enceladus", "m", "breathy"),
    ("Erinome", "f", "clear"),
    ("Fenrir", "m", "excitable"),
    ("Gacrux", "f", "mature"),
    ("Iapetus", "m", "clear"),
    ("Kore", "f", "firm"),
    ("Laomedeia", "f", "upbeat"),
    ("Leda", "f", "youthful"),
    ("Orus", "m", "firm"),
    ("Puck", "m", "upbeat"),
    ("Pulcherrima", "f", "forward"),
    ("Rasalgethi", "m", "informative"),
    ("Sadachbia", "m", "lively"),
    ("Sadaltager", "m", "knowledgeable"),
    ("Schedar", "m", "even"),
    ("Sulafat", "f", "warm"),
    ("Umbriel", "m", "easy-going"),
    ("Vindemiatrix", "f", "gentle"),
    ("Zephyr", "f", "bright"),
    ("Zubenelgenubi", "m", "casual"),
];

fn known_voice(name: &str, gender: &str) -> bool {
    GEMINI_VOICES
        .iter()
        .any(|(voice, voice_gender, _)| *voice == name && *voice_gender == gender)
}

/// The voice to read a line in. `gender` is "f" or "m"; anything else, and
/// non-dialogue text, gets the female voice.
pub fn voice_for(conn: &Connection, gender: &str) -> String {
    let (key, fallback) = if gender == "m" {
        ("tts_voice_male", DEFAULT_MALE_VOICE)
    } else {
        ("tts_voice_female", DEFAULT_FEMALE_VOICE)
    };
    let chosen = stored_model(conn, key);
    if known_voice(&chosen, gender) {
        chosen
    } else {
        fallback.to_string()
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Wire],
    temperature: f32,
    max_tokens: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
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
        .json(&ChatRequest {
            model,
            messages,
            temperature: 0.4,
            max_tokens: 1200,
            stream: false,
        })
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

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Delta,
}

#[derive(Deserialize, Default)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
}

/// What a server-sent-event line turned out to mean.
#[derive(Debug, PartialEq)]
pub enum SseItem {
    Delta(String),
    Done,
    Failed(String),
}

/// Incremental parser for the server-sent-event body of a streaming
/// completion.
///
/// TCP reads split wherever they like — mid-line, mid-UTF-8, between `data:`
/// and its payload — so bytes are buffered until a line is complete. Keeping
/// this separate from the HTTP call is what makes it testable.
#[derive(Default)]
pub struct SseParser {
    buffer: String,
}

impl SseParser {
    pub fn push(&mut self, chunk: &str) -> Vec<SseItem> {
        self.buffer.push_str(chunk);
        let mut items = Vec::new();

        while let Some(newline) = self.buffer.find('\n') {
            let line = self.buffer[..newline].trim().to_string();
            self.buffer.drain(..=newline);

            // Comment lines (": OPENROUTER PROCESSING") keep the socket warm.
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload == "[DONE]" {
                items.push(SseItem::Done);
                continue;
            }

            let parsed: StreamChunk = match serde_json::from_str(payload) {
                Ok(parsed) => parsed,
                // A fragment we cannot read is not worth aborting the answer for.
                Err(_) => continue,
            };
            if let Some(error) = parsed.error {
                items.push(SseItem::Failed(error.message));
                continue;
            }
            for choice in parsed.choices {
                match choice.delta.content {
                    Some(text) if !text.is_empty() => items.push(SseItem::Delta(text)),
                    _ => {}
                }
            }
        }
        items
    }
}

/// Streams a completion, handing each new fragment of text to `on_delta` as it
/// arrives. Returns the full text once the stream ends.
pub async fn complete_streaming<F>(
    key: &str,
    model: &str,
    messages: &[Wire],
    mut on_delta: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    use futures_util::StreamExt;

    let response = client()
        .post(format!("{BASE_URL}/chat/completions"))
        .bearer_auth(key)
        .header("HTTP-Referer", "https://github.com/madsbuch/learn-polish-")
        .header("X-Title", "Polski")
        .header("Accept", "text/event-stream")
        .json(&ChatRequest {
            model,
            messages,
            temperature: 0.4,
            max_tokens: 1200,
            stream: true,
        })
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<ChatResponse>(&body)
            .ok()
            .and_then(|parsed| parsed.error.map(|e| e.message))
            .unwrap_or_else(|| body.chars().take(300).collect());
        return Err(Error::msg(format!("OpenRouter ({status}): {detail}")));
    }

    let mut stream = response.bytes_stream();
    let mut parser = SseParser::default();
    let mut answer = String::new();

    while let Some(chunk) = stream.next().await {
        for item in parser.push(&String::from_utf8_lossy(&chunk?)) {
            match item {
                SseItem::Delta(text) => {
                    answer.push_str(&text);
                    on_delta(&text);
                }
                SseItem::Done => return Ok(answer),
                SseItem::Failed(message) => {
                    return Err(Error::msg(format!("OpenRouter: {message}")))
                }
            }
        }
    }

    if answer.trim().is_empty() {
        return Err(Error::msg("OpenRouter returned an empty answer."));
    }
    Ok(answer)
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
    /// Present on speech models; the set of voices the model accepts.
    #[serde(default)]
    supported_voices: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Pricing {
    #[serde(default)]
    prompt: Option<String>,
}

async fn fetch_catalogue(query: &str) -> Result<Vec<AiModel>> {
    // The catalogue is public, so this works before a key is entered.
    let response = client().get(format!("{BASE_URL}/models{query}")).send().await?;
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
            voices: m.supported_voices.unwrap_or_default(),
            id: m.id,
        })
        .collect();
    models.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(models)
}

pub async fn list_models() -> Result<Vec<AiModel>> {
    fetch_catalogue("").await
}

#[derive(Serialize)]
struct SpeechRequest<'a> {
    model: &'a str,
    input: &'a str,
    response_format: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    voice: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<f32>,
}

/// A clip and the MIME type the webview needs to play it.
pub type Clip = (String, Vec<u8>);

/// Turns Polish text into playable audio bytes.
///
/// Providers disagree about what they accept, and the endpoint only tells you
/// by rejecting the request, so this walks a short ladder of fallbacks:
/// MP3 first (smallest, universally playable), then PCM for models that only
/// speak PCM (Gemini), and finally without `speed`, which not every model
/// honours. The original error is what surfaces if none of it works.
pub async fn synthesize(
    key: &str,
    model: &str,
    voice: &str,
    text: &str,
    speed: Option<f32>,
) -> Result<Clip> {
    let first = speech_request(key, model, voice, text, speed, "mp3").await;
    let error = match first {
        Ok(clip) => return Ok(clip),
        Err(error) => error,
    };
    let complaint = error.to_string().to_lowercase();

    let mut formats: Vec<&str> = Vec::new();
    if complaint.contains("pcm") {
        formats.push("pcm");
    }

    for format in &formats {
        if let Ok(clip) = speech_request(key, model, voice, text, speed, format).await {
            return Ok(clip);
        }
    }
    if speed.is_some() {
        for format in std::iter::once("mp3").chain(formats) {
            if let Ok(clip) = speech_request(key, model, voice, text, None, format).await {
                return Ok(clip);
            }
        }
    }
    Err(error)
}

async fn speech_request(
    key: &str,
    model: &str,
    voice: &str,
    text: &str,
    speed: Option<f32>,
    format: &str,
) -> Result<Clip> {
    let response = client()
        .post(format!("{BASE_URL}/audio/speech"))
        .bearer_auth(key)
        .header("HTTP-Referer", "https://github.com/madsbuch/learn-polish-")
        .header("X-Title", "Polski")
        .json(&SpeechRequest {
            model,
            input: text,
            response_format: format,
            voice,
            speed,
        })
        .send()
        .await?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();

    let bytes = response.bytes().await?;

    if !status.is_success() || content_type.starts_with("application/json") {
        let body = String::from_utf8_lossy(&bytes);
        let detail = serde_json::from_str::<ChatResponse>(&body)
            .ok()
            .and_then(|parsed| parsed.error.map(|e| e.message))
            .unwrap_or_else(|| body.chars().take(300).collect());
        return Err(Error::msg(format!("OpenRouter ({status}): {detail}")));
    }

    if bytes.is_empty() {
        return Err(Error::msg("OpenRouter returned no audio for that text."));
    }

    // Raw PCM has no container, so no player will touch it. The response
    // header carries the rate and channel count, e.g.
    // `audio/pcm;rate=24000;channels=1`.
    if content_type.starts_with("audio/pcm") || content_type.starts_with("audio/l16") {
        let rate = content_type_param(&content_type, "rate").unwrap_or(24_000);
        let channels = content_type_param(&content_type, "channels").unwrap_or(1) as u16;
        return Ok(("audio/wav".to_string(), wav_from_pcm(&bytes, rate, channels)));
    }

    let mime = if content_type.is_empty() {
        "audio/mpeg".to_string()
    } else {
        // Strip any parameters; the webview only needs the type itself.
        content_type
            .split(';')
            .next()
            .unwrap_or("audio/mpeg")
            .trim()
            .to_string()
    };
    Ok((mime, bytes.to_vec()))
}

fn content_type_param(content_type: &str, name: &str) -> Option<u32> {
    content_type
        .split(';')
        .skip(1)
        .filter_map(|part| part.split_once('='))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case(name))
        .and_then(|(_, value)| value.trim().trim_matches('"').parse().ok())
}

/// Wraps signed 16-bit little-endian PCM in a minimal WAV container.
fn wav_from_pcm(pcm: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
    const BITS: u16 = 16;
    let block_align = channels * BITS / 8;
    let byte_rate = sample_rate * u32::from(block_align);

    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM header size
    out.extend_from_slice(&1u16.to_le_bytes()); // format: PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&BITS.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_describes_the_payload() {
        let pcm = vec![0u8; 480];
        let wav = wav_from_pcm(&pcm, 24_000, 1);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + pcm.len());
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 480);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 24_000);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 480);
        // 24 kHz mono 16-bit is 48 000 bytes per second.
        assert_eq!(u32::from_le_bytes(wav[28..32].try_into().unwrap()), 48_000);
    }

    fn delta(payload: &str) -> String {
        format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{payload}\"}}}}]}}\n")
    }

    #[test]
    fn sse_reassembles_text_across_chunk_boundaries() {
        let body = format!("{}{}data: [DONE]\n", delta("Dzień "), delta("dobry"));
        // Split the body at every possible byte boundary; the parser must
        // produce the same answer whatever the network hands it.
        for split in 1..body.len() {
            if !body.is_char_boundary(split) {
                continue;
            }
            let mut parser = SseParser::default();
            let mut items = parser.push(&body[..split]);
            items.extend(parser.push(&body[split..]));
            assert_eq!(
                items,
                vec![
                    SseItem::Delta("Dzień ".into()),
                    SseItem::Delta("dobry".into()),
                    SseItem::Done,
                ],
                "split at {split}"
            );
        }
    }

    #[test]
    fn sse_ignores_keepalives_and_surfaces_errors() {
        let mut parser = SseParser::default();
        // OpenRouter sends comment lines to hold the connection open.
        assert!(parser.push(": OPENROUTER PROCESSING\n\n").is_empty());
        // Empty deltas (role-only first chunk) produce nothing.
        assert!(parser
            .push("data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n")
            .is_empty());
        // Malformed JSON is skipped rather than aborting the answer.
        assert!(parser.push("data: {not json\n").is_empty());

        assert_eq!(
            parser.push("data: {\"error\":{\"message\":\"rate limited\"}}\n"),
            vec![SseItem::Failed("rate limited".into())]
        );
    }

    #[test]
    fn pcm_parameters_come_from_the_content_type() {
        let header = "audio/pcm;rate=24000;channels=1";
        assert_eq!(content_type_param(header, "rate"), Some(24_000));
        assert_eq!(content_type_param(header, "channels"), Some(1));
        assert_eq!(content_type_param(header, "missing"), None);
        assert_eq!(content_type_param("audio/mpeg", "rate"), None);
    }
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
