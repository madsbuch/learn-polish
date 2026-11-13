//! Long-term memory shared by the agents (the tutor and the examiner).
//!
//! A memory is one short fact about the learner — a goal, a preference, a
//! strength, something they keep getting wrong, or context about their life
//! that shapes what is worth learning. The agents save and revise memories
//! with tools, and every agent sees the current set in its prompt, so what
//! the examiner notices in a check-in reaches the tutor and the other way
//! round. The learner can read, add and delete them in Settings → Memories.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db;
use crate::error::{Error, Result};

pub const CATEGORIES: [&str; 5] = ["goal", "preference", "context", "strength", "struggle"];

/// How many memories go into a prompt: the most recently touched first.
const PROMPT_LIMIT: usize = 80;
/// A memory is a sentence or two, not an essay.
const MAX_CHARS: usize = 400;
/// Above this, the agents are told to merge or forget before adding more.
const SOFT_LIMIT: i64 = 150;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: i64,
    pub content: String,
    pub category: String,
    /// "tutor", "examiner" or "you".
    pub source: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn enabled(conn: &Connection) -> bool {
    db::get_setting(conn, "memories_enabled").ok().flatten().as_deref() != Some("0")
}

fn clean_category(category: &str) -> String {
    let category = category.trim().to_lowercase();
    if CATEGORIES.contains(&category.as_str()) {
        category
    } else {
        "context".into()
    }
}

fn clean_content(content: &str) -> Result<String> {
    let content = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if content.is_empty() {
        return Err(Error::msg("A memory needs some content."));
    }
    if content.chars().count() > MAX_CHARS {
        return Err(Error::msg(format!(
            "Keep a memory under {MAX_CHARS} characters: one fact, in a sentence or two."
        )));
    }
    Ok(content)
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get(0)?,
        content: r.get(1)?,
        category: r.get(2)?,
        source: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

const COLUMNS: &str = "id, content, category, source, created_at, updated_at";

pub fn list(conn: &Connection) -> Result<Vec<Memory>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM memories ORDER BY updated_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Memory> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM memories WHERE id = ?1"), [id], row)
        .optional()?
        .ok_or_else(|| Error::msg(format!("There is no memory with id {id}.")))
}

/// Saves a memory. The same fact said again only refreshes the existing one.
pub fn add(conn: &Connection, content: &str, category: &str, source: &str) -> Result<Memory> {
    let content = clean_content(content)?;
    let category = clean_category(category);
    let now = db::now_secs();
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM memories WHERE lower(content) = lower(?1)",
            [&content],
            |r| r.get(0),
        )
        .optional()?;
    let id = match existing {
        Some(id) => {
            conn.execute(
                "UPDATE memories SET category = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, category, now],
            )?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO memories (content, category, source, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![content, category, source, now],
            )?;
            conn.last_insert_rowid()
        }
    };
    get(conn, id)
}

pub fn update(
    conn: &Connection,
    id: i64,
    content: Option<&str>,
    category: Option<&str>,
) -> Result<Memory> {
    let current = get(conn, id)?;
    let content = match content {
        Some(text) => clean_content(text)?,
        None => current.content,
    };
    let category = category.map(clean_category).unwrap_or(current.category);
    conn.execute(
        "UPDATE memories SET content = ?2, category = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, content, category, db::now_secs()],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<Memory> {
    let memory = get(conn, id)?;
    conn.execute("DELETE FROM memories WHERE id = ?1", [id])?;
    Ok(memory)
}

pub fn clear(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM memories", [])?;
    Ok(())
}

/// The memory section of an agent's system prompt.
pub fn prompt_section(conn: &Connection) -> Result<String> {
    if !enabled(conn) {
        return Ok(String::new());
    }
    let memories = list(conn)?;
    let total = memories.len() as i64;
    let mut out = String::from(
        "\n\nWhat you remember about the learner (long-term memory, shared with the app's other \
         agents; ids in brackets):\n",
    );
    if memories.is_empty() {
        out.push_str("  (nothing yet)\n");
    }
    for category in CATEGORIES {
        let lines: Vec<String> = memories
            .iter()
            .filter(|m| m.category == category)
            .take(PROMPT_LIMIT)
            .map(|m| format!("  - [{}] {}", m.id, m.content))
            .collect();
        if !lines.is_empty() {
            out.push_str(&format!("{}:\n{}\n", heading(category), lines.join("\n")));
        }
    }
    out.push_str(MEMORY_RULES);
    if total > SOFT_LIMIT {
        out.push_str(&format!(
            "\nThere are {total} memories: merge overlapping ones with update_memory and forget \
             stale ones before adding more."
        ));
    }
    Ok(out)
}

fn heading(category: &str) -> &'static str {
    match category {
        "goal" => "Goals",
        "preference" => "Preferences",
        "strength" => "Strengths",
        "struggle" => "Struggles",
        _ => "Context",
    }
}

const MEMORY_RULES: &str = "\
Using memory:
- Use what you remember: tailor examples, vocabulary and suggestions to it, without reciting it back.
- Save a memory with `remember` when you learn something that will still matter in later \
  conversations: a goal (\"wants to order food in Kraków in May\"), a preference (\"likes \
  short drills, hates long word lists\"), context (\"partner's family speaks only Polish\"), a \
  strength, or a struggle (\"mixes up the instrumental and locative after 'z'\"). One fact per \
  memory, in a short third-person sentence. Don't save small talk, one-off requests, what the \
  app already records (scores, streaks, blocks), or anything sensitive the learner did not \
  offer to have remembered.
- Keep memories true: when one changes or is no longer true (a struggle is overcome, a goal is \
  reached), call update_memory or forget_memory instead of adding a contradicting one.
- If the learner asks you to remember or forget something, do it.";

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

fn function(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}

/// The memory tools, offered to every agent while memory is switched on.
pub fn tool_definitions() -> Vec<Value> {
    vec![
        function(
            "remember",
            "Save one lasting fact about the learner to long-term memory, shared with the app's other agents.",
            json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "One fact, in a short third-person sentence" },
                    "category": { "type": "string", "enum": CATEGORIES }
                },
                "required": ["content", "category"]
            }),
        ),
        function(
            "update_memory",
            "Correct or refresh a memory that has changed, e.g. a struggle that is now a strength.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "content": { "type": "string" },
                    "category": { "type": "string", "enum": CATEGORIES }
                },
                "required": ["id"]
            }),
        ),
        function(
            "forget_memory",
            "Delete a memory that is no longer true or that the learner wants forgotten.",
            json!({
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }),
        ),
    ]
}

pub fn is_tool(name: &str) -> bool {
    matches!(name, "remember" | "update_memory" | "forget_memory")
}

#[derive(Deserialize)]
struct RememberArgs {
    content: String,
    #[serde(default)]
    category: String,
}

#[derive(Deserialize)]
struct UpdateArgs {
    id: i64,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct IdArgs {
    id: i64,
}

/// What a memory tool did: the result for the model, and a line for the
/// learner (who should always be able to see what was remembered).
pub struct ToolOutcome {
    pub result: Value,
    pub label: Option<String>,
}

/// Runs a memory tool for `agent`. Errors go back to the model as text.
pub fn run_tool(conn: &Connection, agent: &str, name: &str, arguments: &str) -> ToolOutcome {
    match try_run(conn, agent, name, arguments) {
        Ok(outcome) => outcome,
        Err(error) => ToolOutcome { result: json!({ "error": error.to_string() }), label: None },
    }
}

fn try_run(conn: &Connection, agent: &str, name: &str, arguments: &str) -> Result<ToolOutcome> {
    if !enabled(conn) {
        return Err(Error::msg("The learner has switched memory off; nothing was saved."));
    }
    let parse = |e: serde_json::Error| Error::msg(format!("The tool arguments were not valid: {e}"));
    match name {
        "remember" => {
            let args: RememberArgs = serde_json::from_str(arguments).map_err(parse)?;
            let memory = add(conn, &args.content, &args.category, agent)?;
            Ok(ToolOutcome {
                label: Some(format!("Remembered: {}", memory.content)),
                result: json!({ "saved": memory }),
            })
        }
        "update_memory" => {
            let args: UpdateArgs = serde_json::from_str(arguments).map_err(parse)?;
            let memory = update(conn, args.id, args.content.as_deref(), args.category.as_deref())?;
            Ok(ToolOutcome {
                label: Some(format!("Updated memory: {}", memory.content)),
                result: json!({ "updated": memory }),
            })
        }
        "forget_memory" => {
            let args: IdArgs = serde_json::from_str(arguments).map_err(parse)?;
            let memory = delete(conn, args.id)?;
            Ok(ToolOutcome {
                label: Some(format!("Forgot: {}", memory.content)),
                result: json!({ "forgotten": memory.id }),
            })
        }
        other => Err(Error::msg(format!("Unknown memory tool {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> Connection {
        let path = std::env::temp_dir()
            .join("learn-polish-tests")
            .join(format!("memories-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        db::open(&path).unwrap()
    }

    #[test]
    fn agents_save_revise_and_forget_memories() {
        let conn = scratch();
        let out = run_tool(
            &conn,
            "tutor",
            "remember",
            r#"{"content":"  Wants to order food in Kraków in May ","category":"goal"}"#,
        );
        assert_eq!(out.label.as_deref(), Some("Remembered: Wants to order food in Kraków in May"));
        let id = out.result["saved"]["id"].as_i64().unwrap();

        // Saying it again refreshes rather than duplicating.
        run_tool(&conn, "examiner", "remember", r#"{"content":"wants to order food in Kraków in May","category":"goal"}"#);
        assert_eq!(list(&conn).unwrap().len(), 1);

        let struggle = add(&conn, "Mixes up instrumental and locative after z", "struggle", "examiner").unwrap();
        assert_eq!(add(&conn, "x", "nonsense", "tutor").unwrap().category, "context");

        let prompt = prompt_section(&conn).unwrap();
        assert!(prompt.contains(&format!("[{id}] Wants to order food")), "{prompt}");
        assert!(prompt.contains("Struggles:"));

        let updated = run_tool(
            &conn,
            "examiner",
            "update_memory",
            &format!(r#"{{"id":{},"content":"Now uses the instrumental after z correctly","category":"strength"}}"#, struggle.id),
        );
        assert_eq!(updated.result["updated"]["category"], "strength");

        run_tool(&conn, "tutor", "forget_memory", &format!(r#"{{"id":{id}}}"#));
        assert!(get(&conn, id).is_err());
        let missing = run_tool(&conn, "tutor", "forget_memory", r#"{"id":9999}"#);
        assert!(missing.result["error"].is_string());

        // Switched off: no prompt section, and nothing is saved.
        db::set_setting(&conn, "memories_enabled", "0").unwrap();
        assert!(prompt_section(&conn).unwrap().is_empty());
        let off = run_tool(&conn, "tutor", "remember", r#"{"content":"y","category":"goal"}"#);
        assert!(off.result["error"].is_string() && off.label.is_none());
    }
}

// ---------------------------------------------------------------------------
// Commands: the learner's view of the memory
// ---------------------------------------------------------------------------

pub mod commands {
    use serde::Serialize;
    use tauri::State;

    use super::*;
    use crate::db::Db;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct MemoryOverview {
        pub enabled: bool,
        pub memories: Vec<Memory>,
    }

    fn overview(conn: &Connection) -> Result<MemoryOverview> {
        Ok(MemoryOverview { enabled: enabled(conn), memories: list(conn)? })
    }

    #[tauri::command]
    pub async fn memories_overview(db: State<'_, Db>) -> Result<MemoryOverview> {
        overview(&db.lock())
    }

    #[tauri::command]
    pub async fn memories_set_enabled(db: State<'_, Db>, enabled: bool) -> Result<MemoryOverview> {
        let conn = db.lock();
        db::set_setting(&conn, "memories_enabled", if enabled { "1" } else { "0" })?;
        overview(&conn)
    }

    /// The learner tells the agents something directly.
    #[tauri::command]
    pub async fn memory_add(db: State<'_, Db>, content: String, category: String) -> Result<MemoryOverview> {
        let conn = db.lock();
        add(&conn, &content, &category, "you")?;
        overview(&conn)
    }

    #[tauri::command]
    pub async fn memory_update(
        db: State<'_, Db>,
        id: i64,
        content: String,
        category: String,
    ) -> Result<MemoryOverview> {
        let conn = db.lock();
        update(&conn, id, Some(&content), Some(&category))?;
        overview(&conn)
    }

    #[tauri::command]
    pub async fn memory_delete(db: State<'_, Db>, id: i64) -> Result<MemoryOverview> {
        let conn = db.lock();
        delete(&conn, id)?;
        overview(&conn)
    }

    #[tauri::command]
    pub async fn memories_clear(db: State<'_, Db>) -> Result<MemoryOverview> {
        let conn = db.lock();
        clear(&conn)?;
        overview(&conn)
    }
}
