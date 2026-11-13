//! The tutor: an assistant for the whole app, not a Polish role-play partner.
//!
//! It talks to the learner in English and can act in the app through tools —
//! build a practice programme on any theme, look up progress and vocabulary,
//! point the daily check-in at a programme, adjust practice settings. Every
//! tool runs here, in Rust, against the same SQLite database the rest of the
//! app uses, so what the tutor makes is immediately practisable.

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{Emitter, State};

use crate::ai::{self, Wire};
use crate::db::{self, Db};
use crate::error::{Error, Result};
use crate::models::{ChatMessage, TutorAction, TutorStatus};
use crate::programmes::{self, NewLine, NewProgramme, NewSentence, NewWord};
use crate::session;
use crate::songs;
use crate::stats;

/// How much of the conversation is replayed to the model.
const HISTORY_LIMIT: i64 = 30;
/// Tool rounds per learner message. Creating a programme takes one or two;
/// the cap only stops a confused model from looping.
const MAX_ROUNDS: usize = 8;

// ---------------------------------------------------------------------------
// Prompt
// ---------------------------------------------------------------------------

const RULES: &str = "\
You are the assistant built into Polski, an Android app in which an English speaker learns Polish. \
You help the learner use the app and learn: you answer questions about Polish, explain grammar, \
suggest what to practise, and you change things in the app with your tools.

How to talk:
- Write in English. Use Polish for examples, always with an English translation.
- Only hold a conversation in Polish if the learner explicitly asks to practise speaking with you; \
  even then, add English translations in brackets and drop back to English when they ask \
  something about the app.
- Be brief and friendly. Markdown is rendered (bold, lists, short headings).

What the app does:
- A daily check-in: read a short dialogue, meet new words, review due words (spaced repetition), \
  build sentences, fill gaps in the dialogue. By default it walks through ten built-in course \
  topics in order.
- A session mixes these formats on its own: reading the dialogue (when there is one), new-word \
  cards, multiple choice both ways, typing the Polish, building a sentence from shuffled words, \
  memory cards (turn cards over to pair each Polish word with its meaning), fill-the-gaps \
  (sentences with a word missing, from a word bank), and gaps in the dialogue. The learner \
  cannot pick formats per programme yet; if they ask for one (e.g. only memory cards), say so \
  plainly, and point out that every session of a programme includes it. A programme made \
  without a dialogue simply skips the dialogue parts.
- Programmes: practice topics made through you, on any theme (colours, numbers, verbs of motion, \
  words for a trip…). A programme is practised exactly like a course topic — the learner can \
  start it from the button under your reply or from the Exercises tab, and it can be made the daily \
  focus so the check-in uses it until all its words have been met.

Using tools:
- When the learner asks for a programme, course, lesson, list or practice set on a theme, \
  build it with create_programme right away — do not ask for permission first, and never say \
  you cannot do it. Unless they ask for a different size, include 15–30 words, 8–12 short \
  sentences that use those words, and a natural dialogue of 8–12 lines in which those words occur.
- Words: the Polish dictionary form (nouns in the nominative singular, verbs in the infinitive, \
  adjectives in the masculine form), lowercase unless a proper noun; a short English gloss \
  (alternatives separated by commas); a part-of-speech code. Sentences: short (max ~8 words), \
  natural, beginner-friendly Polish. Everything must be correct Polish with correct diacritics.
- Only set the programme as the daily focus if the learner asks for that (e.g. \"make it my \
  daily practice\"). Otherwise the reply already offers buttons to practise it or focus on it.
- Call get_progress before commenting on how the learner is doing; never invent numbers.
- delete_programme and delete_songs are permanent: only call them after the learner has \
  clearly confirmed (name what will be deleted and ask first).
- Songs: the Music tab has learning songs — English lines answered by Polish ones, and a \
  Polish chorus sentence that gets stuck in your head. When the learner asks for a song, call \
  make_song with what it should be about (and a topic id, chorus sentence or style if they \
  said or it fits). The app's songwriter writes the lyrics and Suno sings two takes in 2–4 \
  minutes; it costs the learner kie.ai credits, so make one song per request unless they ask \
  for more. Use list_songs to see what exists, e.g. before suggesting what to delete.
- The built-in course topics cannot be edited; make a programme instead.
- After using a tool, say in one or two sentences what you did. Don't paste the whole word list \
  back unless asked.";

fn system_prompt(conn: &Connection) -> Result<String> {
    let today = session::topic_for_day(conn)?;
    let focus = session::focus_topic_id(conn);
    let stats = stats::collect(conn)?;

    let mut stmt = conn.prepare(
        "SELECT id, title_en, title_pl,
                (SELECT COUNT(*) FROM words w WHERE w.topic_id = t.id)
         FROM topics t WHERE kind = 'programme' ORDER BY created_at DESC LIMIT 40",
    )?;
    let programmes: Vec<String> = stmt
        .query_map([], |r| {
            Ok(format!(
                "  - id {}: {} ({}), {} words",
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;

    Ok(format!(
        "{RULES}\n\nThe learner right now:\n\
         - Today's check-in: {today_title} (topic id {today_id}){focus_note}{done}\n\
         - Level {level}, {xp} XP, streak {streak} days, {seen} of {total} words met, {due} items due.\n\
         - Songs: {song_count} in the Music tab{song_note}.\n\
         - Their programmes:\n{programmes}",
        today_title = today.title,
        today_id = today.id,
        focus_note = if focus.is_some() { ", chosen as the daily focus" } else { ", from the course" },
        done = if stats.completed_today { "; already done today" } else { "" },
        level = stats.level,
        xp = stats.total_xp,
        streak = stats.current_streak,
        seen = stats.words_seen,
        total = stats.words_total,
        due = stats.due_today,
        song_count = conn.query_row("SELECT COUNT(*) FROM songs", [], |r| r.get::<_, i64>(0))?,
        song_note = if songs::kie_key(conn).is_ok() {
            ""
        } else {
            "; making songs needs a kie.ai key in Settings, which is missing"
        },
        programmes = if programmes.is_empty() {
            "  (none yet)".to_string()
        } else {
            programmes.join("\n")
        },
    ))
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

fn word_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "pl": { "type": "string", "description": "Polish dictionary form" },
                "en": { "type": "string", "description": "English gloss; alternatives separated by commas" },
                "pos": {
                    "type": "string",
                    "enum": ["n-m", "n-f", "n-n", "n-pl", "v", "adj", "adv", "num", "pron", "prep", "conj", "part", "interj", "phr"],
                    "description": "Part of speech. Nouns carry their gender: n-m masculine, n-f feminine, n-n neuter, n-pl plural-only."
                }
            },
            "required": ["pl", "en", "pos"]
        }
    })
}

fn sentence_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "pl": { "type": "string" },
                "en": { "type": "string" }
            },
            "required": ["pl", "en"]
        }
    })
}

fn dialogue_schema() -> Value {
    json!({
        "type": "array",
        "description": "A short dialogue, one entry per line, alternating speakers.",
        "items": {
            "type": "object",
            "properties": {
                "speaker": { "type": "string", "description": "A Polish first name" },
                "gender": { "type": "string", "enum": ["f", "m"], "description": "Decides the voice that reads the line" },
                "pl": { "type": "string" },
                "en": { "type": "string" }
            },
            "required": ["speaker", "gender", "pl", "en"]
        }
    })
}

fn function(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}

pub fn tool_definitions() -> Value {
    let none = json!({ "type": "object", "properties": {} });
    Value::Array(vec![
        function(
            "create_programme",
            "Create a new practice programme on a theme, with words, sentences and optionally a dialogue. \
             It can be practised straight away.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short English title, e.g. \"Colours\"" },
                    "title_pl": { "type": "string", "description": "The title in Polish, e.g. \"Kolory\"" },
                    "summary": { "type": "string", "description": "One English sentence on what the programme covers" },
                    "words": word_schema(),
                    "sentences": sentence_schema(),
                    "dialogue": dialogue_schema(),
                    "set_as_daily_focus": { "type": "boolean", "description": "Only true if the learner asked for it to become their daily practice" }
                },
                "required": ["title", "title_pl", "summary", "words", "sentences"]
            }),
        ),
        function(
            "add_to_programme",
            "Add words, sentences or dialogue lines to one of the learner's programmes.",
            json!({
                "type": "object",
                "properties": {
                    "topic_id": { "type": "integer" },
                    "words": word_schema(),
                    "sentences": sentence_schema(),
                    "dialogue": dialogue_schema()
                },
                "required": ["topic_id"]
            }),
        ),
        function(
            "edit_programme",
            "Rename a programme, change its summary, or remove words/sentences from it (by their exact Polish text).",
            json!({
                "type": "object",
                "properties": {
                    "topic_id": { "type": "integer" },
                    "title": { "type": "string" },
                    "title_pl": { "type": "string" },
                    "summary": { "type": "string" },
                    "remove_words": { "type": "array", "items": { "type": "string" } },
                    "remove_sentences": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["topic_id"]
            }),
        ),
        function(
            "delete_programme",
            "Permanently delete one of the learner's programmes and its practice history. Only after the learner confirmed.",
            json!({
                "type": "object",
                "properties": { "topic_id": { "type": "integer" } },
                "required": ["topic_id"]
            }),
        ),
        function(
            "list_topics",
            "List every course topic and programme, with how much of each the learner has met.",
            none.clone(),
        ),
        function(
            "get_topic",
            "The words, sentences and dialogue of one topic or programme, with the learner's progress on each word.",
            json!({
                "type": "object",
                "properties": { "topic_id": { "type": "integer" } },
                "required": ["topic_id"]
            }),
        ),
        function(
            "search_vocabulary",
            "Search every word and sentence in the app, in Polish or English.",
            json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"]
            }),
        ),
        function(
            "get_progress",
            "The learner's statistics: XP, level, streak, accuracy, words met and mastered, recent days, and the words they get wrong most.",
            none,
        ),
        function(
            "set_daily_focus",
            "Make the daily check-in use a topic or programme until all its words have been met, or pass null to go back to the course rotation.",
            json!({
                "type": "object",
                "properties": { "topic_id": { "type": ["integer", "null"] } },
                "required": ["topic_id"]
            }),
        ),
        function(
            "update_practice_settings",
            "Change the daily XP goal (10–1000) or how many new words a session introduces (3–40).",
            json!({
                "type": "object",
                "properties": {
                    "daily_goal": { "type": "integer" },
                    "new_words_per_day": { "type": "integer" }
                }
            }),
        ),
        function(
            "make_song",
            "Have a learning song written and sung (takes 2–4 minutes, costs kie.ai credits). \
             The songwriter writes the lyrics from this brief.",
            json!({
                "type": "object",
                "properties": {
                    "about": { "type": "string", "description": "What the song is about, in English" },
                    "topic_id": { "type": "integer", "description": "Take vocabulary from this topic or programme" },
                    "chorus_pl": { "type": "string", "description": "An exact Polish chorus sentence, if the learner wants one" },
                    "style": { "type": "string", "description": "Musical style, e.g. \"upbeat indie folk, male vocals\"" },
                    "vocal_gender": { "type": "string", "enum": ["f", "m", ""] }
                },
                "required": ["about"]
            }),
        ),
        function(
            "list_songs",
            "The learner's songs: title, chorus, status, takes and how often each was played.",
            json!({ "type": "object", "properties": {} }),
        ),
        function(
            "delete_songs",
            "Permanently delete songs (all their takes). Only after the learner confirmed.",
            json!({
                "type": "object",
                "properties": { "song_ids": { "type": "array", "items": { "type": "integer" } } },
                "required": ["song_ids"]
            }),
        ),
        function(
            "suggest_practice",
            "Show the learner a button that starts a practice session for a topic or programme right now.",
            json!({
                "type": "object",
                "properties": { "topic_id": { "type": "integer" } },
                "required": ["topic_id"]
            }),
        ),
    ])
}

/// What a tool produced: JSON for the model, and, when the app changed, an
/// action for the learner to see.
struct Outcome {
    result: Value,
    action: Option<TutorAction>,
}

impl Outcome {
    fn data(result: Value) -> Self {
        Outcome { result, action: None }
    }
    fn acted(result: Value, kind: &str, label: String, topic_id: Option<i64>) -> Self {
        Outcome {
            result,
            action: Some(TutorAction { kind: kind.into(), label, topic_id, song_id: None }),
        }
    }
    fn song(result: Value, kind: &str, label: String, song_id: Option<i64>) -> Self {
        Outcome {
            result,
            action: Some(TutorAction { kind: kind.into(), label, topic_id: None, song_id }),
        }
    }
}

/// A short "working on it" line for the UI while a tool runs.
fn status_label(name: &str) -> &'static str {
    match name {
        "create_programme" => "Building the programme…",
        "add_to_programme" | "edit_programme" => "Updating the programme…",
        "delete_programme" => "Deleting the programme…",
        "list_topics" | "get_topic" => "Looking at your topics…",
        "search_vocabulary" => "Searching the vocabulary…",
        "get_progress" => "Checking your progress…",
        "set_daily_focus" => "Changing your daily check-in…",
        "update_practice_settings" => "Updating your settings…",
        "make_song" => "Writing the song…",
        "list_songs" => "Looking at your songs…",
        "delete_songs" => "Deleting songs…",
        _ => "Working…",
    }
}

#[derive(Deserialize)]
struct TopicArg {
    topic_id: i64,
}

#[derive(Deserialize)]
struct CreateArgs {
    #[serde(flatten)]
    programme: NewProgramme,
    #[serde(default)]
    set_as_daily_focus: bool,
}

#[derive(Deserialize)]
struct ExtendArgs {
    topic_id: i64,
    #[serde(default)]
    words: Vec<NewWord>,
    #[serde(default)]
    sentences: Vec<NewSentence>,
    #[serde(default)]
    dialogue: Vec<NewLine>,
}

#[derive(Deserialize)]
struct EditArgs {
    topic_id: i64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    title_pl: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    remove_words: Vec<String>,
    #[serde(default)]
    remove_sentences: Vec<String>,
}

#[derive(Deserialize)]
struct FocusArgs {
    #[serde(default)]
    topic_id: Option<i64>,
}

#[derive(Deserialize)]
struct SettingsArgs {
    #[serde(default)]
    daily_goal: Option<i64>,
    #[serde(default)]
    new_words_per_day: Option<i64>,
}

#[derive(Deserialize)]
struct SongArgs {
    about: String,
    #[serde(default)]
    topic_id: Option<i64>,
    #[serde(default)]
    chorus_pl: String,
    #[serde(default)]
    style: String,
    #[serde(default)]
    vocal_gender: String,
}

#[derive(Deserialize)]
struct DeleteSongsArgs {
    song_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
}

fn parse<T: serde::de::DeserializeOwned>(arguments: &str) -> Result<T> {
    serde_json::from_str(arguments)
        .map_err(|e| Error::msg(format!("The tool arguments were not valid: {e}")))
}

/// Runs one tool call. Errors are returned to the model as text rather than
/// failing the turn, so it can correct itself or explain.
fn execute(conn: &Connection, name: &str, arguments: &str) -> Outcome {
    match run_tool(conn, name, arguments) {
        Ok(outcome) => outcome,
        Err(error) => Outcome::data(json!({ "error": error.to_string() })),
    }
}

fn run_tool(conn: &Connection, name: &str, arguments: &str) -> Result<Outcome> {
    match name {
        "create_programme" => {
            let args: CreateArgs = parse(arguments)?;
            let created = programmes::create(conn, args.programme)?;
            let mut label = format!(
                "Created “{}” — {} words, {} sentences",
                created.title, created.words, created.sentences
            );
            if created.dialogue_lines > 0 {
                label.push_str(", a dialogue");
            }
            if args.set_as_daily_focus {
                programmes::set_focus(conn, Some(created.topic_id))?;
                label.push_str(" · now your daily focus");
            }
            Ok(Outcome::acted(
                json!({ "created": created, "daily_focus": args.set_as_daily_focus }),
                "programmeCreated",
                label,
                Some(created.topic_id),
            ))
        }
        "add_to_programme" => {
            let args: ExtendArgs = parse(arguments)?;
            let updated =
                programmes::extend(conn, args.topic_id, &args.words, &args.sentences, &args.dialogue)?;
            Ok(Outcome::acted(
                json!({ "updated": updated }),
                "programmeUpdated",
                format!(
                    "Updated “{}” — now {} words, {} sentences",
                    updated.title, updated.words, updated.sentences
                ),
                Some(updated.topic_id),
            ))
        }
        "edit_programme" => {
            let args: EditArgs = parse(arguments)?;
            programmes::rename(
                conn,
                args.topic_id,
                args.title.as_deref(),
                args.title_pl.as_deref(),
                args.summary.as_deref(),
            )?;
            let updated =
                programmes::remove_items(conn, args.topic_id, &args.remove_words, &args.remove_sentences)?;
            Ok(Outcome::acted(
                json!({ "updated": updated }),
                "programmeUpdated",
                format!("Edited “{}”", updated.title),
                Some(updated.topic_id),
            ))
        }
        "delete_programme" => {
            let args: TopicArg = parse(arguments)?;
            let title = programmes::delete(conn, args.topic_id)?;
            Ok(Outcome::acted(
                json!({ "deleted": title }),
                "programmeDeleted",
                format!("Deleted “{title}”"),
                None,
            ))
        }
        "list_topics" => Ok(Outcome::data(list_topics(conn)?)),
        "get_topic" => {
            let args: TopicArg = parse(arguments)?;
            Ok(Outcome::data(topic_detail(conn, args.topic_id)?))
        }
        "search_vocabulary" => {
            let args: SearchArgs = parse(arguments)?;
            Ok(Outcome::data(search(conn, &args.query)?))
        }
        "get_progress" => Ok(Outcome::data(progress(conn)?)),
        "set_daily_focus" => {
            let args: FocusArgs = parse(arguments)?;
            match programmes::set_focus(conn, args.topic_id)? {
                Some(title) => Ok(Outcome::acted(
                    json!({ "daily_focus": title }),
                    "focusSet",
                    format!("Daily check-in now focuses on “{title}”"),
                    args.topic_id,
                )),
                None => Ok(Outcome::acted(
                    json!({ "daily_focus": null }),
                    "focusCleared",
                    "Daily check-in is back on the course".into(),
                    None,
                )),
            }
        }
        "update_practice_settings" => {
            let args: SettingsArgs = parse(arguments)?;
            let mut changed = Vec::new();
            if let Some(goal) = args.daily_goal {
                let goal = goal.clamp(10, 1000);
                db::set_setting(conn, "daily_goal", &goal.to_string())?;
                changed.push(format!("daily goal {goal} XP"));
            }
            if let Some(count) = args.new_words_per_day {
                let count = count.clamp(3, 40);
                db::set_setting(conn, "new_words_per_day", &count.to_string())?;
                changed.push(format!("{count} new words per session"));
            }
            if changed.is_empty() {
                return Err(Error::msg("Nothing to change."));
            }
            Ok(Outcome::acted(
                json!({ "changed": changed }),
                "settingsChanged",
                format!("Settings: {}", changed.join(", ")),
                None,
            ))
        }
        "list_songs" => {
            let list: Vec<Value> = songs::list_songs(conn)?
                .into_iter()
                .map(|song| {
                    json!({
                        "song_id": song.id,
                        "title": song.title,
                        "chorus_pl": song.chorus_pl,
                        "chorus_en": song.chorus_en,
                        "status": song.status,
                        "takes": song.tracks.len(),
                        "plays": song.tracks.iter().map(|t| t.play_count).sum::<i64>(),
                    })
                })
                .collect();
            Ok(Outcome::data(Value::Array(list)))
        }
        "delete_songs" => {
            let args: DeleteSongsArgs = parse(arguments)?;
            let mut titles = Vec::new();
            for id in args.song_ids {
                if let Ok(song) = songs::load_song(conn, id) {
                    songs::delete_song(conn, id)?;
                    titles.push(song.title);
                }
            }
            if titles.is_empty() {
                return Err(Error::msg("None of those songs exist."));
            }
            Ok(Outcome::song(
                json!({ "deleted": titles }),
                "songsDeleted",
                format!("Deleted {}", titles.iter().map(|t| format!("“{t}”")).collect::<Vec<_>>().join(", ")),
                None,
            ))
        }
        "suggest_practice" => {
            let args: TopicArg = parse(arguments)?;
            let topic = session::load_topic(conn, args.topic_id)?;
            Ok(Outcome::acted(
                json!({ "shown_button_for": topic.title }),
                "practiceSuggested",
                format!("Practise “{}”", topic.title),
                Some(topic.id),
            ))
        }
        other => Err(Error::msg(format!("There is no tool called '{other}'."))),
    }
}

fn list_topics(conn: &Connection) -> Result<Value> {
    let focus = session::focus_topic_id(conn);
    let today = session::topic_for_day(conn)?.id;
    let topics = stats::topic_progress(conn)?
        .into_iter()
        .map(|p| {
            json!({
                "topic_id": p.topic.id,
                "kind": p.topic.kind,
                "title": p.topic.title,
                "title_pl": p.topic.title_pl,
                "words": p.total_words,
                "words_met": p.seen_words,
                "words_mastered": p.mastered_words,
                "sentences": p.total_sentences,
                "is_today": p.topic.id == today,
                "is_daily_focus": Some(p.topic.id) == focus,
            })
        })
        .collect();
    Ok(Value::Array(topics))
}

fn topic_detail(conn: &Connection, topic_id: i64) -> Result<Value> {
    let topic = session::load_topic(conn, topic_id)?;
    let mut stmt = conn.prepare(
        "SELECT w.pl, w.en, w.pos, r.reps, r.lapses FROM words w
         LEFT JOIN reviews r ON r.kind = 'word' AND r.item_id = w.id
         WHERE w.topic_id = ?1 ORDER BY w.ord",
    )?;
    let words: Vec<Value> = stmt
        .query_map([topic_id], |r| {
            Ok(json!({
                "pl": r.get::<_, String>(0)?,
                "en": r.get::<_, String>(1)?,
                "pos": r.get::<_, String>(2)?,
                "met": r.get::<_, Option<i64>>(3)?.is_some(),
                "correct_reps": r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                "lapses": r.get::<_, Option<i64>>(4)?.unwrap_or(0),
            }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    let mut stmt = conn.prepare("SELECT pl, en FROM sentences WHERE topic_id = ?1 ORDER BY ord")?;
    let sentences: Vec<Value> = stmt
        .query_map([topic_id], |r| {
            Ok(json!({ "pl": r.get::<_, String>(0)?, "en": r.get::<_, String>(1)? }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    let dialogue: Vec<Value> = session::dialogue_lines(conn, topic_id)?
        .into_iter()
        .map(|l| json!({ "speaker": l.speaker, "pl": l.pl, "en": l.meaning }))
        .collect();
    Ok(json!({
        "topic_id": topic.id,
        "kind": topic.kind,
        "title": topic.title,
        "title_pl": topic.title_pl,
        "summary": topic.summary,
        "words": words,
        "sentences": sentences,
        "dialogue": dialogue,
    }))
}

fn search(conn: &Connection, query: &str) -> Result<Value> {
    let pattern = format!("%{}%", query.trim());
    let mut stmt = conn.prepare(
        "SELECT w.pl, w.en, w.pos, t.title_en, t.id, r.reps FROM words w
         JOIN topics t ON t.id = w.topic_id
         LEFT JOIN reviews r ON r.kind = 'word' AND r.item_id = w.id
         WHERE w.pl LIKE ?1 OR w.en LIKE ?1 ORDER BY t.kind, t.ord, w.ord LIMIT 40",
    )?;
    let words: Vec<Value> = stmt
        .query_map([&pattern], |r| {
            Ok(json!({
                "pl": r.get::<_, String>(0)?,
                "en": r.get::<_, String>(1)?,
                "pos": r.get::<_, String>(2)?,
                "topic": r.get::<_, String>(3)?,
                "topic_id": r.get::<_, i64>(4)?,
                "met": r.get::<_, Option<i64>>(5)?.is_some(),
            }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    let mut stmt = conn.prepare(
        "SELECT s.pl, s.en, t.title_en FROM sentences s JOIN topics t ON t.id = s.topic_id
         WHERE s.pl LIKE ?1 OR s.en LIKE ?1 LIMIT 15",
    )?;
    let sentences: Vec<Value> = stmt
        .query_map([&pattern], |r| {
            Ok(json!({
                "pl": r.get::<_, String>(0)?,
                "en": r.get::<_, String>(1)?,
                "topic": r.get::<_, String>(2)?,
            }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(json!({ "words": words, "sentences": sentences }))
}

fn progress(conn: &Connection) -> Result<Value> {
    let s = stats::collect(conn)?;
    let mut stmt = conn.prepare(
        "SELECT w.pl, w.en, r.lapses, r.reps FROM reviews r JOIN words w ON w.id = r.item_id
         WHERE r.kind = 'word' AND r.lapses > 0
         ORDER BY r.lapses DESC, r.ease ASC LIMIT 15",
    )?;
    let hardest: Vec<Value> = stmt
        .query_map([], |r| {
            Ok(json!({
                "pl": r.get::<_, String>(0)?,
                "en": r.get::<_, String>(1)?,
                "times_wrong": r.get::<_, i64>(2)?,
                "correct_streak": r.get::<_, i64>(3)?,
            }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let recent: Vec<Value> = s
        .history
        .iter()
        .rev()
        .take(14)
        .map(|d| {
            json!({ "days_ago": s.day - d.day, "xp": d.xp, "answered": d.answered,
                    "correct": d.correct, "completed": d.completed })
        })
        .collect();
    Ok(json!({
        "level": s.level,
        "total_xp": s.total_xp,
        "xp_today": s.day_xp,
        "daily_goal": s.daily_goal,
        "current_streak": s.current_streak,
        "longest_streak": s.longest_streak,
        "accuracy": (s.accuracy * 100.0).round(),
        "words_met": s.words_seen,
        "words_mastered": s.words_mastered,
        "words_total": s.words_total,
        "sentences_met": s.sentences_seen,
        "due_today": s.due_today,
        "completed_today": s.completed_today,
        "new_words_per_session": db::get_setting_i64(conn, "new_words_per_day", 12),
        "recent_days": recent,
        "hardest_words": hardest,
        "achievements_unlocked": s.achievements.iter().filter(|a| a.unlocked).map(|a| a.title.clone()).collect::<Vec<_>>(),
    }))
}

/// make_song needs the network twice (the songwriter, then Suno), so unlike
/// the other tools it cannot run under the database lock.
async fn make_song(app: &tauri::AppHandle, db: &Db, arguments: &str) -> Outcome {
    let result: Result<Outcome> = async {
        let args: SongArgs = parse(arguments)?;
        // Fail before paying for lyrics if Suno cannot be reached anyway.
        songs::kie_key(&db.lock())?;
        let draft = songs::write_draft(
            db,
            songs::DraftRequest {
                brief: args.about,
                topic_id: args.topic_id,
                chorus_pl: args.chorus_pl,
                style: args.style,
                vocal_gender: args.vocal_gender,
                ..Default::default()
            },
        )
        .await?;
        let title = draft.title.clone();
        let chorus = format!("{} — {}", draft.chorus_pl, draft.chorus_en);
        let song_id = songs::start_song(app, db, draft)?;
        Ok(Outcome::song(
            json!({ "song_id": song_id, "title": title, "chorus": chorus, "status": "Suno is singing it now; ready in 2–4 minutes" }),
            "songStarted",
            format!("Writing “{title}” — Suno is singing it now"),
            Some(song_id),
        ))
    }
    .await;
    result.unwrap_or_else(|error| Outcome::data(json!({ "error": error.to_string() })))
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

fn parse_actions(json: Option<String>) -> Vec<TutorAction> {
    json.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

pub fn load_history(conn: &Connection) -> Result<Vec<ChatMessage>> {
    let mut stmt =
        conn.prepare("SELECT id, role, content, actions FROM tutor_messages ORDER BY id")?;
    let messages = stmt
        .query_map([], |r| {
            Ok(ChatMessage {
                id: r.get(0)?,
                role: r.get(1)?,
                content: r.get(2)?,
                actions: parse_actions(r.get(3)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(messages)
}

/// The recent conversation as the model should see it. What the tutor did in
/// earlier turns is spelled out, so it knows the ids of what it made.
fn recent_history(conn: &Connection) -> Result<Vec<Wire>> {
    let mut stmt = conn.prepare(
        "SELECT role, content, actions FROM (
             SELECT id, role, content, actions FROM tutor_messages ORDER BY id DESC LIMIT ?1
         ) ORDER BY id",
    )?;
    let history = stmt
        .query_map([HISTORY_LIMIT], |r| {
            let role: String = r.get(0)?;
            let mut content: String = r.get(1)?;
            let actions = parse_actions(r.get(2)?);
            if !actions.is_empty() {
                let notes: Vec<String> = actions
                    .iter()
                    .map(|a| match a.topic_id {
                        Some(id) => format!("{} (topic id {id})", a.label),
                        None => a.label.clone(),
                    })
                    .collect();
                content.push_str(&format!("\n\n[Done in the app: {}]", notes.join("; ")));
            }
            Ok(if role == "user" { Wire::user(content) } else { Wire::assistant(content) })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(history)
}

fn append(conn: &Connection, role: &str, content: &str, actions: &[TutorAction]) -> Result<i64> {
    let actions = if actions.is_empty() { None } else { Some(serde_json::to_string(actions)?) };
    conn.execute(
        "INSERT INTO tutor_messages (role, content, actions, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![role, content, actions, db::now_secs()],
    )?;
    Ok(conn.last_insert_rowid())
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn tutor_history(db: State<'_, Db>) -> Result<Vec<ChatMessage>> {
    load_history(&db.lock())
}

#[tauri::command]
pub async fn tutor_clear(db: State<'_, Db>) -> Result<()> {
    db.lock().execute("DELETE FROM tutor_messages", [])?;
    Ok(())
}

/// Makes the usual failure — a model that cannot call tools — say what to do.
fn explain_failure(error: Error, model: &str) -> Error {
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

/// Sends a message to the tutor. Text streams out as `ai://stream` events with
/// the given id; while a tool runs, a `tutor://status` event says what it is
/// doing. Returns the stored reply, with the actions taken.
#[tauri::command]
pub async fn tutor_send(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    message: String,
    stream_id: String,
) -> Result<ChatMessage> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err(Error::msg("Write something first."));
    }

    let (key, model, mut wire) = {
        let conn = db.lock();
        let key = ai::api_key(&conn)?;
        let model = ai::require_model(&conn)?;
        let mut wire = vec![Wire::system(system_prompt(&conn)?)];
        wire.extend(recent_history(&conn)?);
        wire.push(Wire::user(message.clone()));
        (key, model, wire)
    };

    let tools = tool_definitions();
    let mut reply = String::new();
    let mut actions: Vec<TutorAction> = Vec::new();

    let result: Result<()> = async {
        for round in 0..MAX_ROUNDS {
            // On the last round tools are withheld, so the model has to answer.
            let offer = if round + 1 < MAX_ROUNDS { Some(&tools) } else { None };
            // Text from successive rounds is one reply; keep the paragraphs apart.
            let mut separated = reply.trim().is_empty();
            let emitter = app.clone();
            let id = stream_id.clone();
            let turn = ai::stream_turn(&key, &model, &wire, offer, |delta| {
                if !separated {
                    crate::commands::stream_to_frontend(&emitter, &id, Some("\n\n"), false, None);
                    separated = true;
                }
                crate::commands::stream_to_frontend(&emitter, &id, Some(delta), false, None);
            })
            .await
            .map_err(|e| explain_failure(e, &model))?;

            if !turn.text.trim().is_empty() {
                if !reply.trim().is_empty() {
                    reply.push_str("\n\n");
                }
                reply.push_str(turn.text.trim());
            }
            if turn.tool_calls.is_empty() {
                break;
            }

            wire.push(Wire::tool_request(&turn.text, turn.tool_calls.clone()));
            for call in &turn.tool_calls {
                let _ = app.emit(
                    "tutor://status",
                    TutorStatus {
                        id: stream_id.clone(),
                        label: status_label(&call.function.name).into(),
                    },
                );
                let outcome = if call.function.name == "make_song" {
                    make_song(&app, &db, &call.function.arguments).await
                } else {
                    let conn = db.lock();
                    execute(&conn, &call.function.name, &call.function.arguments)
                };
                if let Some(action) = outcome.action {
                    actions.push(action);
                }
                wire.push(Wire::tool_result(&call.id, outcome.result.to_string()));
            }
        }
        Ok(())
    }
    .await;

    // Even when the conversation fails half-way, whatever the tools already
    // did in the app is real, so it is recorded and shown.
    if let Err(error) = result {
        crate::commands::stream_to_frontend(&app, &stream_id, None, true, Some(error.to_string()));
        if actions.is_empty() {
            return Err(error);
        }
        reply.push_str(&format!("\n\n_(The answer was cut short: {error})_"));
    } else {
        crate::commands::stream_to_frontend(&app, &stream_id, None, true, None);
    }

    if reply.trim().is_empty() {
        reply = "Done.".into();
    }
    let conn = db.lock();
    append(&conn, "user", &message, &[])?;
    let id = append(&conn, "assistant", &reply, &actions)?;
    Ok(ChatMessage { id, role: "assistant".into(), content: reply, actions })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_valid_definition() {
        let tools = tool_definitions();
        let tools = tools.as_array().unwrap();
        assert!(tools.len() >= 8);
        for tool in tools {
            assert_eq!(tool["type"], "function");
            let name = tool["function"]["name"].as_str().unwrap();
            assert!(!name.is_empty());
            assert_eq!(tool["function"]["parameters"]["type"], "object", "{name}");
            assert_ne!(status_label(name), "", "{name}");
        }
    }

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

    pub(crate) fn run(conn: &Connection, name: &str, args: Value) -> Value {
        let outcome = execute(conn, name, &args.to_string());
        let mut result = outcome.result;
        if let Some(action) = outcome.action {
            result["_action"] = serde_json::to_value(action).unwrap();
        }
        result
    }
}

#[cfg(test)]
pub(crate) use tests::run as run_tool_for_test;
