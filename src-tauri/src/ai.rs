//! OpenRouter access. Every request is made here, in Rust, for two reasons:
//! the webview never issues a cross-origin request (so CORS never applies), and
//! the API key never has to be handed to JavaScript.



use serde::{Deserialize, Serialize};

use crate::db::{self, Conn};
use crate::error::{Error, Result};
use crate::models::AiModel;

const BASE_URL: &str = "https://openrouter.ai/api/v1";


pub fn api_key(conn: &mut Conn) -> Result<String> {
    db::get_setting(conn, "openrouter_api_key")?
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| {
            Error::msg("No OpenRouter API key yet. Add one in Settings to turn on the AI tutor.")
        })
}

/// The chosen chat model, or an empty string. Model ids go stale, so the app
/// never ships one — the learner loads the live catalogue and picks.
pub fn stored_model(conn: &mut Conn, key: &str) -> String {
    db::get_setting(conn, key)
        .ok()
        .flatten()
        .map(|m| m.trim().to_string())
        .unwrap_or_default()
}

pub fn require_model(conn: &mut Conn) -> Result<String> {
    let model = stored_model(conn, "ai_model");
    if model.is_empty() {
        return Err(Error::msg(
            "No AI model chosen yet. Open Settings, load the models from OpenRouter and pick one.",
        ));
    }
    Ok(model)
}

/// Draws course pictures unless another image model is chosen.
pub const DEFAULT_IMAGE_MODEL: &str = "google/gemini-2.5-flash-image";

pub fn image_model(conn: &mut Conn) -> String {
    let model = stored_model(conn, "image_model");
    if model.is_empty() { DEFAULT_IMAGE_MODEL.to_string() } else { model }
}

/// Asks an image model for a picture. Returns its type, bytes and cost.
pub async fn generate_image(key: &str, model: &str, prompt: &str) -> Result<(String, Vec<u8>, Option<f64>)> {
    let response = crate::http::api()
        .post(format!("{BASE_URL}/chat/completions"))
        .bearer_auth(key)
        .header("HTTP-Referer", "https://github.com/madsbuch/learn-polish-")
        .header("X-Title", "Polski")
        .json(&serde_json::json!({
            "model": model,
            "messages": [{ "role": "user", "content": prompt }],
            "modalities": ["image", "text"],
            "usage": { "include": true },
        }))
        .send()
        .await?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|_| Error::msg(format!("OpenRouter returned an unexpected response ({status}).")))?;
    if let Some(message) = body.pointer("/error/message").and_then(|m| m.as_str()) {
        return Err(Error::msg(format!("OpenRouter: {message}")));
    }
    let url = body
        .pointer("/choices/0/message/images/0/image_url/url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| Error::msg(format!("The model {model} sent no picture. Pick one that makes images in Settings.")))?;
    // A data URI: data:image/png;base64,....
    let (head, data) = url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(','))
        .ok_or_else(|| Error::msg("The picture came in a form the app can't read."))?;
    let mime = head.split(';').next().unwrap_or("image/png").to_string();
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|_| Error::msg("The picture could not be decoded."))?;
    let cost = body.pointer("/usage/cost").and_then(|c| c.as_f64());
    Ok((mime, bytes, cost))
}

/// The writing partner's model: its own setting (a small, fast model suits
/// it), else the main model.
pub fn partner_model(conn: &mut Conn) -> Result<String> {
    let model = stored_model(conn, "partner_model");
    if !model.is_empty() {
        return Ok(model);
    }
    require_model(conn)
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
pub fn voice_for(conn: &mut Conn, gender: &str) -> String {
    // Words and sentences have no gender ("") and read in the female voice:
    // the one chosen for it, not just the default.
    let gender = if gender == "m" { "m" } else { "f" };
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
    /// OpenAI-style function definitions, for the tutor.
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a serde_json::Value>,
    /// Asks OpenRouter to report what the request cost, for the cost center.
    usage: UsageRequest,
}

#[derive(Serialize)]
struct UsageRequest {
    include: bool,
}

/// OpenRouter's accounting for one request. `cost` is in USD credits.
#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    cost: Option<f64>,
}

/// A finished, non-streamed answer and what it cost, when reported.
#[derive(Debug)]
pub struct Reply {
    pub text: String,
    pub cost: Option<f64>,
}

/// One message on the wire. `content` is null on an assistant turn that only
/// calls tools; `tool_call_id` marks the answer to one of those calls.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Wire {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "function_type")]
    pub kind: String,
    pub function: FunctionCall,
}

fn function_type() -> String {
    "function".into()
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    /// JSON, as a string — exactly what the model produced.
    #[serde(default)]
    pub arguments: String,
}

impl Wire {
    fn text(role: &str, content: impl Into<String>) -> Self {
        Wire {
            role: role.into(),
            content: Some(content.into()),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }
    pub fn system(content: impl Into<String>) -> Self {
        Wire::text("system", content)
    }
    pub fn user(content: impl Into<String>) -> Self {
        Wire::text("user", content)
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Wire::text("assistant", content)
    }
    /// An assistant turn that asked for tools to run.
    pub fn tool_request(content: &str, tool_calls: Vec<ToolCall>) -> Self {
        Wire {
            role: "assistant".into(),
            content: if content.trim().is_empty() { None } else { Some(content.to_string()) },
            tool_calls,
            tool_call_id: None,
        }
    }
    /// The result of one tool call, handed back to the model.
    pub fn tool_result(call_id: &str, content: impl Into<String>) -> Self {
        Wire {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: vec![],
            tool_call_id: Some(call_id.to_string()),
        }
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    error: Option<ApiError>,
    #[serde(default)]
    usage: Option<Usage>,
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
pub async fn complete(key: &str, model: &str, messages: &[Wire]) -> Result<Reply> {
    complete_with(key, model, messages, 0.4, 1200).await
}

/// `complete` with explicit sampling: songs want more creativity and room.
pub async fn complete_with(
    key: &str,
    model: &str,
    messages: &[Wire],
    temperature: f32,
    max_tokens: u32,
) -> Result<Reply> {
    let response = crate::http::api()
        .post(format!("{BASE_URL}/chat/completions"))
        .bearer_auth(key)
        // OpenRouter uses these for attribution on its dashboard.
        .header("HTTP-Referer", "https://github.com/madsbuch/learn-polish-")
        .header("X-Title", "Polski")
        .json(&ChatRequest {
            model,
            messages,
            temperature,
            max_tokens,
            stream: false,
            tools: None,
            usage: UsageRequest { include: true },
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

    let cost = parsed.usage.and_then(|u| u.cost);
    parsed
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .filter(|text| !text.trim().is_empty())
        .map(|text| Reply { text, cost })
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
    /// Sent on the last chunk, before `[DONE]`.
    #[serde(default)]
    usage: Option<Usage>,
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
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

/// A fragment of a tool call. The id and name come in the first fragment for
/// an index; the JSON arguments arrive in pieces after it.
#[derive(Deserialize, Default)]
struct ToolCallDelta {
    #[serde(default)]
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionDelta>,
}

#[derive(Deserialize, Default)]
struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// What a server-sent-event line turned out to mean.
#[derive(Debug, PartialEq)]
pub enum SseItem {
    Delta(String),
    ToolCall {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    /// What the whole request cost, in USD credits.
    Cost(f64),
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
            if let Some(cost) = parsed.usage.and_then(|u| u.cost) {
                items.push(SseItem::Cost(cost));
            }
            for choice in parsed.choices {
                match choice.delta.content {
                    Some(text) if !text.is_empty() => items.push(SseItem::Delta(text)),
                    _ => {}
                }
                for call in choice.delta.tool_calls {
                    let (name, arguments) = match call.function {
                        Some(f) => (f.name, f.arguments.unwrap_or_default()),
                        None => (None, String::new()),
                    };
                    items.push(SseItem::ToolCall { index: call.index, id: call.id, name, arguments });
                }
            }
        }
        items
    }
}

/// One streamed assistant turn: whatever text it wrote, and the tools it
/// asked to run.
#[derive(Debug, Default)]
pub struct Turn {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    /// What the request cost, when OpenRouter reported it.
    pub cost: Option<f64>,
}

/// Collects tool-call fragments, keyed by the index the model gives them.
#[derive(Default)]
pub struct ToolCallAssembler {
    calls: Vec<ToolCall>,
}

impl ToolCallAssembler {
    pub fn push(&mut self, index: usize, id: Option<String>, name: Option<String>, arguments: &str) {
        while self.calls.len() <= index {
            self.calls.push(ToolCall { kind: function_type(), ..Default::default() });
        }
        let call = &mut self.calls[index];
        if let Some(id) = id.filter(|id| !id.is_empty()) {
            call.id = id;
        }
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            call.function.name = name;
        }
        call.function.arguments.push_str(arguments);
    }

    pub fn finish(self) -> Vec<ToolCall> {
        self.calls
            .into_iter()
            .enumerate()
            .filter(|(_, call)| !call.function.name.is_empty())
            .map(|(i, mut call)| {
                // A few providers leave the id out; the model only needs it to
                // pair each result with its call.
                if call.id.is_empty() {
                    call.id = format!("call_{i}");
                }
                if call.function.arguments.trim().is_empty() {
                    call.function.arguments = "{}".into();
                }
                call
            })
            .collect()
    }
}

/// Streams one turn, optionally offering tools. Text fragments go to
/// `on_delta` as they arrive; tool calls are assembled and returned whole.
pub async fn stream_turn<F>(
    key: &str,
    model: &str,
    messages: &[Wire],
    tools: Option<&serde_json::Value>,
    mut on_delta: F,
) -> Result<Turn>
where
    F: FnMut(&str),
{
    use futures_util::StreamExt;

    let response = crate::http::api()
        .post(format!("{BASE_URL}/chat/completions"))
        .bearer_auth(key)
        .header("HTTP-Referer", "https://github.com/madsbuch/learn-polish-")
        .header("X-Title", "Polski")
        .header("Accept", "text/event-stream")
        .json(&ChatRequest {
            model,
            messages,
            temperature: 0.4,
            // Room for a whole course's worth of tool arguments.
            max_tokens: if tools.is_some() { 8000 } else { 1200 },
            stream: true,
            tools,
            usage: UsageRequest { include: true },
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
    let mut text = String::new();
    let mut calls = ToolCallAssembler::default();
    let mut cost = None;

    'read: while let Some(chunk) = stream.next().await {
        for item in parser.push(&String::from_utf8_lossy(&chunk?)) {
            match item {
                SseItem::Delta(delta) => {
                    text.push_str(&delta);
                    on_delta(&delta);
                }
                SseItem::ToolCall { index, id, name, arguments } => {
                    calls.push(index, id, name, &arguments)
                }
                SseItem::Cost(value) => cost = Some(value),
                SseItem::Done => break 'read,
                SseItem::Failed(message) => {
                    return Err(Error::msg(format!("OpenRouter: {message}")))
                }
            }
        }
    }

    let tool_calls = calls.finish();
    if text.trim().is_empty() && tool_calls.is_empty() {
        return Err(Error::msg("OpenRouter returned an empty answer."));
    }
    Ok(Turn { text, tool_calls, cost })
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
    #[serde(default)]
    supported_parameters: Option<Vec<String>>,
    #[serde(default)]
    architecture: Option<Architecture>,
}

#[derive(Deserialize)]
struct Architecture {
    #[serde(default)]
    output_modalities: Vec<String>,
}

#[derive(Deserialize)]
struct Pricing {
    #[serde(default)]
    prompt: Option<String>,
}

async fn fetch_catalogue(query: &str) -> Result<Vec<AiModel>> {
    // The catalogue is public, so this works before a key is entered.
    let response = crate::http::api().get(format!("{BASE_URL}/models{query}")).send().await?;
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
            tools: m
                .supported_parameters
                .is_some_and(|params| params.iter().any(|p| p == "tools")),
            images: m.architecture.is_some_and(|a| a.output_modalities.iter().any(|o| o == "image")),
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

/// Gemini's speech model takes a prompt, not only a transcript: bare text
/// that is also English ("transport") gets an American accent, and a lone
/// word ("prom") can be taken as a cue to say something else entirely.
/// Saying what the text is, and that nothing else is wanted, fixes both.
pub fn speech_prompt(text: &str) -> String {
    format!(
        "Read the following Polish text aloud with native Polish pronunciation. It is Polish, \
         not English. Say exactly this and nothing else: {text}"
    )
}

async fn speech_request(
    key: &str,
    model: &str,
    voice: &str,
    text: &str,
    speed: Option<f32>,
    format: &str,
) -> Result<Clip> {
    let response = crate::http::api()
        .post(format!("{BASE_URL}/audio/speech"))
        .bearer_auth(key)
        .header("HTTP-Referer", "https://github.com/madsbuch/learn-polish-")
        .header("X-Title", "Polski")
        .json(&SpeechRequest {
            model,
            input: &speech_prompt(text),
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
    fn sse_reports_the_cost_from_the_usage_chunk() {
        let mut parser = SseParser::default();
        assert_eq!(
            parser.push(
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"cost\":0.00042}}\n"
            ),
            vec![SseItem::Cost(0.00042)]
        );
        // Usage without a price says nothing.
        assert!(parser.push("data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10}}\n").is_empty());
    }

    #[test]
    fn sse_assembles_tool_calls_from_fragments() {
        let mut parser = SseParser::default();
        let mut items = parser.push(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"create_course\",\"arguments\":\"\"}}]}}]}\n",
        );
        items.extend(parser.push(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"title\\\":\"}}]}}]}\n",
        ));
        items.extend(parser.push(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Colours\\\"}\"}}]}}]}\ndata: [DONE]\n",
        ));

        let mut calls = ToolCallAssembler::default();
        for item in items {
            if let SseItem::ToolCall { index, id, name, arguments } = item {
                calls.push(index, id, name, &arguments);
            }
        }
        let calls = calls.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].function.name, "create_course");
        assert_eq!(calls[0].function.arguments, "{\"title\":\"Colours\"}");
    }

    #[test]
    fn tool_turns_serialise_the_way_openai_expects() {
        let request = Wire::tool_request(
            "",
            vec![ToolCall {
                id: "c1".into(),
                kind: "function".into(),
                function: FunctionCall { name: "list_topics".into(), arguments: "{}".into() },
            }],
        );
        let json = serde_json::to_value(&request).unwrap();
        assert!(json["content"].is_null(), "a tool-only turn has null content");
        assert_eq!(json["tool_calls"][0]["type"], "function");
        assert!(json.get("tool_call_id").is_none());

        let result = serde_json::to_value(Wire::tool_result("c1", "[]")).unwrap();
        assert_eq!(result["role"], "tool");
        assert_eq!(result["tool_call_id"], "c1");
        assert!(result.get("tool_calls").is_none());
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

pub fn explain_prompt(polish: &str, meaning: &str) -> Vec<Wire> {
    vec![
        Wire::system(
            "You explain Polish to an English-speaking beginner. Answer in English. \
             Use short markdown sections and never exceed 200 words.",
        ),
        Wire::user(format!(
            "Explain the Polish item below.\n\n\
             Polish: {polish}\nEnglish: {meaning}\n\n\
             Cover, briefly:\n\
             1. What it literally means, word by word if it is a phrase.\n\
             2. The grammar a learner needs here (case, aspect, gender) — only what applies.\n\
             3. Pronunciation written for an English speaker.\n\
             4. One extra example sentence with its translation."
        )),
    ]
}
