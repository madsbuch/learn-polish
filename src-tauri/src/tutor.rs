//! The tutor: an assistant for the whole app, not a Polish role-play partner.
//!
//! It talks to the learner in English and can act in the app through tools —
//! build a course on any theme, write lessons and drills into courses, look up
//! progress and vocabulary, make songs, adjust practice settings. Every
//! tool runs here, in Rust, against the same SQLite database the rest of the
//! app uses, so what the tutor makes is immediately practisable.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, Manager, State};

use crate::agent::{tool, Agent, Tool, ToolOutput};
use crate::ai::Wire;
use crate::lessons;
use crate::db::{self, Conn, Db};
use crate::schema::{
    lessons as lessons_table, checkins, course_lessons, course_sentences, course_words, reviews, songs as songs_table, courses as courses_table,
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

pub(crate) const RULES: &str = "\
You are the tutor in Polski, an app for learning Polish. You explain Polish, suggest what to \
practise, and — when asked — change the app with your tools.

Talking:
- Reply in the language the learner writes in (English, Danish, …). Polish examples always come \
  with a translation into that language. Hold a conversation in Polish only when asked to.
- Talk like a tutor at a table, not a textbook: short turns — a few sentences, one idea, a \
  couple of examples — then hand the conversation back.
- Help the learner find what they are really asking. When a question is broad (\"explain the \
  question words\"), give the gist in two or three lines and ask what prompted it or which part \
  they want — they often have one word or sentence in mind. Follow their lead from there.
- Save the full explanation (tables, every form, many examples) for a lesson: offer a study \
  lesson when the learner wants it all in one place.
- Be friendly. Markdown is rendered, and so is TeX math: $3 + 5 = 8$ inline, $$…$$ on a line \
  of its own (\\times, \\div, \\frac work) — in replies and in lessons.

Acting:
- A question gets an answer, not an action. Explaining something is not a request to save it.
- Change the app only when the learner asks you to make, change or delete something. Then do it \
  straight away, without asking first, and say in a sentence or two what you did.
- If something you explained would make a good lesson or course, offer it in one line and wait.
- Deleting (courses, lessons, songs, playlists) is permanent: name what goes and wait for a yes.
- Look before you judge: get_progress, list_lessons and get_lesson show how the learner is doing; \
  never invent numbers.

The app:
- Courses list lessons. A lesson has one job: a drill (practice on a few words, sentences or a \
  dialogue it owns), a study lesson (a short text, checked by an examiner's interview), a listening \
  lesson (a song, then questions), or a writing lesson (a situation written out in Polish with a \
  partner who understands only Polish; goals are things to get across, not grammar).
- Complexity is how hard the subject is: low (1–2 min, one small idea), medium (5–10 min, a \
  pattern with variations), high (20+ min, a system like the cases). Split big subjects into \
  several lessons instead of stuffing one.
- A new course is a small learning path, made in one create_course call: an 8–12 line dialogue \
  to hear the theme; 15–30 words and 8–12 short sentences; one or two short study lessons on the \
  grammar the dialogue uses; and a writing lesson where the learner plays out the course's \
  situation. The app orders and varies it. Afterwards, offer a song — don't make one unasked. \
  To grow a course, add to it rather than making a new one.
- Courses are made to be finished: the learner completes one by passing each lesson once, and \
  masters it when everything holds at long gaps (writing done without help). Keep the core at \
  the learner's level; a lesson clearly beyond it (a free conversation in a beginner course, a \
  deep lesson) is a challenge — set challenge with update_lesson. Challenges count for mastering, \
  not completing. Writing lessons start guided (word bank, hints) and step up as they pass.
- A mastered course can be closed by the learner: it becomes a trophy and its lessons leave the \
  check-ins. Closed courses are finished — never add to or change them. To revisit a closed \
  course's theme, make a new course a level up: build on what it taught (get_course shows it), \
  don't repeat its words and lessons. Grammar & culture, Listening and Writing never end.
- Culture and facts (cities, history, customs): write the study lesson from Wikipedia — wiki_search, \
  wiki_read — and attach the article to the lesson with attach_source (existing lessons too). \
  Keep to lasting facts; name current officeholders only \"as of <year>\". Give the lesson a \
  language hook (the city in \"w Krakowie\", \"z Krakowa\").
- Polish must be correct, with diacritics. Words in dictionary form, lowercase, with a short \
  gloss; sentences short and natural.
- Songs cost the learner credits: one per request. File a new song under the course it fits; \
  to sort older ones, read their lyrics with get_song and move their lesson with move_lesson.
- The Music tab also holds podcasts (make_podcast): short spoken episodes on a subject the \
  learner likes, written from Wikipedia articles (0–5) or as a made-up story, told by a narrator, \
  two hosts or a cast of characters — each with a voice of their own. Two kinds: pairs \
  (each sentence in English, then in Polish) and immersion (only Polish, simpler sentences). \
  set_podcast_sound changes the pause before the Polish, repeats, slow Polish, voices and whether \
  the English is said. They need only the OpenRouter key, not kie.ai. list_songs and get_song \
  cover them too (kind \"podcast\"); get_song gives the articles they were written from.";

/// The whole prompt in one string, for tests.
#[cfg(test)]
pub(crate) fn system_prompt(conn: &mut Conn) -> Result<String> {
    Ok(format!("{RULES}\n\n{}", learner_state(conn)?))
}

/// What changes from turn to turn: the learner's numbers, what is due, the
/// courses and lessons, songs, memories. Sent with the newest message rather
/// than in the system prompt, so the prompt and the conversation before it
/// stay the same from turn to turn and can be cached (cheaper, faster).
pub(crate) fn learner_state(conn: &mut Conn) -> Result<String> {
    let stats = stats::collect(conn)?;
    let overview = lessons::overview(conn)?;
    let line = |b: &lessons::Lesson| {
        format!(
            "  - id {}: {} ({} {}, {}){}",
            b.id,
            b.title,
            b.complexity,
            b.kind,
            b.course_title.clone().unwrap_or_default(),
            b.last_result.as_ref().map(|r| format!(", last check-in: {r}")).unwrap_or_default()
        )
    };
    let due: Vec<String> = overview.due.iter().take(15).map(line).collect();

    let listed: Vec<i64> = course_lessons::table.select(course_lessons::course_id).load(conn)?;
    let all: Vec<(i64, String, String, Option<i64>)> = courses_table::table
        .order(crate::lessons::course_order())
        .limit(80)
        .select((courses_table::id, courses_table::title_en, courses_table::title_pl, courses_table::closed_at))
        .load(conn)?;
    let courses: Vec<String> = all
        .iter()
        .filter(|c| c.3.is_none())
        .map(|(id, title, title_pl, _)| {
            let lessons = listed.iter().filter(|&&c| c == *id).count();
            format!("  - id {id}: {title} ({title_pl}), {lessons} lessons")
        })
        .collect();
    // Closed as mastered: finished for good, kept as trophies.
    let mastered: Vec<String> = all
        .iter()
        .filter_map(|(id, title, title_pl, closed)| {
            closed.map(|at| {
                let (y, m, d) = crate::commands::civil_date(at.div_euclid(86_400));
                format!("  - id {id}: {title} ({title_pl}), closed {y}-{m:02}-{d:02}")
            })
        })
        .collect();

    // Every study lesson, with the courses that list it.
    let mut studies: Vec<(i64, String, String, i64, Vec<(i64, i64, String)>)> = Vec::new();
    for (id, title, complexity, reps, course_ord, list_ord, course) in course_lessons::table
        .inner_join(lessons_table::table)
        .inner_join(courses_table::table)
        .filter(lessons_table::kind.eq("study"))
        .order((courses_table::ord, course_lessons::ord))
        .select((
            lessons_table::id,
            lessons_table::title,
            lessons_table::complexity,
            lessons_table::reps,
            courses_table::ord,
            course_lessons::ord,
            courses_table::title_en,
        ))
        .load::<(i64, String, String, i64, i64, i64, String)>(conn)?
    {
        match studies.iter_mut().find(|l| l.0 == id) {
            Some(entry) => entry.4.push((course_ord, list_ord, course)),
            None => studies.push((id, title, complexity, reps, vec![(course_ord, list_ord, course)])),
        }
    }
    let studies: Vec<String> = studies
        .into_iter()
        .map(|(id, title, complexity, reps, courses)| {
            let names: Vec<String> = courses.into_iter().map(|c| c.2).collect();
            let studied = if reps > 0 { ", studied" } else { "" };
            format!("  - id {id}: {title} ({complexity}, in {}{studied})", names.join(", "))
        })
        .collect();

    let song_count: i64 = songs_table::table.count().get_result(conn)?;
    let unsorted: i64 = course_lessons::table
        .inner_join(courses_table::table)
        .filter(courses_table::slug.eq("listening"))
        .count()
        .get_result(conn)?;
    let playlists = playlists::list(conn)?;

    let none = || "  (none)".to_string();
    Ok(format!(
        "The learner right now:\n\
         - Level {level}, {xp} XP, streak {streak} days{done}, {seen} of {total} words met.\n\
         - Due now ({due_count}):\n{due}\n\
         - Study lessons:\n{studies}\n\
         - Courses:\n{courses}\n\
         - Mastered and closed (finished; never add to these):\n{mastered}\n\
         - Songs and podcasts: {song_count} in the Music tab{song_note}{unsorted}.\n\
         - Playlists: {playlists}.",
        level = stats.level,
        xp = stats.total_xp,
        streak = stats.current_streak,
        done = if stats.completed_today { " (practised today)" } else { "" },
        seen = stats.words_seen,
        total = stats.words_total,
        due_count = overview.due.len(),
        due = if due.is_empty() { none() } else { due.join("\n") },
        studies = if studies.is_empty() { none() } else { studies.join("\n") },
        courses = if courses.is_empty() { none() } else { courses.join("\n") },
        mastered = if mastered.is_empty() { none() } else { mastered.join("\n") },
        song_count = song_count,
        unsorted = match unsorted {
            0 => String::new(),
            n => format!(
                "; {n} listening lesson{} still in \"Listening\" with no course — when it fits the \
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
            "; making songs needs a kie.ai key in Settings, which is missing (podcasts do not)"
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
            "Create a new course on a theme: a small learning path. The words, sentences and \
             dialogue are cut into drills (some word batches are typed, for variety); the lessons \
             and the writing lesson join them; the app orders it all — conversation, words, \
             lessons and sentences interleaved, writing last. It can be practised straight away.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short English title, e.g. \"Colours\"" },
                    "title_pl": { "type": "string", "description": "The title in Polish, e.g. \"Kolory\"" },
                    "summary": { "type": "string", "description": "One English sentence on what the course covers" },
                    "words": word_schema(),
                    "sentences": sentence_schema(),
                    "dialogue": dialogue_schema(),
                    "study_lessons": {
                        "type": "array",
                        "description": "1–2 short lessons on the grammar the dialogue and sentences use (a case ending, a verb form), each a study lesson checked by the examiner",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": { "type": "string" },
                                "summary": { "type": "string", "description": "One sentence" },
                                "complexity": { "type": "string", "enum": ["low", "medium", "high"] },
                                "content": { "type": "string", "description": "The lesson in markdown: short, with a small table or examples. Write every Polish example as [[say: …]] so it can be heard, its translation outside the tag." },
                                "objectives": { "type": "array", "items": { "type": "string" }, "description": "2–4 things the learner can then do" }
                            },
                            "required": ["title", "summary", "complexity", "content", "objectives"]
                        }
                    },
                    "writing": {
                        "type": "object",
                        "description": "The course's situation, played out in writing with a partner who only understands Polish — the last step",
                        "properties": {
                            "title": { "type": "string" },
                            "summary": { "type": "string" },
                            "complexity": { "type": "string", "enum": ["low", "medium", "high"] },
                            "scenario": { "type": "string", "description": "The situation, in English, to the learner" },
                            "goals": { "type": "array", "items": { "type": "string" }, "description": "1–4 things to get across, using the course's words" }
                        },
                        "required": ["title", "summary", "complexity", "scenario", "goals"]
                    }
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
            "Permanently delete a course with all its lessons and practice history. Only after the learner confirmed.",
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
            "create_writing_lesson",
            "Setting the scene…",
            "Make a writing lesson: a situation the learner writes their way through with an AI \
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
            "wiki_search",
            "Searching Wikipedia…",
            "Find Wikipedia articles, to write a lesson on culture or facts from a source.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "lang": { "type": "string", "description": "Wikipedia language code; default \"en\" (\"pl\" for Polish Wikipedia)" }
                },
                "required": ["query"]
            }),
        ),
        tool(
            "wiki_read",
            "Reading Wikipedia…",
            "Read a Wikipedia article: its lead and the list of its sections, or one section in full.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "lang": { "type": "string", "description": "Default \"en\"" },
                    "section": { "type": "string", "description": "A section heading to read in full; leave out for the lead and the section list" }
                },
                "required": ["title"]
            }),
        ),
        tool(
            "attach_source",
            "Attaching the article…",
            "Attach a Wikipedia article to a study lesson (new or existing) as its source: the learner can read it in the app (in Polish too, when Polish Wikipedia has it), the examiner draws on it, and the lesson is checked against it every three months.",
            json!({
                "type": "object",
                "properties": {
                    "lesson_id": { "type": "integer" },
                    "title": { "type": "string" },
                    "lang": { "type": "string", "description": "Default \"en\"" }
                },
                "required": ["lesson_id", "title"]
            }),
        ),
        tool(
            "remove_source",
            "Removing the source…",
            "Take a Wikipedia source off a lesson (its id is in get_lesson).",
            json!({
                "type": "object",
                "properties": { "source_id": { "type": "integer" } },
                "required": ["source_id"]
            }),
        ),
        tool(
            "create_study_lesson",
            "Writing the lesson…",
            "Write a study lesson: a short lesson (conjugation, grammar, culture) with objectives. \
             The examiner interviews the learner on the objectives at each check-in.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short, e.g. \"Iść — to go (on foot)\"" },
                    "summary": { "type": "string", "description": "One sentence: what the lesson covers" },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"], "description": "By how hard the subject is: low ≈ 1–2 min, medium ≈ 5–10 min, high ≈ 20+ min" },
                    "content": { "type": "string", "description": "The lesson in markdown: explanation, a table or examples with translations. Correct Polish only. Write every Polish example as [[say: …]] so it can be heard, its translation outside the tag." },
                    "objectives": { "type": "array", "items": { "type": "string" }, "description": "2–5 things the learner should be able to do afterwards" },
                    "period_days": { "type": "integer", "description": "Days between check-ins; leave out for the default" },
                    "course_id": { "type": "integer", "description": "The course to file it under; leave out to file it in \"Grammar & culture\"" }
                },
                "required": ["title", "summary", "complexity", "content", "objectives"]
            }),
        ),
        tool(
            "create_drill_lesson",
            "Making the drill…",
            "Make one drill lesson in a chosen format over words and/or sentences you give. \
             The items are added to a course (a new one named after the lesson if none is given).",
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
            "create_listening_lesson",
            "Making the listening lesson…",
            "Make a listening lesson for a song: the learner listens, then the examiner asks about what they heard (meaning, missing words, translation, grammar). One per song.",
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
            "list_lessons",
            "Looking at your lessons…",
            "List lessons: those due now, one course's lessons, or every study lesson.",
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
            "get_lesson",
            "Looking at your lessons…",
            "One lesson in full: lesson and objectives or its words and sentences, its schedule, and the last check-in's verdict.",
            json!({
                "type": "object",
                "properties": { "lesson_id": { "type": "integer" } },
                "required": ["lesson_id"]
            }),
        ),
        tool(
            "update_lesson",
            "Updating the lesson…",
            "Change a lesson: title, summary, complexity, content, objectives, check-in period, or \
             whether it is a challenge (beyond the course's level: needed to master the course, \
             not to complete it).",
            json!({
                "type": "object",
                "properties": {
                    "lesson_id": { "type": "integer" },
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"] },
                    "content": { "type": "string" },
                    "objectives": { "type": "array", "items": { "type": "string" } },
                    "period_days": { "type": "integer" },
                    "challenge": { "type": "boolean" }
                },
                "required": ["lesson_id"]
            }),
        ),
        tool(
            "delete_lesson",
            "Deleting the lesson…",
            "Permanently delete a lesson and its check-in history. Only after the learner confirmed.",
            json!({
                "type": "object",
                "properties": { "lesson_id": { "type": "integer" } },
                "required": ["lesson_id"]
            }),
        ),
        tool(
            "add_lesson_to_course",
            "Rearranging your courses…",
            "List a lesson in another course too. It stays one lesson — one schedule, one history — shown in each course.",
            json!({
                "type": "object",
                "properties": { "lesson_id": { "type": "integer" }, "course_id": { "type": "integer" } },
                "required": ["lesson_id", "course_id"]
            }),
        ),
        tool(
            "remove_lesson_from_course",
            "Rearranging your courses…",
            "Take a lesson out of a course. If no other course lists it, the lesson is deleted with its words, sentences and history — only after the learner confirmed that.",
            json!({
                "type": "object",
                "properties": { "lesson_id": { "type": "integer" }, "course_id": { "type": "integer" } },
                "required": ["lesson_id", "course_id"]
            }),
        ),
        tool(
            "move_lesson",
            "Rearranging your courses…",
            "Move a lesson from one course to another; its words and history go with it.",
            json!({
                "type": "object",
                "properties": {
                    "lesson_id": { "type": "integer" },
                    "from_course_id": { "type": "integer" },
                    "to_course_id": { "type": "integer" }
                },
                "required": ["lesson_id", "from_course_id", "to_course_id"]
            }),
        ),
        tool(
            "order_course_lessons",
            "Rearranging your courses…",
            "Put a course's lessons in an order (lesson ids, first to last). Lessons left out keep their order after these.",
            json!({
                "type": "object",
                "properties": {
                    "course_id": { "type": "integer" },
                    "lesson_ids": { "type": "array", "items": { "type": "integer" } }
                },
                "required": ["course_id", "lesson_ids"]
            }),
        ),
        tool(
            "schedule_lesson",
            "Updating the lesson…",
            "Make a lesson due now, so it shows at the top of the learner's list.",
            json!({
                "type": "object",
                "properties": { "lesson_id": { "type": "integer" } },
                "required": ["lesson_id"]
            }),
        ),
        tool(
            "suggest_lesson",
            "Finding the lesson…",
            "Show the learner a button that opens a lesson right now.",
            json!({
                "type": "object",
                "properties": { "lesson_id": { "type": "integer" } },
                "required": ["lesson_id"]
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
                    "course_id": { "type": "integer", "description": "Take vocabulary from this course; the song's listening lesson is filed there too" },
                    "chorus_pl": { "type": "string", "description": "An exact Polish chorus sentence, if the learner wants one" },
                    "style": { "type": "string", "description": "Musical style, e.g. \"upbeat indie folk, male vocals\"" },
                    "vocal_gender": { "type": "string", "enum": ["f", "m", ""] },
                    "length": {
                        "type": "string",
                        "enum": ["short", "medium", "long"],
                        "description": "short ≈ 2 min, medium ≈ 3 min, long ≈ 4–5 min. Leave out to use the learner's setting."
                    },
                    "listening_lesson": { "type": "boolean", "description": "Also make a listening lesson for the song (default true)" }
                },
                "required": ["about"]
            }),
        ),
        tool(
            "make_podcast",
            "Writing the podcast…",
            "Have a short learning podcast written and recorded: every sentence is said in English, \
             then in Polish. It is written from English Wikipedia articles — the ones named (up to \
             five; look them up with wiki_search), or else the best match for the subject — which \
             are kept with it for the learner to read; or it is a short made-up story from none. \
             It appears in the Music tab within a minute or two and costs only OpenRouter speech. \
             Choose subjects the learner cares about (their memories).",
            json!({
                "type": "object",
                "properties": {
                    "about": { "type": "string", "description": "The subject, in English" },
                    "articles": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Exact English Wikipedia article titles to write from (0–5); leave out to use the best match for the subject"
                    },
                    "story": { "type": "boolean", "description": "A short made-up story, written from no articles" },
                    "cast": {
                        "type": "string",
                        "enum": ["narrator", "hosts", "characters"],
                        "description": "Who talks, each in a voice of their own: one narrator; two hosts discussing the subject (default for articles); or characters acting it out, a radio play (default for a story)"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["pairs", "immersion"],
                        "description": "pairs: each sentence in English, then in Polish (default); immersion: only Polish is spoken, in simpler sentences (the English meaning shows with the lyrics)"
                    },
                    "length": {
                        "type": "string",
                        "enum": ["short", "medium", "long"],
                        "description": "pairs: short ≈ 20 sentences, medium ≈ 35, long ≈ 55; immersion: 30, 50, 80. Default medium."
                    },
                    "course_id": { "type": "integer", "description": "Work in vocabulary from this course; its listening lesson is filed there too" },
                    "voice": { "type": "string", "enum": ["f", "m"], "description": "The Polish voice; the English uses the other" },
                    "listening_lesson": { "type": "boolean", "description": "Also make a listening lesson for it (default false)" }
                },
                "required": ["about"]
            }),
        ),
        tool(
            "set_podcast_sound",
            "Changing how the podcast sounds…",
            "Change how a podcast's audio is put together and make it again (quick and free: the \
             sentences already read are reused). Leave out what should stay as it is.",
            json!({
                "type": "object",
                "properties": {
                    "podcast_id": { "type": "integer" },
                    "pause": { "type": "number", "description": "Seconds of silence after the English, before the Polish (0–6): time to say it yourself first" },
                    "repeats": { "type": "integer", "description": "How many times the Polish is said (1–3)" },
                    "slow": { "type": "boolean", "description": "Read the Polish slowly" },
                    "voices": { "type": "string", "enum": ["one", "two"], "description": "two: English and Polish in different voices" },
                    "english": { "type": "boolean", "description": "Say the English before each Polish sentence; off: only Polish is heard" }
                },
                "required": ["podcast_id"]
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
            "One song in full: its lyrics (Polish with English, by section), the course it was written from, where its listening lesson is filed, and everything it was made with — style, singer, Suno model, the brief, what was asked for, length, the songwriter model and any revisions. Use it to make a song that matches or varies another.",
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
    fn acted(result: Value, kind: &str, label: String, course_id: Option<i64>) -> Self {
        Outcome {
            result,
            action: Some(TutorAction { kind: kind.into(), label, course_id, song_id: None, lesson_id: None }),
        }
    }
    fn lesson(result: Value, kind: &str, label: String, lesson_id: i64) -> Self {
        Outcome {
            result,
            action: Some(TutorAction {
                kind: kind.into(),
                label,
                course_id: None,
                song_id: None,
                lesson_id: Some(lesson_id),
            }),
        }
    }
    fn song(result: Value, kind: &str, label: String, song_id: Option<i64>) -> Self {
        Outcome {
            result,
            action: Some(TutorAction { kind: kind.into(), label, course_id: None, song_id, lesson_id: None }),
        }
    }
}

#[derive(Deserialize)]
struct CourseArg {
    #[serde(rename = "course_id", alias = "topic_id")]
    course_id: i64,
}

#[derive(Deserialize)]
struct CreateArgs {
    #[serde(flatten)]
    course: NewCourse,
    /// Short study lessons on the grammar the course uses.
    #[serde(default, alias = "lessons")]
    study_lessons: Vec<lessons::NewStudy>,
    /// The course's situation, written out in Polish at the end.
    #[serde(default)]
    writing: Option<lessons::NewWriting>,
}

#[derive(Deserialize)]
struct ExtendArgs {
    #[serde(rename = "course_id", alias = "topic_id")]
    course_id: i64,
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
    course_id: i64,
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
struct LessonArg {
    lesson_id: i64,
}

#[derive(Deserialize)]
struct MembershipArgs {
    lesson_id: i64,
    course_id: i64,
}

#[derive(Deserialize)]
struct MoveArgs {
    lesson_id: i64,
    from_course_id: i64,
    to_course_id: i64,
}

#[derive(Deserialize)]
struct OrderArgs {
    course_id: i64,
    #[serde(default)]
    lesson_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct SourceArg {
    source_id: i64,
}

#[derive(Deserialize)]
struct WikiArgs {
    #[serde(default)]
    query: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    section: Option<String>,
    #[serde(default)]
    lesson_id: Option<i64>,
}

/// A tool call in today's names. Conversations saved before "blocks" became
/// lessons and "topics" became courses hold calls like `get_block` with a
/// `block_id`; a model reading them back may call the same way again.
fn current_call(name: &str, arguments: &str) -> (String, String) {
    fn upgrade(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (old, new) in [
                    ("block_id", "lesson_id"),
                    ("block_ids", "lesson_ids"),
                    ("topic_id", "course_id"),
                    ("topic_ids", "course_ids"),
                    ("lesson", "content"),
                    ("lessons", "study_lessons"),
                ] {
                    if !map.contains_key(new) {
                        if let Some(v) = map.remove(old) {
                            map.insert(new.to_string(), v);
                        }
                    }
                }
                map.values_mut().for_each(upgrade);
            }
            Value::Array(items) => items.iter_mut().for_each(upgrade),
            _ => {}
        }
    }
    let name = name.replace("blocks", "lessons").replace("block", "lesson");
    let arguments = match serde_json::from_str::<Value>(arguments) {
        Ok(mut value) => {
            upgrade(&mut value);
            value.to_string()
        }
        Err(_) => arguments.to_string(),
    };
    (name, arguments)
}

/// The Wikipedia tools that need the network, run without holding the lock.
fn is_wiki_tool(name: &str) -> bool {
    matches!(name, "wiki_search" | "wiki_read" | "attach_source")
}

async fn run_wiki_tool(db: &Db, name: &str, arguments: &str) -> Outcome {
    let result: Result<Outcome> = async {
        let args: WikiArgs = parse(arguments)?;
        let lang = args.lang.as_deref().filter(|l| !l.is_empty()).unwrap_or("en");
        match name {
            "wiki_search" => {
                let hits = crate::wiki::search(lang, &args.query).await?;
                Ok(Outcome::data(json!({ "results": hits })))
            }
            "wiki_read" => {
                let article = crate::wiki::fetch(lang, &args.title).await?;
                let sections: Vec<&str> = article
                    .content
                    .lines()
                    .filter(|l| l.starts_with('#'))
                    .map(|l| l.trim_start_matches('#').trim())
                    .collect();
                let text = match &args.section {
                    Some(wanted) => section_text(&article.content, wanted)
                        .ok_or_else(|| Error::msg(format!("No section “{wanted}”. Sections: {}", sections.join(", "))))?,
                    None => article.summary.clone(),
                };
                Ok(Outcome::data(json!({
                    "title": article.title,
                    "url": article.url,
                    "text": text.chars().take(8000).collect::<String>(),
                    "sections": sections,
                    "polish_title": article.polish_title,
                })))
            }
            _ => {
                let lesson_id = args.lesson_id.ok_or_else(|| Error::msg("attach_source needs a lesson_id."))?;
                let source = crate::wiki::attach(db, lesson_id, lang, &args.title).await?;
                Ok(Outcome::lesson(
                    json!({ "attached": { "source_id": source.id, "title": source.title, "in_polish_too": source.polish_title.is_some() } }),
                    "sourceAdded",
                    format!("Attached “{}” from Wikipedia", source.title),
                    lesson_id,
                ))
            }
        }
    }
    .await;
    result.unwrap_or_else(|error| Outcome::data(json!({ "error": error.to_string() })))
}

/// One section of an article's markdown, by its heading (sub-sections
/// included).
fn section_text(markdown: &str, wanted: &str) -> Option<String> {
    let lines: Vec<&str> = markdown.lines().collect();
    let start = lines.iter().position(|l| {
        l.starts_with('#') && l.trim_start_matches('#').trim().eq_ignore_ascii_case(wanted.trim())
    })?;
    let level = lines[start].chars().take_while(|c| *c == '#').count();
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.starts_with('#') && l.chars().take_while(|c| *c == '#').count() <= level)
        .map_or(lines.len(), |i| start + 1 + i);
    Some(lines[start..end].join("\n"))
}

#[derive(Deserialize)]
struct WritingArgs {
    #[serde(flatten)]
    writing: lessons::NewWriting,
}

#[derive(Deserialize)]
struct StudyArgs {
    #[serde(flatten)]
    study: lessons::NewStudy,
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
struct ListLessonsArgs {
    which: String,
    #[serde(default, rename = "course_id", alias = "topic_id")]
    course_id: Option<i64>,
}

#[derive(Deserialize)]
struct UpdateLessonArgs {
    lesson_id: i64,
    #[serde(flatten)]
    changes: lessons::LessonChanges,
}

fn complexity_label(complexity: &str) -> &'static str {
    match complexity {
        "low" => "a low (1–2 min)",
        "medium" => "a medium (5–10 min)",
        _ => "a deep (20+ min)",
    }
}

fn lesson_json(lesson: &lessons::Lesson) -> Value {
    json!({
        "lesson_id": lesson.id,
        "kind": lesson.kind,
        "format": lesson.format,
        "title": lesson.title,
        "summary": lesson.summary,
        "complexity": lesson.complexity,
        "course_ids": lesson.course_ids,
        "course": lesson.course_title,
        "items": lesson.items,
        "period_days": lesson.period_days,
        "times_done": lesson.reps,
        "last_result": lesson.last_result,
        "due_in_days": lesson.due_day.map(|due| due - lesson.last_day.unwrap_or(due)),
        "made_by": lesson.origin,
        "challenge": lesson.challenge,
        "passed": lesson.passed,
        "writing_support": (lesson.format == "writing").then(|| ["guided", "supported", "free"][lesson.support_level.clamp(0, 2) as usize]),
    })
}


#[derive(Deserialize)]
struct SongArgs {
    about: String,
    #[serde(default, rename = "course_id", alias = "topic_id")]
    course_id: Option<i64>,
    #[serde(default)]
    chorus_pl: String,
    #[serde(default)]
    style: String,
    #[serde(default)]
    vocal_gender: String,
    #[serde(default)]
    length: String,
    #[serde(default = "yes")]
    listening_lesson: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct PodcastArgs {
    about: String,
    #[serde(default)]
    articles: Vec<String>,
    /// One article, as a model may still name it.
    #[serde(default)]
    article: String,
    #[serde(default)]
    story: bool,
    #[serde(default)]
    cast: String,
    #[serde(default)]
    format: String,
    #[serde(default)]
    length: String,
    #[serde(default)]
    course_id: Option<i64>,
    #[serde(default)]
    voice: String,
    #[serde(default)]
    listening_lesson: bool,
}

#[derive(Deserialize)]
struct PodcastSoundArgs {
    podcast_id: i64,
    pause: Option<f64>,
    repeats: Option<u32>,
    slow: Option<bool>,
    voices: Option<String>,
    english: Option<bool>,
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

/// Which course a song was written from, and where its listening lesson is.
fn song_placement(conn: &mut Conn, song_id: i64) -> Result<Value> {
    let course: Option<(i64, String)> = songs_table::table
        .inner_join(courses_table::table)
        .filter(songs_table::id.eq(song_id))
        .select((courses_table::id, courses_table::title_en))
        .first(conn)
        .optional()?;
    let lesson: Option<i64> = lessons_table::table
        .filter(lessons_table::song_id.eq(song_id))
        .select(lessons_table::id)
        .first(conn)
        .optional()?;
    let lesson_courses: Vec<Value> = match lesson {
        Some(id) => lessons::detail(conn, id)?
            .courses
            .into_iter()
            .map(|(id, title, _)| json!({ "course_id": id, "title": title }))
            .collect(),
        None => vec![],
    };
    Ok(json!({
        "written_from_course": course.map(|(id, title)| json!({ "course_id": id, "title": title })),
        "listening_lesson_id": lesson,
        "listening_lesson_in": lesson_courses,
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
            let course_id = created.course_id;
            // The lessons and the writing lesson join the course; one that
            // cannot be made is reported, not a reason to lose the course.
            let mut problems = Vec::new();
            let mut lessons = 0;
            for mut study in args.study_lessons {
                study.course_id = Some(course_id);
                match lessons::create_study(conn, study, "tutor") {
                    Ok(_) => lessons += 1,
                    Err(error) => problems.push(format!("lesson: {error}")),
                }
            }
            let mut writing = false;
            if let Some(mut lesson) = args.writing {
                lesson.course_id = Some(course_id);
                match lessons::create_writing(conn, lesson, "tutor") {
                    Ok(_) => writing = true,
                    Err(error) => problems.push(format!("writing lesson: {error}")),
                }
            }
            courses::arrange_path(conn, course_id)?;

            let mut label = format!(
                "Created “{}” — {} words, {} sentences",
                created.title, created.words, created.sentences
            );
            if created.dialogue_lines > 0 {
                label.push_str(", a dialogue");
            }
            match lessons {
                0 => {}
                1 => label.push_str(", a lesson"),
                n => label.push_str(&format!(", {n} lessons")),
            }
            if writing {
                label.push_str(", a writing lesson");
            }
            let lessons_made = lessons::for_course(conn, course_id)?.len();
            label.push_str(&format!(" · {lessons_made} lessons"));
            Ok(Outcome::acted(
                json!({ "created": created, "lessons": lessons_made, "lessons": lessons, "writing": writing, "problems": problems }),
                "courseCreated",
                label,
                Some(created.course_id),
            ))
        }
        "add_to_course" => {
            let args: ExtendArgs = parse(arguments)?;
            let updated =
                courses::extend(conn, args.course_id, &args.words, &args.sentences, &args.dialogue, "tutor")?;
            Ok(Outcome::acted(
                json!({ "updated": updated }),
                "courseUpdated",
                format!(
                    "Updated “{}” — now {} words, {} sentences",
                    updated.title, updated.words, updated.sentences
                ),
                Some(updated.course_id),
            ))
        }
        "edit_course" => {
            let args: EditArgs = parse(arguments)?;
            courses::rename(
                conn,
                args.course_id,
                args.title.as_deref(),
                args.title_pl.as_deref(),
                args.summary.as_deref(),
            )?;
            let updated =
                courses::remove_items(conn, args.course_id, &args.remove_words, &args.remove_sentences)?;
            Ok(Outcome::acted(
                json!({ "updated": updated }),
                "courseUpdated",
                format!("Edited “{}”", updated.title),
                Some(updated.course_id),
            ))
        }
        "delete_course" => {
            let args: CourseArg = parse(arguments)?;
            let title = courses::delete(conn, args.course_id)?;
            Ok(Outcome::acted(
                json!({ "deleted": title }),
                "courseDeleted",
                format!("Deleted “{title}”"),
                None,
            ))
        }
        "list_courses" => Ok(Outcome::data(list_courses(conn)?)),
        "get_course" => {
            let args: CourseArg = parse(arguments)?;
            Ok(Outcome::data(course_detail(conn, args.course_id)?))
        }
        "search_vocabulary" => {
            let args: SearchArgs = parse(arguments)?;
            Ok(Outcome::data(search(conn, &args.query)?))
        }
        "get_progress" => Ok(Outcome::data(progress(conn)?)),
        "create_writing_lesson" => {
            let args: WritingArgs = parse(arguments)?;
            let lesson = lessons::create_writing(conn, args.writing, "tutor")?;
            Ok(Outcome::lesson(
                json!({ "created": lesson_json(&lesson) }),
                "lessonCreated",
                format!("Set up “{}” — {} writing lesson", lesson.title, complexity_label(&lesson.complexity)),
                lesson.id,
            ))
        }
        "create_study_lesson" => {
            let args: StudyArgs = parse(arguments)?;
            let lesson = lessons::create_study(conn, args.study, "tutor")?;
            Ok(Outcome::lesson(
                json!({ "created": lesson_json(&lesson) }),
                "lessonCreated",
                format!("Wrote “{}” — {} study lesson", lesson.title, complexity_label(&lesson.complexity)),
                lesson.id,
            ))
        }
        "create_drill_lesson" => {
            let args: DrillArgs = parse(arguments)?;
            if args.words.is_empty() && args.sentences.is_empty() {
                return Err(Error::msg("A drill needs words or sentences."));
            }
            // The lesson owns its words; it is not cut into the course's own drills.
            let course_id = match args.course_id {
                Some(id) => id,
                None => courses::create_empty(conn, &args.title, &args.summary)?,
            };
            let (lesson, skipped) = lessons::create_drill(
                conn,
                course_id,
                &args.title,
                &args.summary,
                &args.format,
                &args.complexity,
                &courses::Items { words: args.words, sentences: args.sentences, dialogue: vec![] },
                args.period_days,
                "tutor",
            )?;
            Ok(Outcome::lesson(
                json!({ "created": lesson_json(&lesson), "course_id": course_id, "skipped_duplicates": skipped }),
                "lessonCreated",
                format!("Made “{}” — {} drill, {} items", lesson.title, complexity_label(&lesson.complexity), lesson.items),
                lesson.id,
            ))
        }
        "create_listening_lesson" => {
            let args: ListeningArgs = parse(arguments)?;
            let lesson = lessons::create_listening(
                conn,
                args.song_id,
                args.complexity.as_deref(),
                args.focus.as_deref(),
                args.course_id,
                "tutor",
            )?;
            Ok(Outcome::lesson(
                json!({ "created": lesson_json(&lesson) }),
                "lessonCreated",
                format!("Made “{}” — {} listening lesson", lesson.title, complexity_label(&lesson.complexity)),
                lesson.id,
            ))
        }
        "list_lessons" => {
            let args: ListLessonsArgs = parse(arguments)?;
            let list = match args.which.as_str() {
                "due" => lessons::overview(conn)?.due,
                "course" | "topic" => lessons::for_course(
                    conn,
                    args.course_id.ok_or_else(|| Error::msg("Give a course_id."))?,
                )?,
                _ => {
                    let ids: Vec<i64> = lessons_table::table
                        .filter(lessons_table::kind.eq("study"))
                        .order((lessons_table::ord, lessons_table::id))
                        .select(lessons_table::id)
                        .load(conn)?;
                    ids.into_iter().map(|id| lessons::load(conn, id)).collect::<Result<_>>()?
                }
            };
            Ok(Outcome::data(Value::Array(list.iter().map(lesson_json).collect())))
        }
        "get_lesson" => {
            let args: LessonArg = parse(arguments)?;
            let detail = lessons::detail(conn, args.lesson_id)?;
            Ok(Outcome::data(json!({
                "lesson": lesson_json(&detail.lesson),
                "content": detail.content,
                "objectives": detail.objectives,
                "words": detail.words.iter().map(|(pl, en)| json!({ "pl": pl, "en": en })).collect::<Vec<_>>(),
                "sentences": detail.sentences.iter().map(|(pl, en)| json!({ "pl": pl, "en": en })).collect::<Vec<_>>(),
                "last_checkin_feedback": detail.last_feedback,
                "sources": crate::wiki::sources_of(conn, args.lesson_id)?
                    .into_iter()
                    .map(|s| json!({ "source_id": s.id, "title": s.title, "lang": s.lang, "url": s.url, "in_polish_too": s.polish_title.is_some() }))
                    .collect::<Vec<_>>(),
            })))
        }
        "remove_source" => {
            let args: SourceArg = parse(arguments)?;
            crate::wiki::remove(conn, args.source_id)?;
            Ok(Outcome::data(json!({ "removed": args.source_id })))
        }
        "update_lesson" => {
            let args: UpdateLessonArgs = parse(arguments)?;
            let lesson = lessons::update(conn, args.lesson_id, args.changes)?;
            Ok(Outcome::lesson(
                json!({ "updated": lesson_json(&lesson) }),
                "lessonUpdated",
                format!("Updated “{}”", lesson.title),
                lesson.id,
            ))
        }
        "delete_lesson" => {
            let args: LessonArg = parse(arguments)?;
            let title = lessons::delete(conn, args.lesson_id)?;
            Ok(Outcome::acted(json!({ "deleted": title }), "lessonDeleted", format!("Deleted “{title}”"), None))
        }
        "add_lesson_to_course" => {
            let args: MembershipArgs = parse(arguments)?;
            courses::add_lesson(conn, args.course_id, args.lesson_id)?;
            let lesson = lessons::load(conn, args.lesson_id)?;
            let course = courses::require_course(conn, args.course_id)?;
            Ok(Outcome::acted(
                json!({ "lesson": lesson_json(&lesson) }),
                "courseUpdated",
                format!("“{}” is now also in “{course}”", lesson.title),
                Some(args.course_id),
            ))
        }
        "remove_lesson_from_course" => {
            let args: MembershipArgs = parse(arguments)?;
            let title = lessons::load(conn, args.lesson_id)?.title;
            let course = courses::require_course(conn, args.course_id)?;
            let deleted = courses::remove_lesson(conn, args.course_id, args.lesson_id)?;
            Ok(Outcome::acted(
                json!({ "removed": title, "lesson_deleted": deleted }),
                if deleted { "lessonDeleted" } else { "courseUpdated" },
                if deleted {
                    format!("Deleted “{title}” (it was only in “{course}”)")
                } else {
                    format!("Took “{title}” out of “{course}”")
                },
                if deleted { None } else { Some(args.course_id) },
            ))
        }
        "move_lesson" => {
            let args: MoveArgs = parse(arguments)?;
            courses::move_lesson(conn, args.lesson_id, args.from_course_id, args.to_course_id)?;
            let lesson = lessons::load(conn, args.lesson_id)?;
            let course = courses::require_course(conn, args.to_course_id)?;
            Ok(Outcome::acted(
                json!({ "lesson": lesson_json(&lesson) }),
                "courseUpdated",
                format!("Moved “{}” to “{course}”", lesson.title),
                Some(args.to_course_id),
            ))
        }
        "order_course_lessons" => {
            let args: OrderArgs = parse(arguments)?;
            courses::order_lessons(conn, args.course_id, &args.lesson_ids)?;
            let course = courses::require_course(conn, args.course_id)?;
            Ok(Outcome::acted(
                json!({ "order": lessons::for_course(conn, args.course_id)?.iter().map(|b| b.id).collect::<Vec<_>>() }),
                "courseUpdated",
                format!("Reordered “{course}”"),
                Some(args.course_id),
            ))
        }
        "schedule_lesson" => {
            let args: LessonArg = parse(arguments)?;
            let lesson = lessons::due_now(conn, args.lesson_id)?;
            Ok(Outcome::lesson(
                json!({ "due_now": lesson.title }),
                "lessonScheduled",
                format!("“{}” is due now", lesson.title),
                lesson.id,
            ))
        }
        "suggest_lesson" => {
            let args: LessonArg = parse(arguments)?;
            let lesson = lessons::load(conn, args.lesson_id)?;
            Ok(Outcome::lesson(
                json!({ "shown_button_for": lesson.title }),
                "lessonSuggested",
                format!("{} · {}", lesson.title, complexity_label(&lesson.complexity)),
                lesson.id,
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
                        "kind": song.kind,
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
                "kind": song.kind,
                "podcast_sound": song.podcast,
                "sources": song.origin.sources.iter().map(|s| json!({ "title": s.title, "url": s.url })).collect::<Vec<_>>(),
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

fn list_courses(conn: &mut Conn) -> Result<Value> {
    let courses = stats::course_progress(conn)?
        .into_iter()
        .map(|p| {
            json!({
                "course_id": p.course.id,
                "title": p.course.title,
                "title_pl": p.course.title_pl,
                "words": p.total_words,
                "words_met": p.seen_words,
                "words_mastered": p.mastered_words,
                "sentences": p.total_sentences,
                "lessons": lessons::for_course(conn, p.course.id).map(|b| b.len()).unwrap_or(0),
            })
        })
        .collect();
    Ok(Value::Array(courses))
}

fn course_detail(conn: &mut Conn, course_id: i64) -> Result<Value> {
    let course = session::load_course(conn, course_id)?;
    let rows: Vec<(i64, String, String, String, i64)> = course_words::table
        .filter(course_words::course_id.eq(course_id))
        .order(course_words::ord)
        .select((course_words::id, course_words::pl, course_words::en, course_words::pos, course_words::lesson_id))
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
        .map(|(id, pl, en, pos, lesson_id)| {
            let seen = history.get(&id);
            json!({
                "pl": pl,
                "en": en,
                "pos": pos,
                "met": seen.is_some(),
                "correct_reps": seen.map_or(0, |s| s.0),
                "lapses": seen.map_or(0, |s| s.1),
                "lesson_id": lesson_id,
            })
        })
        .collect();
    let sentences: Vec<Value> = course_sentences::table
        .filter(course_sentences::course_id.eq(course_id))
        .order(course_sentences::ord)
        .select((course_sentences::pl, course_sentences::en, course_sentences::lesson_id))
        .load::<(String, String, i64)>(conn)?
        .into_iter()
        .map(|(pl, en, lesson_id)| json!({ "pl": pl, "en": en, "lesson_id": lesson_id }))
        .collect();
    let dialogue: Vec<Value> = session::dialogue_lines(conn, course_id)?
        .into_iter()
        .map(|l| json!({ "speaker": l.speaker, "pl": l.pl, "en": l.meaning }))
        .collect();
    let lessons: Vec<Value> = lessons::for_course(conn, course_id)?.iter().map(lesson_json).collect();
    let status = courses::status(conn, course_id)?;
    Ok(json!({
        "course_id": course.id,
        "title": course.title,
        "title_pl": course.title_pl,
        "summary": course.summary,
        "status": if status.closed_at.is_some() { "mastered and closed — finished, never changed" } else { status.status.as_str() },
        "words": words,
        "sentences": sentences,
        "dialogue": dialogue,
        "lessons": lessons,
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
        .inner_join(courses_table::table)
        .filter(course_words::pl.like(&pattern).or(course_words::en.like(&pattern)))
        .order((courses_table::ord, course_words::ord))
        .select((
            course_words::id,
            course_words::pl,
            course_words::en,
            course_words::pos,
            courses_table::title_en,
            courses_table::id,
            course_words::lesson_id,
        ))
        .load::<(i64, String, String, String, String, i64, i64)>(conn)?
        .into_iter()
        // A word in a lesson two courses list is one word.
        .filter(|row| seen.insert(row.0))
        .take(40)
        .map(|(id, pl, en, pos, course, course_id, lesson_id)| {
            json!({
                "pl": pl, "en": en, "pos": pos, "course": course, "course_id": course_id,
                "met": met.contains(&id), "lesson_id": lesson_id,
            })
        })
        .collect();
    let mut seen = HashSet::new();
    let sentences: Vec<Value> = course_sentences::table
        .inner_join(courses_table::table)
        .filter(course_sentences::pl.like(&pattern).or(course_sentences::en.like(&pattern)))
        .select((course_sentences::id, course_sentences::pl, course_sentences::en, courses_table::title_en))
        .load::<(i64, String, String, String)>(conn)?
        .into_iter()
        .filter(|row| seen.insert(row.0))
        .take(15)
        .map(|(_, pl, en, course)| json!({ "pl": pl, "en": en, "course": course }))
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
        "lessons_due_now": s.due_today,
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
                course_id: args.course_id,
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
        let listen = args.listening_lesson;
        let song_id = songs::start_song(app, db, draft)?;
        let lesson = if listen {
            Some(lessons::create_listening(&mut db.lock(), song_id, None, None, args.course_id, "tutor")?)
        } else {
            None
        };
        Ok(Outcome::song(
            json!({
                "song_id": song_id,
                "title": title,
                "chorus": chorus,
                "status": "Suno is singing it now; ready in 2–4 minutes",
                "listening_lesson_id": lesson.as_ref().map(|b| b.id),
            }),
            "songStarted",
            format!(
                "Writing “{title}” — Suno is singing it now{}",
                if lesson.is_some() { ", with a listening lesson" } else { "" }
            ),
            Some(song_id),
        ))
    }
    .await;
    result.unwrap_or_else(|error| Outcome::data(json!({ "error": error.to_string() })))
}

/// make_podcast reads Wikipedia and asks the writer, so it cannot run under
/// the database lock either.
fn make_podcast(app: &tauri::AppHandle, db: &Db, arguments: &str) -> Outcome {
    let result: Result<Outcome> = (|| {
        let args: PodcastArgs = parse(arguments)?;
        let request = crate::podcasts::PodcastRequest {
            topic: args.about.clone(),
            articles: if args.article.trim().is_empty() {
                args.articles
            } else {
                std::iter::once(args.article).chain(args.articles).collect()
            },
            story: args.story,
            cast: args.cast,
            format: args.format,
            length: args.length,
            course_id: args.course_id,
            vocal_gender: args.voice,
            ..Default::default()
        };
        // Written and recorded in the background; the episode is in the
        // library already, as "being made".
        let song_id = crate::podcasts::start_from_request(app, db, request)?;
        let lesson = if args.listening_lesson {
            Some(lessons::create_listening(&mut db.lock(), song_id, None, None, args.course_id, "tutor")?)
        } else {
            None
        };
        Ok(Outcome::song(
            json!({
                "podcast_id": song_id,
                "status": "being written and recorded in the background; ready in a few minutes in the Music tab",
                "listening_lesson_id": lesson.as_ref().map(|b| b.id),
            }),
            "songStarted",
            format!("Making a podcast about {}", args.about.trim()),
            Some(song_id),
        ))
    })();
    result.unwrap_or_else(|error| Outcome::data(json!({ "error": error.to_string() })))
}

/// Changes a podcast's sound and starts putting its audio together again.
fn set_podcast_sound(app: &tauri::AppHandle, db: &Db, arguments: &str) -> Outcome {
    let result: Result<Outcome> = (|| {
        let args: PodcastSoundArgs = parse(arguments)?;
        let song = songs::load_song(&mut db.lock(), args.podcast_id)?;
        let Some(mut audio) = song.podcast.clone().filter(|_| song.kind == "podcast") else {
            return Err(crate::error::Error::msg("That is a song, not a podcast."));
        };
        if let Some(pause) = args.pause {
            audio.pause = pause;
        }
        if let Some(repeats) = args.repeats {
            audio.repeats = repeats;
        }
        if let Some(slow) = args.slow {
            audio.slow = slow;
        }
        if let Some(voices) = args.voices {
            audio.voices = voices;
        }
        if let Some(english) = args.english {
            audio.english = english;
        }
        let audio = audio.clean();
        crate::podcasts::rerender(app, db, song.id, Some(audio.clone()))?;
        Ok(Outcome::song(
            json!({ "podcast_id": song.id, "sound": audio, "status": "making the audio again; ready in moments" }),
            "songStarted",
            format!("New sound for “{}”", song.title),
            Some(song.id),
        ))
    })();
    result.unwrap_or_else(|error| Outcome::data(json!({ "error": error.to_string() })))
}

/// The last study check-ins, with the examiner's notes, so the tutor can
/// follow up on what the learner found hard.
fn recent_checkins(conn: &mut Conn) -> Result<Value> {
    let rows: Vec<Value> = checkins::table
        .inner_join(lessons_table::table)
        .filter(checkins::finished_at.is_not_null())
        .order(checkins::finished_at.desc())
        .limit(8)
        .select((lessons_table::id, lessons_table::title, checkins::result, checkins::feedback))
        .load::<(i64, String, Option<String>, Option<String>)>(conn)?
        .into_iter()
        .map(|(id, title, result, feedback)| {
            json!({ "lesson_id": id, "lesson": title, "result": result, "examiner_said": feedback })
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
                    .map(|a| match (a.course_id, a.song_id) {
                        (Some(id), _) => format!("{} (course id {id})", a.label),
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
    let heads: Vec<(i64, String, i64, Option<i64>, Option<String>, Option<String>)> = tutor_conversations::table
        .order((tutor_conversations::updated_at.desc(), tutor_conversations::id.desc()))
        .select((
            tutor_conversations::id,
            tutor_conversations::title,
            tutor_conversations::updated_at,
            tutor_conversations::job_started_at,
            tutor_conversations::job_error,
            tutor_conversations::tag,
        ))
        .load(conn)?;
    heads
        .into_iter()
        .map(|(id, title, updated_at, job, error, tag)| {
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
                working: job.is_some(),
                error,
                tag,
                // Markdown marks would show as stray symbols in a one-line preview.
                preview: crate::text::clean(&crate::ai::unsay(&last).replace(['*', '_', '#', '`'], ""))
                    .chars()
                    .take(120)
                    .collect(),
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
/// (courses, lessons, songs) stays.
#[tauri::command]
pub async fn tutor_delete(db: State<'_, Db>, conversation_id: i64) -> Result<()> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    diesel::delete(tutor_messages::table.filter(tutor_messages::conversation_id.eq(conversation_id))).execute(conn)?;
    diesel::delete(tutor_conversations::table.find(conversation_id)).execute(conn)?;
    Ok(())
}

/// Whether the app is on screen; when it is not, a finished reply is
/// announced with a notification. The frontend reports it.
static APP_VISIBLE: AtomicBool = AtomicBool::new(true);

#[tauri::command]
pub async fn app_visibility(visible: bool) -> Result<()> {
    APP_VISIBLE.store(visible, Ordering::Relaxed);
    Ok(())
}

/// The stream id a conversation's background reply streams under, so a
/// screen opened later can follow it.
fn job_stream(conversation_id: i64) -> String {
    format!("tutor-{conversation_id}")
}

/// Sent as `tutor://done` when a background reply is finished or failed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDone {
    pub conversation_id: i64,
    pub message: Option<ChatMessage>,
    pub error: Option<String>,
}

/// What `tutor_send` returns at once: the conversation (new or not) and the
/// stored question. The reply follows in the background.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub conversation_id: i64,
    pub message: ChatMessage,
}

/// Sends a message to the tutor. It is stored at once and the tutor answers
/// in the background — the app can be left meanwhile. Text streams out as
/// `ai://stream` events under `tutor-<conversation id>`; while a tool runs,
/// `agent://status` says what it is doing; `tutor://done` carries the stored
/// reply. Off screen, the reply is announced with a notification.
#[tauri::command]
pub async fn tutor_send(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    message: String,
    // None starts a new conversation, titled after this message.
    conversation_id: Option<i64>,
) -> Result<Started> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err(Error::msg("Write something first."));
    }
    let (conversation_id, id) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let conversation_id = match conversation_id {
            Some(id) => {
                let job: Option<Option<i64>> = tutor_conversations::table
                    .find(id)
                    .select(tutor_conversations::job_started_at)
                    .first(conn)
                    .optional()?;
                match job {
                    None => return Err(Error::msg("That conversation no longer exists.")),
                    Some(Some(_)) => return Err(Error::msg("The tutor is still working on the last message.")),
                    Some(None) => id,
                }
            }
            None => create_conversation(conn, &message)?,
        };
        let id = append(conn, conversation_id, "user", &message, &[])?;
        (conversation_id, id)
    };
    start_job(&app, &db, conversation_id)?;
    Ok(Started {
        conversation_id,
        message: ChatMessage { id, conversation_id, role: "user".into(), content: message, actions: Vec::new() },
    })
}

/// Asks again after a reply failed: the tutor answers the last message anew.
#[tauri::command]
pub async fn tutor_retry(app: tauri::AppHandle, db: State<'_, Db>, conversation_id: i64) -> Result<()> {
    start_job(&app, &db, conversation_id)
}

/// Marks the conversation busy and starts the reply in the background.
fn start_job(app: &tauri::AppHandle, db: &Db, conversation_id: i64) -> Result<()> {
    {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let started = diesel::update(
            tutor_conversations::table
                .find(conversation_id)
                .filter(tutor_conversations::job_started_at.is_null()),
        )
        .set((tutor_conversations::job_started_at.eq(db::now_secs()), tutor_conversations::job_error.eq(None::<String>)))
        .execute(conn)?;
        if started == 0 {
            return Err(Error::msg("The tutor is already working on this conversation."));
        }
    }
    let app = app.clone();
    // With the screen off, Android would freeze the app half-way through.
    let busy = crate::work::begin(&app, "The tutor is writing a reply…");
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        let db = app.state::<Db>();
        let result = answer(&app, &db, conversation_id).await;
        let done = {
            let mut guard = db.lock();
            let conn = &mut *guard;
            let error = result.as_ref().err().map(|e| e.to_string());
            let _ = diesel::update(tutor_conversations::table.find(conversation_id))
                .set((
                    tutor_conversations::job_started_at.eq(None::<i64>),
                    tutor_conversations::job_error.eq(&error),
                ))
                .execute(conn);
            JobDone { conversation_id, message: result.ok(), error }
        };
        if !APP_VISIBLE.load(Ordering::Relaxed) {
            notify_done(&app, &db, &done);
        }
        let _ = app.emit("tutor://done", done);
    });
    Ok(())
}

fn notify_done(app: &tauri::AppHandle, db: &Db, done: &JobDone) {
    use tauri_plugin_notification::NotificationExt;
    let title: String = tutor_conversations::table
        .find(done.conversation_id)
        .select(tutor_conversations::title)
        .first(&mut *db.lock())
        .unwrap_or_else(|_| "The tutor".into());
    let body = match (&done.message, &done.error) {
        (Some(message), _) => {
            let text = crate::text::clean(&crate::ai::unsay(&message.content).replace(['*', '_', '#', '`'], ""));
            let mut short: String = text.chars().take(140).collect();
            if text.chars().count() > 140 {
                short.push('…');
            }
            short
        }
        (None, Some(error)) => format!("The tutor could not answer: {error}"),
        (None, None) => "The tutor answered.".into(),
    };
    let _ = app.notification().builder().title(format!("Tutor: {title}")).body(body).show();
}

/// Runs one reply to the conversation's last message and stores it.
async fn answer(app: &tauri::AppHandle, db: &Db, conversation_id: i64) -> Result<ChatMessage> {
    let stream_id = job_stream(conversation_id);
    let (wire, model, word_chat) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let mut prompt = RULES.to_string();
        let state = learner_state(conn)?;
        // A chat about one word: talk about it, don't lecture.
        let about: Option<String> = tutor_conversations::table
            .find(conversation_id)
            .select(tutor_conversations::about)
            .first(conn)?;
        // A word chat can run on its own, smaller and faster model.
        let mut model = None;
        let word_chat = about.is_some();
        if let Some(about) = about {
            prompt.push_str(&format!("\n\n{}", word_chat_rules(&about)));
            let own = crate::ai::stored_model(conn, "word_chat_model");
            if !own.is_empty() {
                model = Some(own);
            }
        }
        let mut wire = vec![Wire::system(crate::ai::with_say_player(prompt))];
        // The question is already stored, so it is the end of the history.
        let mut history = recent_history(conn, conversation_id)?;
        // The learner's current state goes just before the question: the
        // system prompt and everything before stay cacheable.
        let at = history.len().saturating_sub(1);
        history.insert(at, Wire::user(format!("(The app, for context — not the learner writing:)\n{state}")));
        wire.extend(history);
        (wire, model, word_chat)
    };

    let agent = Agent {
        app,
        db,
        // Word chats are counted on their own in the cost overview.
        name: if word_chat { crate::costs::WORD_CHAT } else { crate::costs::TUTOR },
        stream_id: &stream_id,
        tools: Some(tool_definitions()),
        max_rounds: MAX_ROUNDS,
        stop_once_answered: false,
        show_status: true,
        model,
    };
    let outcome = agent
        .run(wire, |call| {
            Box::pin(async move {
                let (name, arguments) = current_call(&call.function.name, &call.function.arguments);
                let done = if name == "make_song" {
                    // The network twice (songwriter, then Suno): not under the lock.
                    make_song(app, db, &arguments).await
                } else if name == "make_podcast" {
                    make_podcast(app, db, &arguments)
                } else if name == "set_podcast_sound" {
                    // Starts work in the background, which takes the lock itself.
                    set_podcast_sound(app, db, &arguments)
                } else if is_wiki_tool(&name) {
                    run_wiki_tool(db, &name, &arguments).await
                } else {
                    let mut guard = db.lock();
                    let conn = &mut *guard;
                    // Every new course gets a picture, whichever tool made it.
                    let before = course_ids(conn);
                    let done = execute(conn, &name, &arguments);
                    for course_id in course_ids(conn).difference(&before) {
                        crate::pictures::draw_in_background(app, *course_id);
                    }
                    done
                };
                ToolOutput { result: done.result, extra: done.action, ends_turn: false }
            })
        })
        .await;

    let mut actions = outcome.extras;
    actions.extend(outcome.remembered.into_iter().map(|label| TutorAction {
        kind: "memory".into(),
        label,
        course_id: None,
        song_id: None,
        lesson_id: None,
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
    let mut guard = db.lock();
    let conn = &mut *guard;
    let id = append(conn, conversation_id, "assistant", &reply, &actions)?;
    Ok(ChatMessage { id, conversation_id, role: "assistant".into(), content: reply, actions })
}

fn course_ids(conn: &mut Conn) -> HashSet<i64> {
    courses_table::table.select(courses_table::id).load::<i64>(conn).map(|ids| ids.into_iter().collect()).unwrap_or_default()
}

/// How a conversation about one word or sentence goes (\"Talk about it\"):
/// a chat about using it, not a dictionary entry.
pub(crate) fn word_chat_rules(about: &str) -> String {
    format!(
        "This conversation is about one thing the learner just practised: {about}.
- Talk, don't lecture: two to four sentences a turn, no headings, no dictionary entry.
- Open with how and when people actually use it — a real situation and one natural example \
  (Polish, with a translation) — then invite the learner to try: ask them to say something with \
  it, or what they would say in a situation you describe.
- When they answer, react like a person would. If their Polish has a slip, show the right form in \
  passing and why, briefly. Then take it one step further: a related phrase, another form, or a \
  new little situation to try.
- Grammar only when it helps them say the next thing. If they want practice on it, you can make \
  a lesson — when they ask."
    )
}

/// What \"Talk about it\" opens.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalkAbout {
    pub conversation_id: i64,
    /// The word had been talked about before: its conversation carries on.
    pub existing: bool,
}

/// Opens the tutor conversation about a word or sentence (tagged \"word\"),
/// or carries on the one already about it. A new one starts with the tutor
/// talking, in the background like any reply.
#[tauri::command]
pub async fn tutor_talk_about(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    item_kind: String,
    item_id: i64,
    // For a line of a conversation: its Polish and meaning.
    polish: Option<String>,
    meaning: Option<String>,
) -> Result<TalkAbout> {
    let conversation_id = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let (polish, meaning): (String, String) = match item_kind.as_str() {
            "word" => crate::schema::words::table
                .find(item_id)
                .select((crate::schema::words::pl, crate::schema::words::en))
                .first(conn)?,
            "sentence" => crate::schema::sentences::table
                .find(item_id)
                .select((crate::schema::sentences::pl, crate::schema::sentences::en))
                .first(conn)?,
            "line" => match (polish, meaning) {
                (Some(pl), Some(en)) if !pl.trim().is_empty() => (pl.trim().to_string(), en.trim().to_string()),
                _ => return Err(Error::msg("A line needs its Polish and meaning.")),
            },
            other => return Err(Error::msg(format!("Cannot talk about a '{other}'."))),
        };
        let about = format!("{polish} — {meaning}");
        let existing: Option<i64> = tutor_conversations::table
            .filter(tutor_conversations::tag.eq("word"))
            .filter(tutor_conversations::about.eq(&about))
            .select(tutor_conversations::id)
            .first(conn)
            .optional()?;
        if let Some(id) = existing {
            return Ok(TalkAbout { conversation_id: id, existing: true });
        }
        let id = create_conversation(conn, &polish)?;
        diesel::update(tutor_conversations::table.find(id))
            .set((tutor_conversations::tag.eq("word"), tutor_conversations::about.eq(&about)))
            .execute(conn)?;
        let opening = if item_kind == "line" {
            format!(
                "Let's talk about this line from a conversation I practised: “{polish}” ({meaning}). Why is it said like this?"
            )
        } else {
            format!("Let's talk about “{polish}”.")
        };
        append(conn, id, "user", &opening, &[])?;
        id
    };
    start_job(&app, &db, conversation_id)?;
    Ok(TalkAbout { conversation_id, existing: false })
}

/// A reply that was running when the app was closed never finished: say so,
/// so the conversation can be asked again. Called at startup.
pub fn recover_interrupted(conn: &mut Conn) -> Result<()> {
    diesel::update(tutor_conversations::table.filter(tutor_conversations::job_started_at.is_not_null()))
        .set((
            tutor_conversations::job_started_at.eq(None::<i64>),
            tutor_conversations::job_error.eq("The app was closed before the tutor finished. Ask again."),
        ))
        .execute(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn calls_in_the_old_names_still_work() {
        let (name, args) = current_call("add_block_to_course", r#"{"block_id": 4, "course_id": 2}"#);
        assert_eq!(name, "add_lesson_to_course");
        assert_eq!(serde_json::from_str::<Value>(&args).unwrap(), json!({"lesson_id": 4, "course_id": 2}));
        let (name, args) = current_call(
            "create_course",
            r#"{"title": "x", "lessons": [{"title": "y", "lesson": "**Mam**"}]}"#,
        );
        assert_eq!(name, "create_course");
        assert_eq!(serde_json::from_str::<Value>(&args).unwrap()["study_lessons"][0]["content"], "**Mam**");
        assert_eq!(current_call("order_course_blocks", "{}").0, "order_course_lessons");
    }

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
