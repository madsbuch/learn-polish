//! The one loop every AI feature runs through: the tutor, the examiner and
//! the explanations.
//!
//! A run streams the model's answer to the frontend as `ai://stream` events,
//! logs what each request cost, and — when tools are offered — runs the tool
//! calls the model makes and lets it continue, round after round, until it
//! answers. The memory tools are handled here for every agent, so what one
//! agent remembers is saved the same way as another's.
//!
//! What differs between agents is passed in: the tools, how a tool call is
//! run ([`Agent::run`]'s `call` function), and when a turn is over.

use std::future::Future;
use std::pin::Pin;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::ai::{self, ToolCall, Wire};
use crate::db::Db;
use crate::error::Error;

/// One fragment of a streamed answer, emitted as an `ai://stream` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamEvent {
    /// Matches the id the caller passed, so concurrent streams stay separate.
    pub id: String,
    pub delta: String,
    pub done: bool,
    pub error: Option<String>,
}

/// What a tool is doing, emitted as an `agent://status` event while it runs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusEvent {
    pub id: String,
    pub label: String,
}

/// Pushes a streamed fragment (or the end of the stream) to the frontend.
pub fn emit_stream(app: &AppHandle, stream_id: &str, delta: Option<&str>, done: bool, error: Option<String>) {
    let _ = app.emit(
        "ai://stream",
        StreamEvent { id: stream_id.to_string(), delta: delta.unwrap_or_default().to_string(), done, error },
    );
}

/// A tool an agent can call: what the model sees, and the line the learner
/// sees while it runs.
pub struct Tool {
    pub name: &'static str,
    /// "Building the course…" — shown while the tool runs.
    pub label: &'static str,
    pub definition: Value,
}

/// Declares a tool: its name, its "working on it" line, what it does (for
/// the model) and its JSON-schema parameters.
pub fn tool(name: &'static str, label: &'static str, description: &str, parameters: Value) -> Tool {
    Tool {
        name,
        label,
        definition: serde_json::json!({
            "type": "function",
            "function": { "name": name, "description": description, "parameters": parameters }
        }),
    }
}

/// What running one tool call produced.
pub struct ToolOutput<T> {
    /// Sent back to the model.
    pub result: Value,
    /// For the caller, collected over the run (e.g. an action to show).
    pub extra: Option<T>,
    /// The turn is over once this round is done (e.g. the examiner's verdict).
    pub ends_turn: bool,
}

impl<T> ToolOutput<T> {
    pub fn reply(result: Value) -> Self {
        ToolOutput { result, extra: None, ends_turn: false }
    }
}

pub type ToolFuture<'a, T> = Pin<Box<dyn Future<Output = ToolOutput<T>> + Send + 'a>>;

/// How a run ended. A run that failed half-way still reports what its tools
/// already did, since that happened in the app.
pub struct Outcome<T> {
    /// Everything the model said, rounds separated by blank lines.
    pub text: String,
    pub extras: Vec<T>,
    /// What memory tools saved or changed, one line each, for the learner.
    pub remembered: Vec<String>,
    pub failed: Option<Error>,
}

pub struct Agent<'a> {
    pub app: &'a AppHandle,
    pub db: &'a Db,
    /// Which agent this is: for the cost center, and as a memory's source.
    pub name: &'static str,
    pub stream_id: &'a str,
    /// Tools offered to the model; the memory tools are added when memory is
    /// on. None for a plain answer.
    pub tools: Option<Vec<Tool>>,
    /// Tool rounds before tools are withheld, so the model has to answer.
    pub max_rounds: usize,
    /// End the turn as soon as the model has said something (the examiner asks
    /// one question per turn); otherwise go on while it calls tools.
    pub stop_once_answered: bool,
    /// Say what each tool is doing while it runs (`agent://status`).
    pub show_status: bool,
}

impl<'a> Agent<'a> {
    /// Runs the conversation in `wire` until the model answers. Tool calls
    /// other than memory ones go to `call`.
    pub async fn run<T, F>(&self, mut wire: Vec<Wire>, mut call: F) -> Outcome<T>
    where
        F: FnMut(ToolCall) -> ToolFuture<'a, T>,
    {
        let mut outcome = Outcome { text: String::new(), extras: Vec::new(), remembered: Vec::new(), failed: None };
        let (key, model) = {
            let conn = self.db.lock();
            match (ai::api_key(&conn), ai::require_model(&conn)) {
                (Ok(key), Ok(model)) => (key, model),
                (Err(error), _) | (_, Err(error)) => {
                    outcome.failed = Some(error);
                    emit_stream(self.app, self.stream_id, None, true, outcome.failed.as_ref().map(|e| e.to_string()));
                    return outcome;
                }
            }
        };
        let memory_tools = match &self.tools {
            Some(_) if crate::memories::enabled(&self.db.lock()) => crate::memories::tools(),
            _ => Vec::new(),
        };
        let all: Vec<&Tool> = self.tools.iter().flatten().chain(memory_tools.iter()).collect();
        let tools = self.tools.as_ref().map(|_| Value::Array(all.iter().map(|t| t.definition.clone()).collect()));

        for round in 0..self.max_rounds.max(1) {
            let offer = tools.as_ref().filter(|_| round + 1 < self.max_rounds);
            // Text from successive rounds is one reply; keep the paragraphs apart.
            let mut separated = outcome.text.is_empty();
            let (app, id) = (self.app.clone(), self.stream_id.to_string());
            let turn = ai::stream_turn(&key, &model, &wire, offer, |delta| {
                if !separated {
                    emit_stream(&app, &id, Some("\n\n"), false, None);
                    separated = true;
                }
                emit_stream(&app, &id, Some(delta), false, None);
            })
            .await;
            let turn = match turn {
                Ok(turn) => turn,
                Err(error) => {
                    outcome.failed = Some(explain_failure(error, &model));
                    break;
                }
            };
            let _ = crate::costs::record(&self.db.lock(), self.name, "openrouter", &model, turn.cost, "");

            let text = turn.text.trim();
            if !text.is_empty() {
                if !outcome.text.is_empty() {
                    outcome.text.push_str("\n\n");
                }
                outcome.text.push_str(text);
            }
            if turn.tool_calls.is_empty() {
                break;
            }

            wire.push(Wire::tool_request(&turn.text, turn.tool_calls.clone()));
            let mut ends_turn = false;
            for tool_call in turn.tool_calls {
                if self.show_status {
                    let label = all
                        .iter()
                        .find(|t| t.name == tool_call.function.name)
                        .map_or("Working…", |t| t.label);
                    let _ = self.app.emit(
                        "agent://status",
                        StatusEvent { id: self.stream_id.to_string(), label: label.into() },
                    );
                }
                let id = tool_call.id.clone();
                let result = if crate::memories::is_tool(&tool_call.function.name) {
                    let saved = crate::memories::run_tool(
                        &self.db.lock(),
                        self.name,
                        &tool_call.function.name,
                        &tool_call.function.arguments,
                    );
                    outcome.remembered.extend(saved.label);
                    saved.result
                } else {
                    let output = call(tool_call).await;
                    outcome.extras.extend(output.extra);
                    ends_turn |= output.ends_turn;
                    output.result
                };
                wire.push(Wire::tool_result(&id, result.to_string()));
            }
            if ends_turn || (self.stop_once_answered && !outcome.text.is_empty()) {
                break;
            }
        }

        emit_stream(self.app, self.stream_id, None, true, outcome.failed.as_ref().map(|e| e.to_string()));
        outcome
    }
}

/// OpenRouter's error for a model without tool support is cryptic; this says
/// what to do instead.
pub fn explain_failure(error: Error, model: &str) -> Error {
    let text = error.to_string();
    let lower = text.to_lowercase();
    if lower.contains("tool") && (lower.contains("support") || lower.contains("no endpoints")) {
        Error::msg(format!(
            "The model {model} can't use tools, so the tutor can't act in the app with it. \
             In Settings, load the models and pick one marked 🛠."
        ))
    } else {
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_only_models_get_a_helpful_error() {
        let error = explain_failure(
            Error::msg("OpenRouter (404): No endpoints found that support tool use."),
            "some/model",
        );
        assert!(error.to_string().contains("🛠"));
        let other = explain_failure(Error::msg("rate limited"), "some/model");
        assert_eq!(other.to_string(), "rate limited");
    }
}
