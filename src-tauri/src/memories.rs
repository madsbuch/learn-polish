//! Long-term memory shared by the agents (the tutor and the examiner).
//!
//! A memory is one short fact about the learner — a goal, a preference, a
//! strength, something they keep getting wrong, or context about their life
//! that shapes what is worth learning. The agents save and revise memories
//! with tools, and every agent sees the current set in its prompt, so what
//! the examiner notices in a check-in reaches the tutor and the other way
//! round. The learner can read, add and delete them in Settings → Memories.

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::{tool, Tool};

use crate::db::{self, Conn};
use crate::schema::memories;
use crate::error::{Error, Result};

pub const CATEGORIES: [&str; 5] = ["goal", "preference", "context", "strength", "struggle"];

/// How many memories go into a prompt: the most recently touched first.
const PROMPT_LIMIT: usize = 80;
/// A memory is a sentence or two, not an essay.
const MAX_CHARS: usize = 400;
/// Above this, the agents are told to merge or forget before adding more.
const SOFT_LIMIT: i64 = 150;

#[derive(Debug, Clone, Serialize, PartialEq, Queryable, Selectable)]
#[diesel(table_name = memories)]
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

pub fn enabled(conn: &mut Conn) -> bool {
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
    let content = crate::text::clean(content);
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

pub fn list(conn: &mut Conn) -> Result<Vec<Memory>> {
    Ok(memories::table
        .order((memories::updated_at.desc(), memories::id.desc()))
        .select(Memory::as_select())
        .load(conn)?)
}

pub fn get(conn: &mut Conn, id: i64) -> Result<Memory> {
    memories::table
        .find(id)
        .select(Memory::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg(format!("There is no memory with id {id}.")))
}

/// Saves a memory. The same fact said again only refreshes the existing one.
pub fn add(conn: &mut Conn, content: &str, category: &str, source: &str) -> Result<Memory> {
    let content = clean_content(content)?;
    let category = clean_category(category);
    let now = db::now_secs();
    let lowered = content.to_lowercase();
    let existing = memories::table
        .select((memories::id, memories::content))
        .load::<(i64, String)>(conn)?
        .into_iter()
        .find(|(_, text)| text.to_lowercase() == lowered)
        .map(|(id, _)| id);
    let id = match existing {
        Some(id) => {
            diesel::update(memories::table.find(id))
                .set((memories::category.eq(&category), memories::updated_at.eq(now)))
                .execute(conn)?;
            id
        }
        None => diesel::insert_into(memories::table)
            .values((
                memories::content.eq(&content),
                memories::category.eq(&category),
                memories::source.eq(source),
                memories::created_at.eq(now),
                memories::updated_at.eq(now),
            ))
            .returning(memories::id)
            .get_result(conn)?,
    };
    get(conn, id)
}

pub fn update(
    conn: &mut Conn,
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
    diesel::update(memories::table.find(id))
        .set((
            memories::content.eq(content),
            memories::category.eq(category),
            memories::updated_at.eq(db::now_secs()),
        ))
        .execute(conn)?;
    get(conn, id)
}

pub fn delete(conn: &mut Conn, id: i64) -> Result<Memory> {
    let memory = get(conn, id)?;
    diesel::delete(memories::table.find(id)).execute(conn)?;
    Ok(memory)
}

pub fn clear(conn: &mut Conn) -> Result<()> {
    diesel::delete(memories::table).execute(conn)?;
    Ok(())
}

/// The memory section of an agent's system prompt.
pub fn prompt_section(conn: &mut Conn) -> Result<String> {
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

/// The memory tools, offered to every agent while memory is switched on.
pub fn tools() -> Vec<Tool> {
    vec![
        tool(
            "remember",
            "Remembering…",
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
        tool(
            "update_memory",
            "Remembering…",
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
        tool(
            "forget_memory",
            "Remembering…",
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
pub fn run_tool(conn: &mut Conn, agent: &str, name: &str, arguments: &str) -> ToolOutcome {
    match try_run(conn, agent, name, arguments) {
        Ok(outcome) => outcome,
        Err(error) => ToolOutcome { result: json!({ "error": error.to_string() }), label: None },
    }
}

fn try_run(conn: &mut Conn, agent: &str, name: &str, arguments: &str) -> Result<ToolOutcome> {
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

    fn scratch() -> Conn {
        let path = std::env::temp_dir()
            .join("learn-polish-tests")
            .join(format!("memories-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        db::open(&path).unwrap()
    }

    #[test]
    fn agents_save_revise_and_forget_memories() {
        let conn = &mut scratch();
        let out = run_tool(
            conn,
            "tutor",
            "remember",
            r#"{"content":"  Wants to order food in Kraków in May ","category":"goal"}"#,
        );
        assert_eq!(out.label.as_deref(), Some("Remembered: Wants to order food in Kraków in May"));
        let id = out.result["saved"]["id"].as_i64().unwrap();

        // Saying it again refreshes rather than duplicating.
        run_tool(conn, "examiner", "remember", r#"{"content":"wants to order food in Kraków in May","category":"goal"}"#);
        assert_eq!(list(conn).unwrap().len(), 1);

        let struggle = add(conn, "Mixes up instrumental and locative after z", "struggle", "examiner").unwrap();
        assert_eq!(add(conn, "x", "nonsense", "tutor").unwrap().category, "context");

        let prompt = prompt_section(conn).unwrap();
        assert!(prompt.contains(&format!("[{id}] Wants to order food")), "{prompt}");
        assert!(prompt.contains("Struggles:"));

        let updated = run_tool(
            conn,
            "examiner",
            "update_memory",
            &format!(r#"{{"id":{},"content":"Now uses the instrumental after z correctly","category":"strength"}}"#, struggle.id),
        );
        assert_eq!(updated.result["updated"]["category"], "strength");

        run_tool(conn, "tutor", "forget_memory", &format!(r#"{{"id":{id}}}"#));
        assert!(get(conn, id).is_err());
        let missing = run_tool(conn, "tutor", "forget_memory", r#"{"id":9999}"#);
        assert!(missing.result["error"].is_string());

        // Switched off: no prompt section, and nothing is saved.
        db::set_setting(conn, "memories_enabled", "0").unwrap();
        assert!(prompt_section(conn).unwrap().is_empty());
        let off = run_tool(conn, "tutor", "remember", r#"{"content":"y","category":"goal"}"#);
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

    fn overview(conn: &mut Conn) -> Result<MemoryOverview> {
        Ok(MemoryOverview { enabled: enabled(conn), memories: list(conn)? })
    }

    #[tauri::command]
    pub async fn memories_overview(db: State<'_, Db>) -> Result<MemoryOverview> {
        overview(&mut db.lock())
    }

    #[tauri::command]
    pub async fn memories_set_enabled(db: State<'_, Db>, enabled: bool) -> Result<MemoryOverview> {
        let mut guard = db.lock();
        let conn = &mut *guard;
        db::set_setting(conn, "memories_enabled", if enabled { "1" } else { "0" })?;
        overview(conn)
    }

    /// The learner tells the agents something directly.
    #[tauri::command]
    pub async fn memory_add(db: State<'_, Db>, content: String, category: String) -> Result<MemoryOverview> {
        let mut guard = db.lock();
        let conn = &mut *guard;
        add(conn, &content, &category, "you")?;
        overview(conn)
    }

    #[tauri::command]
    pub async fn memory_update(
        db: State<'_, Db>,
        id: i64,
        content: String,
        category: String,
    ) -> Result<MemoryOverview> {
        let mut guard = db.lock();
        let conn = &mut *guard;
        update(conn, id, Some(&content), Some(&category))?;
        overview(conn)
    }

    #[tauri::command]
    pub async fn memory_delete(db: State<'_, Db>, id: i64) -> Result<MemoryOverview> {
        let mut guard = db.lock();
        let conn = &mut *guard;
        delete(conn, id)?;
        overview(conn)
    }

    #[tauri::command]
    pub async fn memories_clear(db: State<'_, Db>) -> Result<MemoryOverview> {
        let mut guard = db.lock();
        let conn = &mut *guard;
        clear(conn)?;
        overview(conn)
    }
}
