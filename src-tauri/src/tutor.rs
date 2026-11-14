//! The tutor: an assistant for the whole app, not a Polish role-play partner.
//!
//! It talks to the learner in English and can act in the app through tools —
//! build a course on any theme, write lessons and drills into courses, look up
//! progress and vocabulary, make songs, adjust practice settings. Every
//! tool runs here, in Rust, against the same SQLite database the rest of the
//! app uses, so what the tutor makes is immediately practisable.

use std::collections::HashSet;

use diesel::prelude::*;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::State;

use crate::agent::{tool, Agent, Tool, ToolOutput};
use crate::ai::Wire;
use crate::blocks;
use crate::db::{self, Conn, Db};
use crate::schema::{
    blocks as blocks_table, checkins, course_blocks, course_sentences, course_words, reviews, songs as songs_table, topics,
    tutor_conversations, tutor_messages, words,
};
use crate::error::{Error, Result};
use crate::models::{ChatMessage, Conversation, TutorAction};
use crate::courses::{self, NewCourse, NewLine, NewSentence, NewWord};
use crate::session;
use crate::songs;
use crate::playlists;
use crate::stats;

/// How much of the conversation is replayed to the model.
const HISTORY_LIMIT: i64 = 30;
/// Tool rounds per learner message. Creating a course takes one or two;
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

What the app does — learning happens in blocks:
- A block is small and has one job. The kinds:
  - drill: mechanical practice on a few words or sentences. Formats: words (new-word cards, \
    multiple choice, typing as they get known, then a memory round), memory (memory cards \
    only), typing (type every word from memory), sentences (build them from shuffled words, \
    then fill gaps), gaps (fill-the-gaps only), dialogue (read a dialogue, fill its lines).
  - study: a short lesson in markdown — a conjugation, a grammar point, culture — with \
    objectives. Its check-in is an interview by an examiner agent that asks questions and \
    decides pass, partial or fail.
  - listening: a study block tied to a song. The learner listens (lyrics hidden), then the \
    examiner asks about what they heard. make_song makes one with each new song; \
    create_listening_block makes one for an existing song.
  - writing: a study block that is a situation played out in writing with an AI partner who \
    only understands Polish (explain where you live to a new neighbour, order dinner, book a \
    room). It ends when the learner has got every goal across in Polish. Make one with \
    create_writing_block when the learner wants to practise writing or using what they know; \
    goals must be things to get across (\"Say where you live\"), not grammar to show.
- Complexity comes from how hard the subject is, not from how much is in it:
  - low (1–2 min): one small idea. 6–8 words; one verb's present tense; one politeness rule.
  - medium (5–10 min): a pattern with variations. A conjugation group; gender and adjective \
    agreement; numbers 1–100; a dialogue at the bakery.
  - high (20+ min): a system with interacting parts. Using the cases; verb aspect \
    (perfective/imperfective); the past tense with gender; verbs of motion.
  Never make a block high by stuffing it with items — split it instead.
- Every block has a check-in period (days). Passing pushes the next check-in further out; \
  partial repeats the period; failing brings it back tomorrow. Defaults: drills 2/4/7 days, \
  study blocks 5/7/10 days for low/medium/high.
- Three levels: a course lists blocks; a drill block owns the words, sentences or dialogue \
  it practises (a course has words only through its drills). Giving a course words, sentences \
  or a dialogue cuts them into drills: the dialogue, words in eights, sentences in fives. A \
  block can be listed in several courses (one block, one schedule); taking it out of its last \
  course deletes it with its words. Use move_block, add_block_to_course, \
  remove_block_from_course and order_course_blocks to rearrange. The app started with a set of courses (greetings, \
  food, numbers… and Grammar essentials); they are ordinary courses — you can add to, edit or \
  delete any course or block. Lessons with no better home go in \"Lessons\". A song's listening \
  block goes in the course the song was written from (or the course_id you give). Listening \
  blocks are ordinary blocks: they move, share and reorder like any other.
- File songs yourself: when you make a song, pass the course_id of the course it fits best, \
  even when the learner did not name one. For a song with no course (its listening block sits \
  in \"Listening\"), read its lyrics with get_song, decide which course its words and theme \
  belong to, and move the block there with move_block (or share it into two courses if it \
  fits both). Keep \"Listening\" only for songs that fit no course.

Using tools:
- When the learner asks for practice on a theme (colours, numbers, a trip…), build a \
  course with create_course right away — don't ask first, never say you can't. Unless \
  they ask otherwise: 15–30 words, 8–12 short sentences using them, and a natural dialogue of \
  8–12 lines. For one specific kind of practice (e.g. \"just memory cards\"), use \
  create_drill_block with that format.
- When they want to understand something (a conjugation, a case, a grammar point, a cultural \
  habit), write a study block with create_study_block, filed under the course it belongs to: a clear, correct lesson with a small \
  table or examples, 2–5 concrete objectives (what they should be able to *do*), and an \
  honest complexity. Break big subjects into several blocks of rising complexity rather than \
  one huge one.
- Words: the Polish dictionary form (nouns in the nominative singular, verbs in the infinitive, \
  adjectives in the masculine form), lowercase unless a proper noun; a short English gloss \
  (alternatives separated by commas); a part-of-speech code. Sentences: short (max ~8 words), \
  natural, beginner-friendly Polish. Everything must be correct Polish with correct diacritics.
- The reply under each block you make already offers to open it; use suggest_block to point \
  at an existing one, and schedule_block to make one due now or change its period.
- Use list_blocks and get_block to see what exists and how check-ins went (including what the \
  examiner said to review) before suggesting what to do.
- Call get_progress before commenting on how the learner is doing; never invent numbers.
- delete_course, delete_block and delete_songs are permanent: only call them after the learner has \
  clearly confirmed (name what will be deleted and ask first).
- Songs: the Music tab has learning songs — English lines answered by Polish ones, and a \
  Polish chorus sentence that gets stuck in your head. When the learner asks for a song, call \
  make_song with what it should be about (and a topic id, chorus sentence or style if they \
  said or it fits). The app's songwriter writes the lyrics and Suno sings two takes in 2–4 \
  minutes; it costs the learner kie.ai credits, so make one song per request unless they ask \
  for more. Use list_songs to see what exists, e.g. before suggesting what to delete. Every song keeps what it \
  was made with (style, singer, brief, length, models, revisions): get_song shows it under \
  made_with — to make a song like another, or a variation, start from its style and singer.
- Playlists: the learner groups songs into playlists in the Music tab. list_playlists shows them \
  with their songs; create_playlist, add_to_playlist, remove_from_playlist and rename_playlist \
  change them, by song id (a song's most-played take goes in). Use list_songs to find song ids \
  from titles. delete_playlist only after the learner confirmed; the songs themselves stay.
- To grow an existing course (more words on food, a lesson on a grammar point it uses), add to it \
  with add_to_course or file the new block under it (course_id) rather than making a new course.
- After using a tool, say in one or two sentences what you did. Don't paste the whole word list \
  back unless asked.";

pub(crate) fn system_prompt(conn: &mut Conn) -> Result<String> {
    let stats = stats::collect(conn)?;
    let overview = blocks::overview(conn)?;
    let line = |b: &blocks::Block| {
        format!(
            "  - id {}: {} ({} {}, {}){}",
            b.id,
            b.title,
            b.complexity,
            b.kind,
            b.topic_title.clone().unwrap_or_default(),
            b.last_result.as_ref().map(|r| format!(", last check-in: {r}")).unwrap_or_default()
        )
    };
    let due: Vec<String> = overview.due.iter().take(15).map(line).collect();

    let listed: Vec<i64> = course_blocks::table.select(course_blocks::topic_id).load(conn)?;
    let courses: Vec<String> = topics::table
        .order(crate::blocks::course_order())
        .limit(60)
        .select((topics::id, topics::title_en, topics::title_pl))
        .load::<(i64, String, String)>(conn)?
        .into_iter()
        .map(|(id, title, title_pl)| {
            let blocks = listed.iter().filter(|&&c| c == id).count();
            format!("  - id {id}: {title} ({title_pl}), {blocks} blocks")
        })
        .collect();

    // Every lesson, with the courses that list it.
    let mut lessons: Vec<(i64, String, String, i64, Vec<(i64, i64, String)>)> = Vec::new();
    for (id, title, complexity, reps, course_ord, list_ord, course) in course_blocks::table
        .inner_join(blocks_table::table)
        .inner_join(topics::table)
        .filter(blocks_table::kind.eq("study"))
        .order((topics::ord, course_blocks::ord))
        .select((
            blocks_table::id,
            blocks_table::title,
            blocks_table::complexity,
            blocks_table::reps,
            topics::ord,
            course_blocks::ord,
            topics::title_en,
        ))
        .load::<(i64, String, String, i64, i64, i64, String)>(conn)?
    {
        match lessons.iter_mut().find(|l| l.0 == id) {
            Some(lesson) => lesson.4.push((course_ord, list_ord, course)),
            None => lessons.push((id, title, complexity, reps, vec![(course_ord, list_ord, course)])),
        }
    }
    let lessons: Vec<String> = lessons
        .into_iter()
        .map(|(id, title, complexity, reps, courses)| {
            let names: Vec<String> = courses.into_iter().map(|c| c.2).collect();
            let studied = if reps > 0 { ", studied" } else { "" };
            format!("  - id {id}: {title} ({complexity}, in {}{studied})", names.join(", "))
        })
        .collect();

    let song_count: i64 = songs_table::table.count().get_result(conn)?;
    let unsorted: i64 = course_blocks::table
        .inner_join(topics::table)
        .filter(topics::slug.eq("listening"))
        .count()
        .get_result(conn)?;
    let playlists = playlists::list(conn)?;

    let none = || "  (none)".to_string();
    Ok(format!(
        "{RULES}\n\nThe learner right now:\n\
         - Level {level}, {xp} XP, streak {streak} days{done}, {seen} of {total} words met.\n\
         - Due now ({due_count}):\n{due}\n\
         - Study blocks:\n{lessons}\n\
         - Courses:\n{courses}\n\
         - Songs: {song_count} in the Music tab{song_note}{unsorted}.\n\
         - Playlists: {playlists}.",
        level = stats.level,
        xp = stats.total_xp,
        streak = stats.current_streak,
        done = if stats.completed_today { " (practised today)" } else { "" },
        seen = stats.words_seen,
        total = stats.words_total,
        due_count = overview.due.len(),
        due = if due.is_empty() { none() } else { due.join("\n") },
        lessons = if lessons.is_empty() { none() } else { lessons.join("\n") },
        courses = if courses.is_empty() { none() } else { courses.join("\n") },
        song_count = song_count,
        unsorted = match unsorted {
            0 => String::new(),
            n => format!(
                "; {n} listening block{} still in \"Listening\" with no course — when it fits the \
                 conversation, offer to read their lyrics and file them",
                if n == 1 { " is" } else { "s are" }
            ),
        },
        playlists = {
            let list = &playlists;
            if list.is_empty() {
                "none yet".to_string()
            } else {
                list.iter()
                    .map(|p| format!("id {}: {} ({} songs)", p.id, p.name, p.entries.len()))
                    .collect::<Vec<_>>()
                    .join("; ")
            }
        },
        song_note = if songs::kie_key(conn).is_ok() {
            ""
        } else {
            "; making songs needs a kie.ai key in Settings, which is missing"
        },
    ) + &crate::memories::prompt_section(conn)?)
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

/// Everything the tutor can do in the app.
pub fn tool_definitions() -> Vec<Tool> {
    let none = json!({ "type": "object", "properties": {} });
    vec![
        tool(
            "create_course",
            "Building the course…",
            "Create a new course on a theme, with words, sentences and optionally a dialogue. \
             It is cut into drill blocks and can be practised straight away.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short English title, e.g. \"Colours\"" },
                    "title_pl": { "type": "string", "description": "The title in Polish, e.g. \"Kolory\"" },
                    "summary": { "type": "string", "description": "One English sentence on what the course covers" },
                    "words": word_schema(),
                    "sentences": sentence_schema(),
                    "dialogue": dialogue_schema()
                },
                "required": ["title", "title_pl", "summary", "words", "sentences"]
            }),
        ),
        tool(
            "add_to_course",
            "Updating the course…",
            "Add words, sentences or dialogue lines to a course; its drills are cut again to include them.",
            json!({
                "type": "object",
                "properties": {
                    "course_id": { "type": "integer" },
                    "words": word_schema(),
                    "sentences": sentence_schema(),
                    "dialogue": dialogue_schema()
                },
                "required": ["course_id"]
            }),
        ),
        tool(
            "edit_course",
            "Updating the course…",
            "Rename a course, change its summary, or remove words/sentences from it (by their exact Polish text).",
            json!({
                "type": "object",
                "properties": {
                    "course_id": { "type": "integer" },
                    "title": { "type": "string" },
                    "title_pl": { "type": "string" },
                    "summary": { "type": "string" },
                    "remove_words": { "type": "array", "items": { "type": "string" } },
                    "remove_sentences": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["course_id"]
            }),
        ),
        tool(
            "delete_course",
            "Deleting the course…",
            "Permanently delete a course with all its blocks and practice history. Only after the learner confirmed.",
            json!({
                "type": "object",
                "properties": { "course_id": { "type": "integer" } },
                "required": ["course_id"]
            }),
        ),
        tool(
            "list_courses",
            "Looking at your courses…",
            "List every course, with how much of each the learner has met.",
            none.clone(),
        ),
        tool(
            "get_course",
            "Looking at your courses…",
            "The words, sentences and dialogue of one course, with the learner's progress on each word.",
            json!({
                "type": "object",
                "properties": { "course_id": { "type": "integer" } },
                "required": ["course_id"]
            }),
        ),
        tool(
            "search_vocabulary",
            "Searching the vocabulary…",
            "Search every word and sentence in the app, in Polish or English.",
            json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"]
            }),
        ),
        tool(
            "get_progress",
            "Checking your progress…",
            "The learner's statistics: XP, level, streak, accuracy, words met and mastered, recent days, and the words they get wrong most.",
            none,
        ),
        tool(
            "create_writing_block",
            "Setting the scene…",
            "Make a writing block: a situation the learner writes their way through with an AI \
             partner who only understands Polish. It ends when they have got every goal across.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short, e.g. \"Where do you live?\"" },
                    "summary": { "type": "string", "description": "One sentence: what the learner does" },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"], "description": "How much Polish it takes: low — a few simple sentences; medium — a short exchange; high — a real conversation. Also sets how simply the partner writes." },
                    "scenario": { "type": "string", "description": "The situation in English, to the learner: where they are and who they are writing with, e.g. \"You've just moved in. Your new neighbour Pani Ewa stops you on the stairs.\"" },
                    "goals": { "type": "array", "items": { "type": "string" }, "description": "1–4 things the learner has to get across, e.g. \"Say which city you live in\", \"Say who you live with\"" },
                    "period_days": { "type": "integer", "description": "Days between check-ins; leave out for the default" },
                    "course_id": { "type": "integer", "description": "The course to file it under; leave out to file it in \"Writing\"" }
                },
                "required": ["title", "summary", "complexity", "scenario", "goals"]
            }),
        ),
        tool(
            "create_study_block",
            "Writing the lesson…",
            "Write a study block: a short lesson (conjugation, grammar, culture) with objectives. \
             The examiner interviews the learner on the objectives at each check-in.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short, e.g. \"Iść — to go (on foot)\"" },
                    "summary": { "type": "string", "description": "One sentence: what the block covers" },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"], "description": "By how hard the subject is: low ≈ 1–2 min, medium ≈ 5–10 min, high ≈ 20+ min" },
                    "lesson": { "type": "string", "description": "The lesson in markdown: explanation, a table or examples with translations. Correct Polish only." },
                    "objectives": { "type": "array", "items": { "type": "string" }, "description": "2–5 things the learner should be able to do afterwards" },
                    "period_days": { "type": "integer", "description": "Days between check-ins; leave out for the default" },
                    "course_id": { "type": "integer", "description": "The course to file it under; leave out to file it in \"Lessons\"" }
                },
                "required": ["title", "summary", "complexity", "lesson", "objectives"]
            }),
        ),
        tool(
            "create_drill_block",
            "Making the drill…",
            "Make one drill block in a chosen format over words and/or sentences you give. \
             The items are added to a course (a new one named after the block if none is given).",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "format": { "type": "string", "enum": ["words", "memory", "typing", "sentences", "gaps"], "description": "words: cards + multiple choice + memory round; memory: memory cards only; typing: type each word; sentences: build them + gaps; gaps: fill-the-gaps only" },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"] },
                    "words": word_schema(),
                    "sentences": sentence_schema(),
                    "period_days": { "type": "integer" },
                    "course_id": { "type": "integer" }
                },
                "required": ["title", "format", "complexity"]
            }),
        ),
        tool(
            "create_listening_block",
            "Making the listening block…",
            "Make a listening block for a song: the learner listens, then the examiner asks about what they heard (meaning, missing words, translation, grammar). One per song.",
            json!({
                "type": "object",
                "properties": {
                    "song_id": { "type": "integer" },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"], "description": "low: the gist and the chorus; medium: lines and missing words; high: also the grammar of what is sung" },
                    "focus": { "type": "string", "description": "Optional: what to listen out for, e.g. \"the verbs in the first verse\"" },
                    "course_id": { "type": "integer", "description": "The course to file it in; leave out for the course the song was written from" }
                },
                "required": ["song_id"]
            }),
        ),
        tool(
            "list_blocks",
            "Looking at your blocks…",
            "List blocks: those due now, one course's blocks, or every study block.",
            json!({
                "type": "object",
                "properties": {
                    "which": { "type": "string", "enum": ["due", "course", "study"] },
                    "course_id": { "type": "integer", "description": "With which = course" }
                },
                "required": ["which"]
            }),
        ),
        tool(
            "get_block",
            "Looking at your blocks…",
            "One block in full: lesson and objectives or its words and sentences, its schedule, and the last check-in's verdict.",
            json!({
                "type": "object",
                "properties": { "block_id": { "type": "integer" } },
                "required": ["block_id"]
            }),
        ),
        tool(
            "update_block",
            "Updating the block…",
            "Change a block: title, summary, complexity, lesson, objectives or check-in period.",
            json!({
                "type": "object",
                "properties": {
                    "block_id": { "type": "integer" },
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"] },
                    "lesson": { "type": "string" },
                    "objectives": { "type": "array", "items": { "type": "string" } },
                    "period_days": { "type": "integer" }
                },
                "required": ["block_id"]
            }),
        ),
        tool(
            "delete_block",
            "Deleting the block…",
            "Permanently delete a block and its check-in history. Only after the learner confirmed.",
            json!({
                "type": "object",
                "properties": { "block_id": { "type": "integer" } },
                "required": ["block_id"]
            }),
        ),
        tool(
            "add_block_to_course",
            "Rearranging your courses…",
            "List a block in another course too. It stays one block — one schedule, one history — shown in each course.",
            json!({
                "type": "object",
                "properties": { "block_id": { "type": "integer" }, "course_id": { "type": "integer" } },
                "required": ["block_id", "course_id"]
            }),
        ),
        tool(
            "remove_block_from_course",
            "Rearranging your courses…",
            "Take a block out of a course. If no other course lists it, the block is deleted with its words, sentences and history — only after the learner confirmed that.",
            json!({
                "type": "object",
                "properties": { "block_id": { "type": "integer" }, "course_id": { "type": "integer" } },
                "required": ["block_id", "course_id"]
            }),
        ),
        tool(
            "move_block",
            "Rearranging your courses…",
            "Move a block from one course to another; its words and history go with it.",
            json!({
                "type": "object",
                "properties": {
                    "block_id": { "type": "integer" },
                    "from_course_id": { "type": "integer" },
                    "to_course_id": { "type": "integer" }
                },
                "required": ["block_id", "from_course_id", "to_course_id"]
            }),
        ),
        tool(
            "order_course_blocks",
            "Rearranging your courses…",
            "Put a course's blocks in an order (block ids, first to last). Blocks left out keep their order after these.",
            json!({
                "type": "object",
                "properties": {
                    "course_id": { "type": "integer" },
                    "block_ids": { "type": "array", "items": { "type": "integer" } }
                },
                "required": ["course_id", "block_ids"]
            }),
        ),
        tool(
            "schedule_block",
            "Updating the block…",
            "Make a block due now, so it shows at the top of the learner's list.",
            json!({
                "type": "object",
                "properties": { "block_id": { "type": "integer" } },
                "required": ["block_id"]
            }),
        ),
        tool(
            "suggest_block",
            "Finding the block…",
            "Show the learner a button that opens a block right now.",
            json!({
                "type": "object",
                "properties": { "block_id": { "type": "integer" } },
                "required": ["block_id"]
            }),
        ),
        tool(
            "update_practice_settings",
            "Updating your settings…",
            "Change the daily XP goal (10–1000).",
            json!({
                "type": "object",
                "properties": {
                    "daily_goal": { "type": "integer" }
                },
                "required": ["daily_goal"]
            }),
        ),
        tool(
            "make_song",
            "Writing the song…",
            "Have a learning song written and sung (takes 2–4 minutes, costs kie.ai credits). \
             The songwriter writes the lyrics from this brief.",
            json!({
                "type": "object",
                "properties": {
                    "about": { "type": "string", "description": "What the song is about, in English" },
                    "course_id": { "type": "integer", "description": "Take vocabulary from this course; the song's listening block is filed there too" },
                    "chorus_pl": { "type": "string", "description": "An exact Polish chorus sentence, if the learner wants one" },
                    "style": { "type": "string", "description": "Musical style, e.g. \"upbeat indie folk, male vocals\"" },
                    "vocal_gender": { "type": "string", "enum": ["f", "m", ""] },
                    "length": {
                        "type": "string",
                        "enum": ["short", "medium", "long"],
                        "description": "short ≈ 2 min, medium ≈ 3 min, long ≈ 4–5 min. Leave out to use the learner's setting."
                    },
                    "listening_block": { "type": "boolean", "description": "Also make a listening block for the song (default true)" }
                },
                "required": ["about"]
            }),
        ),
        tool(
            "list_songs",
            "Looking at your songs…",
            "The learner's songs: title, chorus, musical style, singer, what each is about, status, takes and how often they were played.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "get_song",
            "Reading the lyrics…",
            "One song in full: its lyrics (Polish with English, by section), the course it was written from, where its listening block is filed, and everything it was made with — style, singer, Suno model, the brief, what was asked for, length, the songwriter model and any revisions. Use it to make a song that matches or varies another.",
            json!({
                "type": "object",
                "properties": { "song_id": { "type": "integer" } },
                "required": ["song_id"]
            }),
        ),
        tool(
            "delete_songs",
            "Deleting songs…",
            "Permanently delete songs (all their takes). Only after the learner confirmed.",
            json!({
                "type": "object",
                "properties": { "song_ids": { "type": "array", "items": { "type": "integer" } } },
                "required": ["song_ids"]
            }),
        ),
        tool(
            "list_playlists",
            "Looking at your playlists…",
            "The learner's playlists, each with its songs in order.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "create_playlist",
            "Updating your playlists…",
            "Make a playlist, optionally with songs in it already.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "song_ids": { "type": "array", "items": { "type": "integer" } }
                },
                "required": ["name"]
            }),
        ),
        tool(
            "add_to_playlist",
            "Updating your playlists…",
            "Add songs to the end of a playlist. Songs already in it are skipped.",
            json!({
                "type": "object",
                "properties": {
                    "playlist_id": { "type": "integer" },
                    "song_ids": { "type": "array", "items": { "type": "integer" } }
                },
                "required": ["playlist_id", "song_ids"]
            }),
        ),
        tool(
            "remove_from_playlist",
            "Updating your playlists…",
            "Take songs out of a playlist. The songs stay in the library.",
            json!({
                "type": "object",
                "properties": {
                    "playlist_id": { "type": "integer" },
                    "song_ids": { "type": "array", "items": { "type": "integer" } }
                },
                "required": ["playlist_id", "song_ids"]
            }),
        ),
        tool(
            "rename_playlist",
            "Updating your playlists…",
            "Rename a playlist.",
            json!({
                "type": "object",
                "properties": { "playlist_id": { "type": "integer" }, "name": { "type": "string" } },
                "required": ["playlist_id", "name"]
            }),
        ),
        tool(
            "delete_playlist",
            "Updating your playlists…",
            "Delete a playlist (its songs stay in the library). Only after the learner confirmed.",
            json!({
                "type": "object",
                "properties": { "playlist_id": { "type": "integer" } },
                "required": ["playlist_id"]
            }),
        ),
    ]
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
            action: Some(TutorAction { kind: kind.into(), label, topic_id, song_id: None, block_id: None }),
        }
    }
    fn block(result: Value, kind: &str, label: String, block_id: i64) -> Self {
        Outcome {
            result,
            action: Some(TutorAction {
                kind: kind.into(),
                label,
                topic_id: None,
                song_id: None,
                block_id: Some(block_id),
            }),
        }
    }
    fn song(result: Value, kind: &str, label: String, song_id: Option<i64>) -> Self {
        Outcome {
            result,
            action: Some(TutorAction { kind: kind.into(), label, topic_id: None, song_id, block_id: None }),
        }
    }
}

#[derive(Deserialize)]
struct TopicArg {
    #[serde(rename = "course_id", alias = "topic_id")]
    topic_id: i64,
}

#[derive(Deserialize)]
struct CreateArgs {
    #[serde(flatten)]
    course: NewCourse,
}

#[derive(Deserialize)]
struct ExtendArgs {
    #[serde(rename = "course_id", alias = "topic_id")]
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
    #[serde(rename = "course_id", alias = "topic_id")]
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
struct SettingsArgs {
    #[serde(default)]
    daily_goal: Option<i64>,
}

#[derive(Deserialize)]
struct BlockArg {
    block_id: i64,
}

#[derive(Deserialize)]
struct MembershipArgs {
    block_id: i64,
    course_id: i64,
}

#[derive(Deserialize)]
struct MoveArgs {
    block_id: i64,
    from_course_id: i64,
    to_course_id: i64,
}

#[derive(Deserialize)]
struct OrderArgs {
    course_id: i64,
    #[serde(default)]
    block_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct WritingArgs {
    #[serde(flatten)]
    writing: blocks::NewWriting,
}

#[derive(Deserialize)]
struct StudyArgs {
    #[serde(flatten)]
    study: blocks::NewStudy,
}

#[derive(Deserialize)]
struct DrillArgs {
    title: String,
    #[serde(default)]
    summary: String,
    format: String,
    complexity: String,
    #[serde(default)]
    words: Vec<NewWord>,
    #[serde(default)]
    sentences: Vec<NewSentence>,
    #[serde(default)]
    period_days: Option<i64>,
    #[serde(default, alias = "programme_id", alias = "topic_id")]
    course_id: Option<i64>,
}

#[derive(Deserialize)]
struct ListBlocksArgs {
    which: String,
    #[serde(default, rename = "course_id", alias = "topic_id")]
    topic_id: Option<i64>,
}

#[derive(Deserialize)]
struct UpdateBlockArgs {
    block_id: i64,
    #[serde(flatten)]
    changes: blocks::BlockChanges,
}

fn complexity_label(complexity: &str) -> &'static str {
    match complexity {
        "low" => "a low (1–2 min)",
        "medium" => "a medium (5–10 min)",
        _ => "a deep (20+ min)",
    }
}

fn block_json(block: &blocks::Block) -> Value {
    json!({
        "block_id": block.id,
        "kind": block.kind,
        "format": block.format,
        "title": block.title,
        "summary": block.summary,
        "complexity": block.complexity,
        "course_ids": block.course_ids,
        "course": block.topic_title,
        "items": block.items,
        "period_days": block.period_days,
        "times_done": block.reps,
        "last_result": block.last_result,
        "due_in_days": block.due_day.map(|due| due - block.last_day.unwrap_or(due)),
        "made_by": block.origin,
    })
}


#[derive(Deserialize)]
struct SongArgs {
    about: String,
    #[serde(default, rename = "course_id", alias = "topic_id")]
    topic_id: Option<i64>,
    #[serde(default)]
    chorus_pl: String,
    #[serde(default)]
    style: String,
    #[serde(default)]
    vocal_gender: String,
    #[serde(default)]
    length: String,
    #[serde(default = "yes")]
    listening_block: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct ListeningArgs {
    song_id: i64,
    #[serde(default, alias = "topic_id")]
    course_id: Option<i64>,
    #[serde(default)]
    complexity: Option<String>,
    #[serde(default)]
    focus: Option<String>,
}

#[derive(Deserialize)]
struct SongArg {
    song_id: i64,
}

/// "female", "male" or "any" — clearer for the model than "f"/"m"/"".
fn singer(vocal_gender: &str) -> &'static str {
    match vocal_gender {
        "f" => "female",
        "m" => "male",
        _ => "any",
    }
}

/// Which course a song was written from, and where its listening block is.
fn song_placement(conn: &mut Conn, song_id: i64) -> Result<Value> {
    let course: Option<(i64, String)> = songs_table::table
        .inner_join(topics::table)
        .filter(songs_table::id.eq(song_id))
        .select((topics::id, topics::title_en))
        .first(conn)
        .optional()?;
    let block: Option<i64> = blocks_table::table
        .filter(blocks_table::song_id.eq(song_id))
        .select(blocks_table::id)
        .first(conn)
        .optional()?;
    let block_courses: Vec<Value> = match block {
        Some(id) => blocks::detail(conn, id)?
            .courses
            .into_iter()
            .map(|(id, title, _)| json!({ "course_id": id, "title": title }))
            .collect(),
        None => vec![],
    };
    Ok(json!({
        "written_from_course": course.map(|(id, title)| json!({ "course_id": id, "title": title })),
        "listening_block_id": block,
        "listening_block_in": block_courses,
    }))
}

#[derive(Deserialize)]
struct DeleteSongsArgs {
    song_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct PlaylistArgs {
    #[serde(default)]
    playlist_id: Option<i64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    song_ids: Vec<i64>,
}

impl PlaylistArgs {
    fn id(&self) -> Result<i64> {
        self.playlist_id.ok_or_else(|| Error::msg("Give a playlist_id (see list_playlists)."))
    }
}

fn titles(list: &[String]) -> String {
    list.iter().map(|t| format!("“{t}”")).collect::<Vec<_>>().join(", ")
}

fn playlist_json(conn: &mut Conn, playlist: &playlists::Playlist) -> Result<Value> {
    let songs: Vec<Value> = playlists::songs_in(conn, playlist.id)?
        .into_iter()
        .map(|(_, song, title, take)| json!({ "song_id": song, "title": title, "take": take }))
        .collect();
    Ok(json!({ "playlist_id": playlist.id, "name": playlist.name, "songs": songs }))
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
fn execute(conn: &mut Conn, name: &str, arguments: &str) -> Outcome {
    match run_tool(conn, name, arguments) {
        Ok(outcome) => outcome,
        Err(error) => Outcome::data(json!({ "error": error.to_string() })),
    }
}

fn run_tool(conn: &mut Conn, name: &str, arguments: &str) -> Result<Outcome> {
    match name {
        "create_course" => {
            let args: CreateArgs = parse(arguments)?;
            let created = courses::create(conn, args.course, "tutor")?;
            let mut label = format!(
                "Created “{}” — {} words, {} sentences",
                created.title, created.words, created.sentences
            );
            if created.dialogue_lines > 0 {
                label.push_str(", a dialogue");
            }
            let blocks_made = blocks::for_topic(conn, created.topic_id)?.len();
            label.push_str(&format!(" · {blocks_made} drill blocks"));
            Ok(Outcome::acted(
                json!({ "created": created, "drill_blocks": blocks_made }),
                "courseCreated",
                label,
                Some(created.topic_id),
            ))
        }
        "add_to_course" => {
            let args: ExtendArgs = parse(arguments)?;
            let updated =
                courses::extend(conn, args.topic_id, &args.words, &args.sentences, &args.dialogue, "tutor")?;
            Ok(Outcome::acted(
                json!({ "updated": updated }),
                "courseUpdated",
                format!(
                    "Updated “{}” — now {} words, {} sentences",
                    updated.title, updated.words, updated.sentences
                ),
                Some(updated.topic_id),
            ))
        }
        "edit_course" => {
            let args: EditArgs = parse(arguments)?;
            courses::rename(
                conn,
                args.topic_id,
                args.title.as_deref(),
                args.title_pl.as_deref(),
                args.summary.as_deref(),
            )?;
            let updated =
                courses::remove_items(conn, args.topic_id, &args.remove_words, &args.remove_sentences)?;
            Ok(Outcome::acted(
                json!({ "updated": updated }),
                "courseUpdated",
                format!("Edited “{}”", updated.title),
                Some(updated.topic_id),
            ))
        }
        "delete_course" => {
            let args: TopicArg = parse(arguments)?;
            let title = courses::delete(conn, args.topic_id)?;
            Ok(Outcome::acted(
                json!({ "deleted": title }),
                "courseDeleted",
                format!("Deleted “{title}”"),
                None,
            ))
        }
        "list_courses" => Ok(Outcome::data(list_topics(conn)?)),
        "get_course" => {
            let args: TopicArg = parse(arguments)?;
            Ok(Outcome::data(topic_detail(conn, args.topic_id)?))
        }
        "search_vocabulary" => {
            let args: SearchArgs = parse(arguments)?;
            Ok(Outcome::data(search(conn, &args.query)?))
        }
        "get_progress" => Ok(Outcome::data(progress(conn)?)),
        "create_writing_block" => {
            let args: WritingArgs = parse(arguments)?;
            let block = blocks::create_writing(conn, args.writing, "tutor")?;
            Ok(Outcome::block(
                json!({ "created": block_json(&block) }),
                "blockCreated",
                format!("Set up “{}” — {} writing block", block.title, complexity_label(&block.complexity)),
                block.id,
            ))
        }
        "create_study_block" => {
            let args: StudyArgs = parse(arguments)?;
            let block = blocks::create_study(conn, args.study, "tutor")?;
            Ok(Outcome::block(
                json!({ "created": block_json(&block) }),
                "blockCreated",
                format!("Wrote “{}” — {} study block", block.title, complexity_label(&block.complexity)),
                block.id,
            ))
        }
        "create_drill_block" => {
            let args: DrillArgs = parse(arguments)?;
            if args.words.is_empty() && args.sentences.is_empty() {
                return Err(Error::msg("A drill needs words or sentences."));
            }
            // The block owns its words; it is not cut into the course's own drills.
            let topic_id = match args.course_id {
                Some(id) => id,
                None => courses::create_empty(conn, &args.title, &args.summary)?,
            };
            let (block, skipped) = blocks::create_drill(
                conn,
                topic_id,
                &args.title,
                &args.summary,
                &args.format,
                &args.complexity,
                &courses::Items { words: args.words, sentences: args.sentences, dialogue: vec![] },
                args.period_days,
                "tutor",
            )?;
            Ok(Outcome::block(
                json!({ "created": block_json(&block), "course_id": topic_id, "skipped_duplicates": skipped }),
                "blockCreated",
                format!("Made “{}” — {} drill, {} items", block.title, complexity_label(&block.complexity), block.items),
                block.id,
            ))
        }
        "create_listening_block" => {
            let args: ListeningArgs = parse(arguments)?;
            let block = blocks::create_listening(
                conn,
                args.song_id,
                args.complexity.as_deref(),
                args.focus.as_deref(),
                args.course_id,
                "tutor",
            )?;
            Ok(Outcome::block(
                json!({ "created": block_json(&block) }),
                "blockCreated",
                format!("Made “{}” — {} listening block", block.title, complexity_label(&block.complexity)),
                block.id,
            ))
        }
        "list_blocks" => {
            let args: ListBlocksArgs = parse(arguments)?;
            let list = match args.which.as_str() {
                "due" => blocks::overview(conn)?.due,
                "course" | "topic" => blocks::for_topic(
                    conn,
                    args.topic_id.ok_or_else(|| Error::msg("Give a course_id."))?,
                )?,
                _ => {
                    let ids: Vec<i64> = blocks_table::table
                        .filter(blocks_table::kind.eq("study"))
                        .order((blocks_table::ord, blocks_table::id))
                        .select(blocks_table::id)
                        .load(conn)?;
                    ids.into_iter().map(|id| blocks::load(conn, id)).collect::<Result<_>>()?
                }
            };
            Ok(Outcome::data(Value::Array(list.iter().map(block_json).collect())))
        }
        "get_block" => {
            let args: BlockArg = parse(arguments)?;
            let detail = blocks::detail(conn, args.block_id)?;
            Ok(Outcome::data(json!({
                "block": block_json(&detail.block),
                "lesson": detail.lesson,
                "objectives": detail.objectives,
                "words": detail.words.iter().map(|(pl, en)| json!({ "pl": pl, "en": en })).collect::<Vec<_>>(),
                "sentences": detail.sentences.iter().map(|(pl, en)| json!({ "pl": pl, "en": en })).collect::<Vec<_>>(),
                "last_checkin_feedback": detail.last_feedback,
            })))
        }
        "update_block" => {
            let args: UpdateBlockArgs = parse(arguments)?;
            let block = blocks::update(conn, args.block_id, args.changes)?;
            Ok(Outcome::block(
                json!({ "updated": block_json(&block) }),
                "blockUpdated",
                format!("Updated “{}”", block.title),
                block.id,
            ))
        }
        "delete_block" => {
            let args: BlockArg = parse(arguments)?;
            let title = blocks::delete(conn, args.block_id)?;
            Ok(Outcome::acted(json!({ "deleted": title }), "blockDeleted", format!("Deleted “{title}”"), None))
        }
        "add_block_to_course" => {
            let args: MembershipArgs = parse(arguments)?;
            courses::add_block(conn, args.course_id, args.block_id)?;
            let block = blocks::load(conn, args.block_id)?;
            let course = courses::require_course(conn, args.course_id)?;
            Ok(Outcome::acted(
                json!({ "block": block_json(&block) }),
                "courseUpdated",
                format!("“{}” is now also in “{course}”", block.title),
                Some(args.course_id),
            ))
        }
        "remove_block_from_course" => {
            let args: MembershipArgs = parse(arguments)?;
            let title = blocks::load(conn, args.block_id)?.title;
            let course = courses::require_course(conn, args.course_id)?;
            let deleted = courses::remove_block(conn, args.course_id, args.block_id)?;
            Ok(Outcome::acted(
                json!({ "removed": title, "block_deleted": deleted }),
                if deleted { "blockDeleted" } else { "courseUpdated" },
                if deleted {
                    format!("Deleted “{title}” (it was only in “{course}”)")
                } else {
                    format!("Took “{title}” out of “{course}”")
                },
                if deleted { None } else { Some(args.course_id) },
            ))
        }
        "move_block" => {
            let args: MoveArgs = parse(arguments)?;
            courses::move_block(conn, args.block_id, args.from_course_id, args.to_course_id)?;
            let block = blocks::load(conn, args.block_id)?;
            let course = courses::require_course(conn, args.to_course_id)?;
            Ok(Outcome::acted(
                json!({ "block": block_json(&block) }),
                "courseUpdated",
                format!("Moved “{}” to “{course}”", block.title),
                Some(args.to_course_id),
            ))
        }
        "order_course_blocks" => {
            let args: OrderArgs = parse(arguments)?;
            courses::order_blocks(conn, args.course_id, &args.block_ids)?;
            let course = courses::require_course(conn, args.course_id)?;
            Ok(Outcome::acted(
                json!({ "order": blocks::for_topic(conn, args.course_id)?.iter().map(|b| b.id).collect::<Vec<_>>() }),
                "courseUpdated",
                format!("Reordered “{course}”"),
                Some(args.course_id),
            ))
        }
        "schedule_block" => {
            let args: BlockArg = parse(arguments)?;
            let block = blocks::due_now(conn, args.block_id)?;
            Ok(Outcome::block(
                json!({ "due_now": block.title }),
                "blockScheduled",
                format!("“{}” is due now", block.title),
                block.id,
            ))
        }
        "suggest_block" => {
            let args: BlockArg = parse(arguments)?;
            let block = blocks::load(conn, args.block_id)?;
            Ok(Outcome::block(
                json!({ "shown_button_for": block.title }),
                "blockSuggested",
                format!("{} · {}", block.title, complexity_label(&block.complexity)),
                block.id,
            ))
        }
        "update_practice_settings" => {
            let args: SettingsArgs = parse(arguments)?;
            let mut changed = Vec::new();
            if let Some(goal) = args.daily_goal {
                let goal = goal.clamp(10, 1000);
                db::set_setting(conn, "daily_goal", &goal.to_string())?;
                changed.push(format!("daily goal {goal} XP"));
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
        "list_playlists" => {
            let list = playlists::list(conn)?
                .iter()
                .map(|p| playlist_json(conn, p))
                .collect::<Result<Vec<_>>>()?;
            Ok(Outcome::data(Value::Array(list)))
        }
        "create_playlist" => {
            let args: PlaylistArgs = parse(arguments)?;
            let name = args.name.clone().unwrap_or_default();
            // Check the songs before making anything, so a bad id makes nothing.
            for song in &args.song_ids {
                playlists::preferred_track(conn, *song)?;
            }
            let playlist = playlists::create(conn, &name, &[])?;
            let (added, _) = playlists::add_songs(conn, playlist.id, &args.song_ids)?;
            let label = if added.is_empty() {
                format!("Made the playlist “{}”", playlist.name)
            } else {
                format!("Made the playlist “{}” with {}", playlist.name, titles(&added))
            };
            let playlist = playlists::list(conn)?.into_iter().find(|p| p.id == playlist.id).unwrap_or(playlist);
            Ok(Outcome::acted(playlist_json(conn, &playlist)?, "playlistChanged", label, None))
        }
        "add_to_playlist" => {
            let args: PlaylistArgs = parse(arguments)?;
            let id = args.id()?;
            let (added, skipped) = playlists::add_songs(conn, id, &args.song_ids)?;
            let playlist = playlists::list(conn)?
                .into_iter()
                .find(|p| p.id == id)
                .ok_or_else(|| Error::msg("That playlist no longer exists."))?;
            let mut label = if added.is_empty() {
                format!("Nothing new for “{}”", playlist.name)
            } else {
                format!("Added {} to “{}”", titles(&added), playlist.name)
            };
            if !skipped.is_empty() {
                label.push_str(&format!(" ({} already in it)", titles(&skipped)));
            }
            let mut result = playlist_json(conn, &playlist)?;
            result["already_in_it"] = json!(skipped);
            Ok(Outcome::acted(result, "playlistChanged", label, None))
        }
        "remove_from_playlist" => {
            let args: PlaylistArgs = parse(arguments)?;
            let id = args.id()?;
            let removed = playlists::remove_songs(conn, id, &args.song_ids)?;
            let playlist = playlists::list(conn)?
                .into_iter()
                .find(|p| p.id == id)
                .ok_or_else(|| Error::msg("That playlist no longer exists."))?;
            let mut result = playlist_json(conn, &playlist)?;
            result["entries_removed"] = json!(removed);
            Ok(Outcome::acted(
                result,
                "playlistChanged",
                format!("Took {removed} song{} out of “{}”", if removed == 1 { "" } else { "s" }, playlist.name),
                None,
            ))
        }
        "rename_playlist" => {
            let args: PlaylistArgs = parse(arguments)?;
            let playlist = playlists::rename(conn, args.id()?, args.name.as_deref().unwrap_or_default())?;
            let label = format!("Renamed the playlist to “{}”", playlist.name);
            Ok(Outcome::acted(playlist_json(conn, &playlist)?, "playlistChanged", label, None))
        }
        "delete_playlist" => {
            let args: PlaylistArgs = parse(arguments)?;
            let name = playlists::delete(conn, args.id()?)?;
            Ok(Outcome::acted(
                json!({ "deleted": name }),
                "playlistChanged",
                format!("Deleted the playlist “{name}”"),
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
                        "style": song.style,
                        "singer": singer(&song.vocal_gender),
                        "about": song.origin.brief,
                        "status": song.status,
                        "takes": song.tracks.len(),
                        "plays": song.tracks.iter().map(|t| t.play_count).sum::<i64>(),
                        "filed": song_placement(conn, song.id).unwrap_or(Value::Null),
                    })
                })
                .collect();
            Ok(Outcome::data(Value::Array(list)))
        }
        "get_song" => {
            let args: SongArg = parse(arguments)?;
            let song = songs::load_song(conn, args.song_id)?;
            let lyrics: Vec<Value> = song
                .sections
                .iter()
                .map(|section| {
                    json!({
                        "section": section.kind,
                        "lines": section.lines.iter().map(|l| json!({ "pl": l.pl, "en": l.en })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let (y, m, d) = crate::commands::civil_date(song.created_at.div_euclid(86_400));
            let made_on = format!("{y}-{m:02}-{d:02}");
            let takes: Vec<Value> =
                song.tracks.iter().map(|t| json!({ "seconds": t.duration.round(), "plays": t.play_count })).collect();
            let made_with = json!({
                "style": song.style,
                "singer": singer(&song.vocal_gender),
                "suno_model": song.suno_model,
                "about": song.origin.brief,
                "asked_for_style": song.origin.requested_style,
                "asked_for_singer": singer(&song.origin.requested_vocal_gender),
                "asked_for_chorus": song.origin.requested_chorus,
                "length": song.origin.length,
                "songwriter_model": song.origin.songwriter_model,
                "revisions": song.origin.revisions,
                "made_on": made_on,
            });
            Ok(Outcome::data(json!({
                "song_id": song.id,
                "title": song.title,
                "chorus_pl": song.chorus_pl,
                "chorus_en": song.chorus_en,
                "status": song.status,
                "failed_because": song.error,
                "lyrics": lyrics,
                "filed": song_placement(conn, song.id)?,
                // Everything that went into making it, so a new song can match
                // or vary it.
                "made_with": made_with,
                "takes": takes,
            })))
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
        other => Err(Error::msg(format!("There is no tool called '{other}'."))),
    }
}

fn list_topics(conn: &mut Conn) -> Result<Value> {
    let topics = stats::topic_progress(conn)?
        .into_iter()
        .map(|p| {
            json!({
                "course_id": p.topic.id,
                "title": p.topic.title,
                "title_pl": p.topic.title_pl,
                "words": p.total_words,
                "words_met": p.seen_words,
                "words_mastered": p.mastered_words,
                "sentences": p.total_sentences,
                "blocks": blocks::for_topic(conn, p.topic.id).map(|b| b.len()).unwrap_or(0),
            })
        })
        .collect();
    Ok(Value::Array(topics))
}

fn topic_detail(conn: &mut Conn, topic_id: i64) -> Result<Value> {
    let topic = session::load_topic(conn, topic_id)?;
    let rows: Vec<(i64, String, String, String, i64)> = course_words::table
        .filter(course_words::topic_id.eq(topic_id))
        .order(course_words::ord)
        .select((course_words::id, course_words::pl, course_words::en, course_words::pos, course_words::block_id))
        .load(conn)?;
    let history: std::collections::HashMap<i64, (i64, i64)> = reviews::table
        .filter(reviews::kind.eq("word"))
        .filter(reviews::item_id.eq_any(rows.iter().map(|r| r.0).collect::<Vec<_>>()))
        .select((reviews::item_id, reviews::reps, reviews::lapses))
        .load::<(i64, i64, i64)>(conn)?
        .into_iter()
        .map(|(id, reps, lapses)| (id, (reps, lapses)))
        .collect();
    let words: Vec<Value> = rows
        .into_iter()
        .map(|(id, pl, en, pos, block_id)| {
            let seen = history.get(&id);
            json!({
                "pl": pl,
                "en": en,
                "pos": pos,
                "met": seen.is_some(),
                "correct_reps": seen.map_or(0, |s| s.0),
                "lapses": seen.map_or(0, |s| s.1),
                "block_id": block_id,
            })
        })
        .collect();
    let sentences: Vec<Value> = course_sentences::table
        .filter(course_sentences::topic_id.eq(topic_id))
        .order(course_sentences::ord)
        .select((course_sentences::pl, course_sentences::en, course_sentences::block_id))
        .load::<(String, String, i64)>(conn)?
        .into_iter()
        .map(|(pl, en, block_id)| json!({ "pl": pl, "en": en, "block_id": block_id }))
        .collect();
    let dialogue: Vec<Value> = session::dialogue_lines(conn, topic_id)?
        .into_iter()
        .map(|l| json!({ "speaker": l.speaker, "pl": l.pl, "en": l.meaning }))
        .collect();
    let blocks: Vec<Value> = blocks::for_topic(conn, topic_id)?.iter().map(block_json).collect();
    Ok(json!({
        "course_id": topic.id,
        "title": topic.title,
        "title_pl": topic.title_pl,
        "summary": topic.summary,
        "words": words,
        "sentences": sentences,
        "dialogue": dialogue,
        "blocks": blocks,
    }))
}

fn search(conn: &mut Conn, query: &str) -> Result<Value> {
    let pattern = format!("%{}%", query.trim());
    let met: HashSet<i64> = reviews::table
        .filter(reviews::kind.eq("word"))
        .select(reviews::item_id)
        .load::<i64>(conn)?
        .into_iter()
        .collect();
    let mut seen = HashSet::new();
    let words: Vec<Value> = course_words::table
        .inner_join(topics::table)
        .filter(course_words::pl.like(&pattern).or(course_words::en.like(&pattern)))
        .order((topics::ord, course_words::ord))
        .select((
            course_words::id,
            course_words::pl,
            course_words::en,
            course_words::pos,
            topics::title_en,
            topics::id,
            course_words::block_id,
        ))
        .load::<(i64, String, String, String, String, i64, i64)>(conn)?
        .into_iter()
        // A word in a block two courses list is one word.
        .filter(|row| seen.insert(row.0))
        .take(40)
        .map(|(id, pl, en, pos, topic, course_id, block_id)| {
            json!({
                "pl": pl, "en": en, "pos": pos, "topic": topic, "course_id": course_id,
                "met": met.contains(&id), "block_id": block_id,
            })
        })
        .collect();
    let mut seen = HashSet::new();
    let sentences: Vec<Value> = course_sentences::table
        .inner_join(topics::table)
        .filter(course_sentences::pl.like(&pattern).or(course_sentences::en.like(&pattern)))
        .select((course_sentences::id, course_sentences::pl, course_sentences::en, topics::title_en))
        .load::<(i64, String, String, String)>(conn)?
        .into_iter()
        .filter(|row| seen.insert(row.0))
        .take(15)
        .map(|(_, pl, en, topic)| json!({ "pl": pl, "en": en, "topic": topic }))
        .collect();
    Ok(json!({ "words": words, "sentences": sentences }))
}

fn progress(conn: &mut Conn) -> Result<Value> {
    let s = stats::collect(conn)?;
    let hardest: Vec<Value> = words::table
        .inner_join(reviews::table.on(reviews::item_id.eq(words::id).and(reviews::kind.eq("word"))))
        .filter(reviews::lapses.gt(0))
        .order((reviews::lapses.desc(), reviews::ease.asc()))
        .limit(15)
        .select((words::pl, words::en, reviews::lapses, reviews::reps))
        .load::<(String, String, i64, i64)>(conn)?
        .into_iter()
        .map(|(pl, en, wrong, streak)| json!({ "pl": pl, "en": en, "times_wrong": wrong, "correct_streak": streak }))
        .collect();
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
        "blocks_due_now": s.due_today,
        "recent_checkins": recent_checkins(conn)?,
        "achievements_unlocked": s.achievements.iter().filter(|a| a.unlocked).map(|a| a.title.clone()).collect::<Vec<_>>(),
    }))
}

/// make_song needs the network twice (the songwriter, then Suno), so unlike
/// the other tools it cannot run under the database lock.
async fn make_song(app: &tauri::AppHandle, db: &Db, arguments: &str) -> Outcome {
    let result: Result<Outcome> = async {
        let args: SongArgs = parse(arguments)?;
        // Fail before paying for lyrics if Suno cannot be reached anyway.
        songs::kie_key(&mut db.lock())?;
        let draft = songs::write_draft(
            db,
            songs::DraftRequest {
                brief: args.about,
                topic_id: args.topic_id,
                chorus_pl: args.chorus_pl,
                style: args.style,
                vocal_gender: args.vocal_gender,
                length: args.length,
                ..Default::default()
            },
        )
        .await?;
        let title = draft.title.clone();
        let chorus = format!("{} — {}", draft.chorus_pl, draft.chorus_en);
        let listen = args.listening_block;
        let song_id = songs::start_song(app, db, draft)?;
        let block = if listen {
            Some(blocks::create_listening(&mut db.lock(), song_id, None, None, args.topic_id, "tutor")?)
        } else {
            None
        };
        Ok(Outcome::song(
            json!({
                "song_id": song_id,
                "title": title,
                "chorus": chorus,
                "status": "Suno is singing it now; ready in 2–4 minutes",
                "listening_block_id": block.as_ref().map(|b| b.id),
            }),
            "songStarted",
            format!(
                "Writing “{title}” — Suno is singing it now{}",
                if block.is_some() { ", with a listening block" } else { "" }
            ),
            Some(song_id),
        ))
    }
    .await;
    result.unwrap_or_else(|error| Outcome::data(json!({ "error": error.to_string() })))
}

/// The last study check-ins, with the examiner's notes, so the tutor can
/// follow up on what the learner found hard.
fn recent_checkins(conn: &mut Conn) -> Result<Value> {
    let rows: Vec<Value> = checkins::table
        .inner_join(blocks_table::table)
        .filter(checkins::finished_at.is_not_null())
        .order(checkins::finished_at.desc())
        .limit(8)
        .select((blocks_table::id, blocks_table::title, checkins::result, checkins::feedback))
        .load::<(i64, String, Option<String>, Option<String>)>(conn)?
        .into_iter()
        .map(|(id, title, result, feedback)| {
            json!({ "block_id": id, "block": title, "result": result, "examiner_said": feedback })
        })
        .collect();
    Ok(Value::Array(rows))
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

fn parse_actions(json: Option<String>) -> Vec<TutorAction> {
    json.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

pub fn load_history(conn: &mut Conn, conversation_id: i64) -> Result<Vec<ChatMessage>> {
    Ok(tutor_messages::table
        .filter(tutor_messages::conversation_id.eq(conversation_id))
        .order(tutor_messages::id)
        .select((tutor_messages::id, tutor_messages::role, tutor_messages::content, tutor_messages::actions))
        .load::<(i64, String, String, Option<String>)>(conn)?
        .into_iter()
        .map(|(id, role, content, actions)| ChatMessage {
            id,
            conversation_id,
            role,
            content,
            actions: parse_actions(actions),
        })
        .collect())
}

/// The recent conversation as the model should see it. What the tutor did in
/// earlier turns is spelled out, so it knows the ids of what it made.
fn recent_history(conn: &mut Conn, conversation_id: i64) -> Result<Vec<Wire>> {
    let mut rows: Vec<(String, String, Option<String>)> = tutor_messages::table
        .filter(tutor_messages::conversation_id.eq(conversation_id))
        .order(tutor_messages::id.desc())
        .limit(HISTORY_LIMIT)
        .select((tutor_messages::role, tutor_messages::content, tutor_messages::actions))
        .load(conn)?;
    rows.reverse();
    Ok(rows
        .into_iter()
        .map(|(role, mut content, actions)| {
            let actions = parse_actions(actions);
            if !actions.is_empty() {
                let notes: Vec<String> = actions
                    .iter()
                    .map(|a| match (a.topic_id, a.song_id) {
                        (Some(id), _) => format!("{} (topic id {id})", a.label),
                        (_, Some(id)) => format!("{} (song id {id})", a.label),
                        _ => a.label.clone(),
                    })
                    .collect();
                content.push_str(&format!("\n\n[Done in the app: {}]", notes.join("; ")));
            }
            if role == "user" { Wire::user(content) } else { Wire::assistant(content) }
        })
        .collect())
}

fn append(
    conn: &mut Conn,
    conversation_id: i64,
    role: &str,
    content: &str,
    actions: &[TutorAction],
) -> Result<i64> {
    let actions = if actions.is_empty() { None } else { Some(serde_json::to_string(actions)?) };
    let now = db::now_secs();
    let id = diesel::insert_into(tutor_messages::table)
        .values((
            tutor_messages::conversation_id.eq(conversation_id),
            tutor_messages::role.eq(role),
            tutor_messages::content.eq(content),
            tutor_messages::actions.eq(actions),
            tutor_messages::created_at.eq(now),
        ))
        .returning(tutor_messages::id)
        .get_result(conn)?;
    diesel::update(tutor_conversations::table.find(conversation_id))
        .set(tutor_conversations::updated_at.eq(now))
        .execute(conn)?;
    Ok(id)
}

/// A conversation's title: its first question, cut at a word near 48 characters.
pub fn title_from(message: &str) -> String {
    let text = crate::text::clean(message);
    if text.chars().count() <= 48 {
        return text;
    }
    let cut: String = text.chars().take(48).collect();
    let at_word = cut.rfind(' ').filter(|&i| i > 24).map_or(cut.as_str(), |i| &cut[..i]);
    format!("{}…", at_word.trim_end_matches([',', '.', ';', ':']))
}

pub fn create_conversation(conn: &mut Conn, first_message: &str) -> Result<i64> {
    let now = db::now_secs();
    Ok(diesel::insert_into(tutor_conversations::table)
        .values((
            tutor_conversations::title.eq(title_from(first_message)),
            tutor_conversations::created_at.eq(now),
            tutor_conversations::updated_at.eq(now),
        ))
        .returning(tutor_conversations::id)
        .get_result(conn)?)
}

pub fn list_conversations(conn: &mut Conn) -> Result<Vec<Conversation>> {
    let heads: Vec<(i64, String, i64)> = tutor_conversations::table
        .order((tutor_conversations::updated_at.desc(), tutor_conversations::id.desc()))
        .select((tutor_conversations::id, tutor_conversations::title, tutor_conversations::updated_at))
        .load(conn)?;
    heads
        .into_iter()
        .map(|(id, title, updated_at)| {
            let of = tutor_messages::table.filter(tutor_messages::conversation_id.eq(id));
            let messages = of.count().get_result(conn)?;
            let last: String = of
                .order(tutor_messages::id.desc())
                .select(tutor_messages::content)
                .first(conn)
                .optional()?
                .unwrap_or_default();
            Ok(Conversation {
                id,
                title,
                updated_at,
                messages,
                // Markdown marks would show as stray symbols in a one-line preview.
                preview: crate::text::clean(&last.replace(['*', '_', '#', '`'], "")).chars().take(120).collect(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn tutor_conversations(db: State<'_, Db>) -> Result<Vec<Conversation>> {
    list_conversations(&mut db.lock())
}

#[tauri::command]
pub async fn tutor_history(db: State<'_, Db>, conversation_id: i64) -> Result<Vec<ChatMessage>> {
    load_history(&mut db.lock(), conversation_id)
}

#[tauri::command]
pub async fn tutor_rename(db: State<'_, Db>, conversation_id: i64, title: String) -> Result<()> {
    let title = crate::text::clean(&title);
    if title.is_empty() {
        return Err(Error::msg("A conversation needs a title."));
    }
    diesel::update(tutor_conversations::table.find(conversation_id))
        .set(tutor_conversations::title.eq(title))
        .execute(&mut *db.lock())?;
    Ok(())
}

/// Deletes one conversation and its messages. What the tutor made in the app
/// (courses, blocks, songs) stays.
#[tauri::command]
pub async fn tutor_delete(db: State<'_, Db>, conversation_id: i64) -> Result<()> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    diesel::delete(tutor_messages::table.filter(tutor_messages::conversation_id.eq(conversation_id))).execute(conn)?;
    diesel::delete(tutor_conversations::table.find(conversation_id)).execute(conn)?;
    Ok(())
}

/// Sends a message to the tutor. Text streams out as `ai://stream` events with
/// the given id; while a tool runs, an `agent://status` event says what it is
/// doing. Returns the stored reply, with the actions taken.
#[tauri::command]
pub async fn tutor_send(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    message: String,
    stream_id: String,
    // None starts a new conversation, titled after this message.
    conversation_id: Option<i64>,
) -> Result<ChatMessage> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err(Error::msg("Write something first."));
    }
    let db: &Db = &db;
    let wire = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let mut wire = vec![Wire::system(system_prompt(conn)?)];
        if let Some(id) = conversation_id {
            wire.extend(recent_history(conn, id)?);
        }
        wire.push(Wire::user(message.clone()));
        wire
    };

    let agent = Agent {
        app: &app,
        db,
        name: crate::costs::TUTOR,
        stream_id: &stream_id,
        tools: Some(tool_definitions()),
        max_rounds: MAX_ROUNDS,
        stop_once_answered: false,
        show_status: true,
        model: None,
    };
    let app_ref = &app;
    let outcome = agent
        .run(wire, |call| {
            Box::pin(async move {
                let done = if call.function.name == "make_song" {
                    // The network twice (songwriter, then Suno): not under the lock.
                    make_song(app_ref, db, &call.function.arguments).await
                } else {
                    let mut guard = db.lock();
                    let conn = &mut *guard;
                    execute(conn, &call.function.name, &call.function.arguments)
                };
                ToolOutput { result: done.result, extra: done.action, ends_turn: false }
            })
        })
        .await;

    let mut actions = outcome.extras;
    actions.extend(outcome.remembered.into_iter().map(|label| TutorAction {
        kind: "memory".into(),
        label,
        topic_id: None,
        song_id: None,
        block_id: None,
    }));
    let mut reply = outcome.text;
    // Even when the conversation fails half-way, whatever the tools already
    // did in the app is real, so it is recorded and shown.
    if let Some(error) = outcome.failed {
        if actions.is_empty() {
            return Err(error);
        }
        reply.push_str(&format!("\n\n_(The answer was cut short: {error})_"));
    }
    if reply.trim().is_empty() {
        reply = "Done.".into();
    }
    // The conversation is only created once there is a reply worth keeping.
    let mut guard = db.lock();
    let conn = &mut *guard;
    let conversation_id = match conversation_id {
        Some(id) => id,
        None => create_conversation(conn, &message)?,
    };
    append(conn, conversation_id, "user", &message, &[])?;
    let id = append(conn, conversation_id, "assistant", &reply, &actions)?;
    Ok(ChatMessage { id, conversation_id, role: "assistant".into(), content: reply, actions })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_valid_definition() {
        let mut tools = tool_definitions();
        tools.extend(crate::memories::tools());
        assert!(tools.len() >= 8);
        let mut names = std::collections::HashSet::new();
        for tool in tools {
            assert_eq!(tool.definition["type"], "function");
            assert_eq!(tool.definition["function"]["name"], tool.name);
            assert_eq!(tool.definition["function"]["parameters"]["type"], "object", "{}", tool.name);
            assert!(tool.label.ends_with('…') && tool.label != "Working…", "{} has no label of its own", tool.name);
            assert!(names.insert(tool.name), "{} is declared twice", tool.name);
        }
    }

    #[test]
    fn conversations_are_titled_by_their_first_question() {
        assert_eq!(title_from("  Build me a   programme for colours "), "Build me a programme for colours");
        let long = title_from("Can you explain the difference between the instrumental and the locative case, with examples?");
        assert!(long.ends_with('…'), "{long}");
        assert!(long.chars().count() <= 49, "{long}");
        assert!(!long.contains("locat…"), "cut at a word: {long}");
    }

    pub(crate) fn run(conn: &mut Conn, name: &str, args: Value) -> Value {
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
