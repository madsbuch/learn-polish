//! The tutor: an assistant for the whole app, not a Polish role-play partner.
//!
//! It talks to the learner in English and can act in the app through tools —
//! build a course on any theme, write lessons and drills into courses, look up
//! progress and vocabulary, make songs, adjust practice settings. Every
//! tool runs here, in Rust, against the same SQLite database the rest of the
//! app uses, so what the tutor makes is immediately practisable.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use tokio::sync::watch;

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
#[allow(unused_imports)]
use crate::ids::*;

/// How much of the conversation is replayed to the model.
const HISTORY_LIMIT: i64 = 30;
/// Tool rounds per learner message. Creating a course takes one or two;
/// the cap only stops a confused model from looping.
const MAX_ROUNDS: usize = 8;
/// Tool calls per learner message: `counsel.max-steps`, as this app sets it.
const MAX_STEPS: usize = 30;

// ---------------------------------------------------------------------------
// Prompt
// ---------------------------------------------------------------------------

// Spec: TUTOR-6, TERM-2, counsel.IDN-1, counsel.IDN-4
pub(crate) const RULES: &str = "\
You are the tutor in Polski, an app for learning Polish. You explain Polish, suggest what to \
practise, and — when asked — change the app with your tools.

Talking:
- Reply in the language the learner writes in (English, Danish, …), and in English when that \
  is not clear. Polish examples always come with a translation into that language. Hold a conversation in Polish only when asked to.
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
- Courses list lessons. A lesson has one job, and is one of two kinds: a drill, tapped through \
  (practice on a few words, sentences or a dialogue it owns — the types are listed below), or a \
  conversation: a study lesson (a short text, then the examiner's questions), a listening lesson \
  (a song or podcast, then questions), or a writing lesson (a situation written out in Polish with \
  a partner who understands only Polish; goals are things to get across, not grammar).
- A check-in is a lesson coming back on its schedule, whatever its kind — not a kind of lesson.
- Complexity is how long a lesson takes and how deep it goes: low (one small idea, a few \
  minutes), medium (a pattern with variations), high (a system like the cases, the longest). It \
  is not its level. Split big subjects into several lessons instead of stuffing one.
- A new course is a small learning path, made in one create_course call: an 8–12 line dialogue \
  to hear the theme, where each line answers or reacts to the one before (the learner is asked \
  what comes next); 15–30 words and 8–12 short sentences; one or two short study lessons on the \
  grammar the dialogue uses; and a writing lesson where the learner plays out the course's \
  situation. The app orders and varies it. Then mix in other lesson types where the theme \
  suits them (pictures or a scene for a place full of things, a story, gaps, memory cards), \
  made with their own tools and the course's course_id: a course should practise in varied \
  ways, with no set number of each. Afterwards, offer a song — don't make one unasked. \
  To grow a course, add to it rather than making a new one.
- Courses are made to be finished: the learner completes one by passing each lesson once, and \
  masters it when every lesson has full mastery points, earned by passing it again — spaced \
  passes earn more, a go short of a pass earns a share by its score, nothing takes points away (a miss \
  brings the lesson back sooner), and the last stretch needs a pass on a second day (writing done without help). \
  A drill's cards follow its level: an A1 lesson asks words by choice and builds sentences from tiles already \
  in their form; typing, endings to pick and dictation come from A2 up, each item harder as it is known. Keep the core at \
  the course's level; a lesson clearly beyond it (free writing in a beginner course, a grammar \
  point a level up) is a challenge — set challenge with update_lesson, and give it its level. Challenges count for mastering, \
  not completing. Writing lessons start guided (word bank, hints) and step up as they pass.
- Every course has a CEFR level (A1–C2): give one when you make a course — what its words, \
  grammar and tasks demand — and give a lesson its own (update_lesson) only when it differs, \
  like a challenge a level up. A level you give always stands. Asked to add missing levels, \
  read each unlevelled course (get_course) and set its level with edit_course. A level goes in the level field (create_course, \
  edit_course, update_lesson) and the app shows it as a badge — never write it into a title. \
  Asked to level all courses, call edit_course with level for each one.
- Levels describe material; they never limit it. Make exactly what the learner asks for, at \
  whatever level they ask — never refuse, water down or second-guess a harder request. When no \
  level is asked for, fit what they have been doing; \"harder\" means a level up.
- Mastery is for good: a mastered lesson is never due again and never loses its mastery (its \
  words may come back now and then as a light review). When \
  every lesson of a course is mastered, it is ready to close; the learner closes it themselves \
  (\"Close as mastered\", their celebration) and it becomes a trophy. Closed is closed: never \
  add to or change a closed course. Revisiting a subject is your job, not the course's — when \
  a closed course is getting old, or the learner brings up its theme, offer a new course on it, \
  a level up: build on what it taught (get_course shows it), don't repeat its words and lessons.
- Grammar, culture, listening and writing lessons belong in the course whose material they \
  explain or use: the accusative in the bakery course, a logician's podcast in the logic course. \
  Give their course_id. Only a lesson that truly fits no course goes in \"Other lessons\" (the \
  folder for one-offs, which never ends). A lesson is in one course only and is never moved: \
  for one in another course, make a new lesson there.
- Culture and facts (cities, history, customs): write the study lesson from Wikipedia — wiki_search, \
  wiki_read — and attach the article to the lesson with attach_source (existing lessons too). \
  Keep to lasting facts; name current officeholders only \"as of <year>\". Give the lesson a \
  language hook (the city in \"w Krakowie\", \"z Krakowa\").
- Polish must be correct, with diacritics. Words in dictionary form, lowercase, with a short \
  gloss; sentences short and natural. With each sentence, list in `endings` its words that \
  stand in another form than their dictionary one (\"kawę\" for \"kawa\"), each with two \
  other real forms of the same word that do not fit there: the learner picks the ending when \
  building the sentence from its words. Where a word has a real Polish neighbour that differs \
  by one sound English ears mix up (\"proszę\" and \"prosię\", \"być\" and \"bić\"), give \
  it as the word's `near`: words lessons play the two for the learner to tell apart. \
  Give each word and sentence its `grammar`: the structures it shows, as keys from this list \
  (the learner's Progress page groups what they have met by them):{grammar}
  Give each word, sentence and lesson its `topics`: what it is about, as keys from this list \
  (a domain, or one of its topics):{topics}
  Give each sentence its `tokens`: the sentence word by word, one token per word in order \
  (punctuation left out), with its dictionary form, part of speech and — where they apply — \
  case, number, gender, person, tense, aspect, mood and the reason for its form, from the \
  values the tool lists.
- Tools that read the learner's answer log (get_learning_overview, find_hard_items, \
  get_item_history, get_structure_stats, get_mistakes, compare_periods, get_activity, \
  get_raw_log, get_context_stats, get_mastery_log) are available. Asked to tag the grammar, annotate the \
  sentences or tag the topics of existing courses, list_untagged gives what has none \
  (`missing`: grammar, tokens or topics), and tag_grammar, annotate_sentences and tag_topics \
  set them.
- Songs cost the learner credits: one per request. Give a new song or podcast the course it \
  fits (course_id): it is filed there, with its listening lesson. Put the course's level and the \
  words it should use in requirements — the songwriter writes at the level it is given (a \
  beginner's when none is).
- You have full control over songs and podcasts. make_song and make_podcast write and make one \
  in one go. When it has to meet something — words from a course (get_course for them), \
  grammar, line length, a structure — draft it first (draft_song, draft_podcast) with those \
  constraints as requirements, read the draft, fix anything that misses them yourself \
  (edit_song: lines, title, chorus, style, singer) or ask the writer again (revise_song), then \
  make it with sing_song. Don't ask the learner to approve it: they delete what they don't like. \
  Something already made is never changed: editing or revising it makes a new one based on it \
  (the one exception: set_podcast_sound redoes a podcast's sound in place). \
  Everything a song was made with is kept (get_song): to make one \"like\" another, reuse its \
  style, singer and requirements and set based_on.
- The Music tab also holds podcasts (make_podcast): short spoken episodes on a subject the \
  learner likes, written from Wikipedia articles (0–5), from today's Polish news (news: true — \
  \"what's going on in Poland?\"), or as a made-up story, told by a narrator, \
  two hosts or a cast of characters — each with a voice of their own. Two kinds: pairs \
  (each sentence in English, then in Polish) and immersion (only Polish, simpler sentences). \
  set_podcast_sound changes the pause before the Polish, repeats, slow Polish, voices and whether \
  the English is said. They need only the OpenRouter key, not kie.ai. list_songs and get_song \
  cover them too (kind \"podcast\"); get_song gives the articles they were written from.";

/// The whole prompt in one string, for tests.
#[cfg(test)]
pub(crate) fn system_prompt(conn: &mut Conn) -> Result<String> {
    // The lesson types come from their registry, so a new type is known to
    // the tutor without editing these rules.
    Ok(format!("{}\n\n{}", rules(), learner_state(conn)?))
}

/// The tutor's standing rules, with the lesson types from their registry —
/// a new type is known to the tutor without editing the rules. The same from
/// turn to turn, so it can be cached.
pub(crate) fn rules() -> String {
    format!(
        "{}\n- The lesson types:\n{}",
        RULES.replace("{grammar}", &crate::grammar::for_tutor()).replace("{topics}", &crate::topics::for_tutor()),
        crate::lesson_types::for_tutor()
    )
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
            b.lesson_type,
            b.course_title.clone().unwrap_or_default(),
            b.last_result.as_ref().map(|r| format!(", last check-in: {r}")).unwrap_or_default()
        )
    };
    let due: Vec<String> = overview.due.iter().take(15).map(line).collect();

    let listed: Vec<CourseId> = course_lessons::table.select(course_lessons::course_id).load(conn)?;
    let all: Vec<(CourseId, String, String, Option<i64>, Option<String>)> = courses_table::table
        .order(crate::lessons::course_order())
        .limit(80)
        .select((
            courses_table::id,
            courses_table::title_en,
            courses_table::title_pl,
            courses_table::closed_at,
            courses_table::level,
        ))
        .load(conn)?;
    let courses: Vec<String> = all
        .iter()
        .filter(|c| c.3.is_none())
        .map(|(id, title, title_pl, _, level)| {
            let lessons = listed.iter().filter(|&&c| c == *id).count();
            let level = level.as_deref().unwrap_or("level not set");
            format!("  - id {id}: {title} ({title_pl}), {level}, {lessons} lessons")
        })
        .collect();
    // Closed as mastered: finished for good, kept as trophies.
    let mastered: Vec<String> = all
        .iter()
        .filter_map(|(id, title, title_pl, closed, level)| {
            closed.map(|at| {
                let (y, m, d) = crate::commands::civil_date(at.div_euclid(86_400));
                let level = level.as_deref().unwrap_or("level not set");
                format!("  - id {id}: {title} ({title_pl}), {level}, mastered {y}-{m:02}-{d:02}")
            })
        })
        .collect();

    // Every study lesson, with the courses that list it.
    let mut studies: Vec<(LessonId, String, String, i64, Vec<(i64, i64, String)>)> = Vec::new();
    for (id, title, complexity, reps, course_ord, list_ord, course) in course_lessons::table
        .inner_join(lessons_table::table)
        .inner_join(courses_table::table)
        .filter(lessons_table::lesson_type.eq("study"))
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
        .load::<(LessonId, String, String, i64, i64, i64, String)>(conn)?
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

    let song_count: i64 = songs_table::table.filter(songs_table::status.ne("draft")).count().get_result(conn)?;
    let unsorted: i64 = course_lessons::table
        .inner_join(courses_table::table)
        .filter(courses_table::slug.eq(courses::OTHER))
        .count()
        .get_result(conn)?;
    let playlists = playlists::list(conn)?;

    let none = || "  (none)".to_string();
    // Spec: counsel.IDN-2
    let now = db::local_clock(conn);
    Ok(format!(
        "{now}\n\
         The learner right now:\n\
         - {xp} XP, streak {streak} days{done}, {seen} of {total} words met.\n\
         - Due now ({due_count}):\n{due}\n\
         - Study lessons:\n{studies}\n\
         - Courses:\n{courses}\n\
         - Mastered and closed (finished; never add to these):\n{mastered}\n\
         - Songs and podcasts: {song_count} in the Music tab{song_note}{unsorted}.\n\
         - Playlists: {playlists}.",
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
                "; {n} lesson{} in \"Other lessons\" with no course",
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

/// A word's or sentence's grammar tags: keys from the vocabulary.
fn grammar_prop() -> Value {
    json!({
        "type": "array",
        "items": { "type": "string", "enum": crate::grammar::keys() },
        "description": "The grammar structures it shows (one to four), from the list. A word: its own grammar (\"aspect-pair\", \"reflexive\", \"modal\", \"diminutives\"); a sentence: the cases and their triggers, tense, person, agreement it uses."
    })
}

/// A word's, sentence's or lesson's topics: keys from the ontology.
pub(crate) fn topics_prop() -> Value {
    json!({
        "type": "array",
        "items": { "type": "string", "enum": crate::topics::keys() },
        "description": "What it is about (one to three), from the topic list: a domain (\"food\") or one of its topics (\"food.drinks\")."
    })
}

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
                },
                "near": {
                    "type": "object",
                    "description": "Only when there is one: a real Polish word that differs from this one by a sound English ears mix up (sz/ś, cz/ć, ż/sz, rz/r, ą/o, ę/e, y/i, ł/l), e.g. for \"proszę\": {\"pl\": \"prosię\", \"en\": \"piglet\"}. The learner hears one and picks which. Leave it out otherwise; never a made-up word.",
                    "properties": {
                        "pl": { "type": "string" },
                        "en": { "type": "string" }
                    },
                    "required": ["pl", "en"]
                },
                "grammar": grammar_prop(),
                "topics": topics_prop()
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
                "en": { "type": "string" },
                "endings": {
                    "type": "array",
                    "description": "The sentence's words that stand in another form than their dictionary form (a case ending, a conjugated verb), for the word-order tiles: the learner sees the dictionary form and picks the ending. Leave out words already in their dictionary form.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "form": { "type": "string", "description": "The word exactly as it stands in the sentence, e.g. \"kawę\"" },
                            "base": { "type": "string", "description": "Its dictionary form, e.g. \"kawa\"" },
                            "wrong": { "type": "array", "items": { "type": "string" }, "description": "Two other real forms of the same word that are wrong here, e.g. [\"kawy\", \"kawą\"]" }
                        },
                        "required": ["form", "base", "wrong"]
                    }
                },
                "grammar": grammar_prop(),
                "topics": topics_prop(),
                "tokens": crate::grammar::token_schema()
            },
            "required": ["pl", "en"]
        }
    })
}

fn dialogue_schema() -> Value {
    json!({
        "type": "array",
        "description": "A short dialogue, one entry per line, alternating speakers. Each line responds to the one before it — answers its question, takes up its offer, reacts to what was just said — so that from the line before alone the learner can tell what comes next. Ask questions the other person answers; avoid remarks that could come anywhere.",
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
                    "level": { "type": "string", "enum": crate::levels::LEVELS, "description": "How advanced the material is, on the CEFR scale. Shown as a badge; never write it into the title." },
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
                "required": ["title", "title_pl", "summary", "level", "words", "sentences"]
            }),
        ),
        tool(
            "add_to_course",
            "Updating the course…",
            "Grow a course's own material: add words, sentences or dialogue lines to it; its drills are cut again to include them. (For a separate drill in a chosen format, use create_drill_lesson.)",
            json!({
                "type": "object",
                "properties": {
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
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
            "Rename a course, change its summary or level, or remove words/sentences from it (by their exact Polish text).",
            json!({
                "type": "object",
                "properties": {
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                    "level": { "type": "string", "enum": crate::levels::LEVELS, "description": "The course's CEFR level, shown as a badge on it. This is where a level goes — never into the title." },
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
                "properties": { "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$" } },
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
                "properties": { "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$" } },
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
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"], "description": "How much Polish it takes: low — a few simple sentences; medium — a short exchange; high — a real conversation. How simply the partner writes follows the course's level (or the lesson's own)." },
                    "scenario": { "type": "string", "description": "The situation in English, to the learner: where they are and who they are writing with, e.g. \"You've just moved in. Your new neighbour Pani Ewa stops you on the stairs.\"" },
                    "goals": { "type": "array", "items": { "type": "string" }, "description": "1–4 things the learner has to get across, e.g. \"Say which city you live in\", \"Say who you live with\"" },
                    "period_days": { "type": "integer", "description": "Days between check-ins; leave out for the default" },
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The course to file it in; leave out only for a one-off that fits no course (it goes in \"Other lessons\")" }
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
                    "lesson_id": { "type": "string", "pattern": "^ls_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
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
                "properties": { "source_id": { "type": "string", "pattern": "^src_[0-7][0-9a-hjkmnp-tv-z]{25}$" } },
                "required": ["source_id"]
            }),
        ),
        tool(
            "create_study_lesson",
            "Writing the lesson…",
            "Write a study lesson: a short lesson (conjugation, grammar, culture) with objectives. \
             The examiner interviews the learner on the objectives in each conversation.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short, e.g. \"Iść — to go (on foot)\"" },
                    "summary": { "type": "string", "description": "One sentence: what the lesson covers" },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"], "description": "By how hard the subject is: low ≈ 1–2 min, medium ≈ 5–10 min, high ≈ 20+ min" },
                    "content": { "type": "string", "description": "The lesson in markdown: explanation, a table or examples with translations. Correct Polish only. Write every Polish example as [[say: …]] so it can be heard, its translation outside the tag." },
                    "objectives": { "type": "array", "items": { "type": "string" }, "description": "2–5 things the learner should be able to do afterwards" },
                    "period_days": { "type": "integer", "description": "Days between check-ins; leave out for the default" },
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The course it explains: give it. Leave out only for a one-off that fits no course (it goes in \"Other lessons\")" }
                },
                "required": ["title", "summary", "complexity", "content", "objectives"]
            }),
        ),
        tool(
            "create_drill_lesson",
            "Making the drill…",
            "Make one drill lesson in a chosen format over words and/or sentences you give. \
             It is listed in a course as a lesson of its own, not cut into the course's drills (to \
             grow a course's own words and sentences, use add_to_course). Without a course it \
             goes in \"Other lessons\".",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "format": {
                        "type": "string",
                        "enum": crate::lesson_types::item_types().filter(|t| t.id != "dialogue").map(|t| t.id).collect::<Vec<_>>(),
                        "description": crate::lesson_types::item_types().filter(|t| t.id != "dialogue").map(|t| format!("{}: {}", t.id, t.description)).collect::<Vec<_>>().join(" "),
                    },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"] },
                    "words": word_schema(),
                    "sentences": sentence_schema(),
                    "period_days": { "type": "integer", "description": "Days until it first comes back (its check-in period); leave out for the default" },
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The course to list it in; leave out only for a one-off that fits no course (it goes in \"Other lessons\")" }
                },
                "required": ["title", "format", "complexity"]
            }),
        ),
        tool(
            "create_picture_lesson",
            "Making the picture lesson…",
            "Make a picture lesson: 4–10 everyday objects, each drawn as a picture; the learner \
             links each word with its picture (see the picture, pick the word; see the word, pick \
             the picture). Only concrete things you can point at (food, furniture, clothes, tools, \
             animals), each unmistakable on its own: no actions, qualities, places or ideas. \
             Pictures are drawn in the background (about 4 cents each).",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "words": word_schema(),
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The course to add it to; leave out only for a one-off that fits no course (it goes in \"Other lessons\")" }
                },
                "required": ["title", "words"]
            }),
        ),
        tool(
            "create_scene_lesson",
            "Making the scene lesson…",
            "Make a scene lesson: one illustrated scene (a market stall, a kitchen, a train station, a \
             doctor's waiting room…) with 6–10 things in it. The learner explores the picture (tap a \
             thing, hear its name), finds things by their Polish name, names things that light up \
             (typing them in medium and high lessons), and — if you add sentences — says where things \
             are. The things become the lesson's words. Use the course's own words where they fit \
             (same Polish and English). Only nouns you can point at, each unmistakable and drawn once. \
             The picture is drawn and its things found in the background (about 5 cents); the lesson \
             works as a word drill until then.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "scene": { "type": "string", "description": "The scene in English, 1–3 sentences: the place and what is going on, so it can be drawn. Every thing below must fit in it naturally." },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"], "description": "low: pick names from options; medium and high: type the names of things the learner knows. Default low." },
                    "things": word_schema(),
                    "statements": {
                        "type": "array",
                        "description": "Optional, 0–6 \"Where is it?\" sentences about where two of the things are in the scene, for learners ready for prepositions with cases. The picture is drawn to match, and a sentence it does not bear out is left out.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "pl": { "type": "string", "description": "e.g. \"Kot jest pod stołem.\"" },
                                "en": { "type": "string", "description": "e.g. \"The cat is under the table.\"" },
                                "subject": { "type": "string", "description": "The thing that is somewhere: its Polish exactly as in things, e.g. \"kot\"" },
                                "preposition": { "type": "string", "enum": crate::scenes::PREPOSITIONS },
                                "object": { "type": "string", "description": "The thing it is placed by: its Polish exactly as in things, e.g. \"stół\"" },
                                "phrase": { "type": "string", "description": "The words of pl that say where, exactly as written there, e.g. \"pod stołem\"" },
                                "wrong": { "type": "array", "items": { "type": "string" }, "description": "1–3 phrases for a clearly different place, correctly inflected, e.g. [\"na stole\", \"obok stołu\"] — never one that could also be true (not \"nad\" for \"na\")" }
                            },
                            "required": ["pl", "en", "subject", "preposition", "object", "phrase", "wrong"]
                        }
                    },
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The course to add it to; leave out only for a one-off that fits no course (it goes in \"Other lessons\")" }
                },
                "required": ["title", "scene", "things"]
            }),
        ),
        tool(
            "create_story_lesson",
            "Writing the story…",
            "Make a story lesson: a short illustrated story told panel by panel (4–8 panels), each a \
             picture with 1–3 Polish sentences under it, and a quick question after it. The learner reads \
             (and hears) it panel by panel, taps words for their meanings, answers the questions, then \
             practises the story's words. Write it at the lesson's level and build it from the course's \
             words (same Polish and English); list the words it teaches in words — they become the \
             lesson's words. Give 1–4 characters, each with a short description of how they look \
             (age, hair, clothes, colours; an animal's colour and markings): it goes into every picture's \
             prompt so they look the same throughout. A line someone says names its speaker (their \
             voice reads it); narration has none. Questions: who, what or where about that panel, in \
             English for a low lesson and in Polish for medium and high, with 1–3 plausible wrong \
             answers. The pictures are drawn in the background, panel by panel (about 4 cents each); the \
             story can be read meanwhile. Everything is checked first: if something is wrong, nothing is \
             made and you get the list to fix.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "theme": { "type": "string", "description": "What the story is about, in English, one or two sentences." },
                    "setting": { "type": "string", "description": "Where it happens, in English, for the pictures (e.g. \"a small Polish town in autumn\")." },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"], "description": "The lesson's level: low — short present-tense sentences and questions in English; medium and high — longer sentences, past tense, questions in Polish. Default low." },
                    "characters": {
                        "type": "array",
                        "description": "1–4 people or animals in the story.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "As written in the story, e.g. \"Ola\"" },
                                "look": { "type": "string", "description": "How they look, in English, short and concrete: \"a girl of about ten with a red raincoat and two dark plaits\"" },
                                "gender": { "type": "string", "enum": ["f", "m"], "description": "The voice their lines are read in." }
                            },
                            "required": ["name", "look", "gender"]
                        }
                    },
                    "panels": {
                        "type": "array",
                        "description": "4–8 panels, in story order.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "picture": { "type": "string", "description": "What the picture shows, in English: who is there, doing what, where. No text in it." },
                                "lines": {
                                    "type": "array",
                                    "description": "1–3 sentences under the picture.",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "pl": { "type": "string" },
                                            "en": { "type": "string" },
                                            "speaker": { "type": "string", "description": "The character who says it, by name; leave out for narration." },
                                            "practise": { "type": "boolean", "description": "Also keep it as one of the lesson's sentences to practise (at most 4 in the story)." }
                                        },
                                        "required": ["pl", "en"]
                                    }
                                },
                                "question": {
                                    "type": "object",
                                    "description": "Asked after this panel: who, what or where.",
                                    "properties": {
                                        "prompt": { "type": "string", "description": "e.g. \"Who is looking for the dog?\" or \"Gdzie śpi Burek?\"" },
                                        "answer": { "type": "string" },
                                        "wrong": { "type": "array", "items": { "type": "string" }, "description": "1–3 wrong answers, as long as the right one" }
                                    },
                                    "required": ["prompt", "answer", "wrong"]
                                }
                            },
                            "required": ["picture", "lines", "question"]
                        }
                    },
                    "words": word_schema(),
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The course to add it to; leave out only for a one-off that fits no course (it goes in \"Other lessons\")" }
                },
                "required": ["title", "theme", "characters", "panels", "words"]
            }),
        ),
        tool(
            "create_listening_lesson",
            "Making the listening lesson…",
            "Make a listening lesson for a song: the learner listens, then the examiner asks about what they heard (meaning, missing words, translation, grammar). One per song.",
            json!({
                "type": "object",
                "properties": {
                    "song_id": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"], "description": "low: the gist and the chorus; medium: lines and missing words; high: also the grammar of what is sung" },
                    "focus": { "type": "string", "description": "Optional: what to listen out for, e.g. \"the verbs in the first verse\"" },
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The course to file it in; leave out for the song's own course (or, if it has none, \"Other lessons\")" }
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
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "With which = course" }
                },
                "required": ["which"]
            }),
        ),
        tool(
            "get_lesson",
            "Looking at your lessons…",
            "One lesson in full: lesson and objectives or its words and sentences, its schedule, and the last conversation's verdict.",
            json!({
                "type": "object",
                "properties": { "lesson_id": { "type": "string", "pattern": "^ls_[0-7][0-9a-hjkmnp-tv-z]{25}$" } },
                "required": ["lesson_id"]
            }),
        ),
        tool(
            "update_lesson",
            "Updating the lesson…",
            "Change a lesson: title, summary, complexity, content, objectives, check-in period, \
             whether it is a challenge (beyond the course's level: needed to master the course, \
             not to complete it), or its own level (only when it differs from its course's; \
             empty to follow the course again).",
            json!({
                "type": "object",
                "properties": {
                    "lesson_id": { "type": "string", "pattern": "^ls_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "complexity": { "type": "string", "enum": ["low", "medium", "high"] },
                    "content": { "type": "string" },
                    "objectives": { "type": "array", "items": { "type": "string" } },
                    "period_days": { "type": "integer" },
                    "challenge": { "type": "boolean" },
                    "level": { "type": "string", "description": "A1–C2, or empty to follow the course" }
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
                "properties": { "lesson_id": { "type": "string", "pattern": "^ls_[0-7][0-9a-hjkmnp-tv-z]{25}$" } },
                "required": ["lesson_id"]
            }),
        ),
        tool(
            "remove_lesson_from_course",
            "Rearranging your courses…",
            "Take a lesson out of its course. A lesson is in one course only, so this deletes it with its words, sentences and history — only after the learner confirmed that.",
            json!({
                "type": "object",
                "properties": { "lesson_id": { "type": "string", "pattern": "^ls_[0-7][0-9a-hjkmnp-tv-z]{25}$" }, "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$" } },
                "required": ["lesson_id", "course_id"]
            }),
        ),
        tool(
            "order_course_lessons",
            "Rearranging your courses…",
            "Put a course's lessons in an order (lesson ids, first to last). Lessons left out keep their order after these.",
            json!({
                "type": "object",
                "properties": {
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                    "lesson_ids": { "type": "array", "items": { "type": "string", "pattern": "^ls_[0-7][0-9a-hjkmnp-tv-z]{25}$" } }
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
                "properties": { "lesson_id": { "type": "string", "pattern": "^ls_[0-7][0-9a-hjkmnp-tv-z]{25}$" } },
                "required": ["lesson_id"]
            }),
        ),
        tool(
            "suggest_lesson",
            "Finding the lesson…",
            "Show the learner a button that opens a lesson right now.",
            json!({
                "type": "object",
                "properties": { "lesson_id": { "type": "string", "pattern": "^ls_[0-7][0-9a-hjkmnp-tv-z]{25}$" } },
                "required": ["lesson_id"]
            }),
        ),
        tool(
            "update_practice_settings",
            "Updating your settings…",
            "Change how many lessons a day asks for (1–20; 5 by default). The most important ones are chosen each day; the day is done when they are.",
            json!({
                "type": "object",
                "properties": {
                    "lessons_per_day": { "type": "integer" }
                },
                "required": ["lessons_per_day"]
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
                    "requirements": { "type": "string", "description": "What the lyrics must meet, every point: words to use (e.g. a course's, from get_course), grammar, line length, structure. Kept with the song." },
                    "based_on": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The song this one varies (e.g. \"the same style as …\"): reuse its style and singer from get_song" },
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The course it belongs to: the song is filed under it, and so is its listening lesson" },
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
                    "about": { "type": "string", "description": "The subject, in English (for news: what to narrow it to, or empty for the day's mix)" },
                    "articles": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Exact English Wikipedia article titles to write from (0–5); leave out to use the best match for the subject"
                    },
                    "story": { "type": "boolean", "description": "A short made-up story, written from no articles" },
                    "news": { "type": "boolean", "description": "Today's news from Polish outlets (about the subject in 'about', if any — leave it empty for the day's mix), with Wikipedia for background; told by two hosts unless cast says otherwise" },
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
                    "requirements": { "type": "string", "description": "What the script must meet, every point: words to use, grammar, sentence length. Kept with it." },
                    "based_on": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The episode or song this one varies" },
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The course it belongs to: the song is filed under it, and so is its listening lesson" },
                    "voice": { "type": "string", "enum": ["f", "m"], "description": "The Polish voice; the English uses the other" },
                    "listening_lesson": { "type": "boolean", "description": "Also make a listening lesson for it (default true)" }
                },
                "required": []
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
                    "podcast_id": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
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
            "draft_song",
            "Writing the lyrics…",
            "Have the songwriter write a song without singing it yet: returns the draft (title, \
             chorus, style, singer, every line) to check against its requirements, change with \
             edit_song or revise_song, and make with sing_song. Free until sung.",
            json!({
                "type": "object",
                "properties": {
                    "about": { "type": "string", "description": "What the song is about, in English" },
                    "requirements": { "type": "string", "description": "What the lyrics must meet, every point" },
                    "based_on": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The song this one varies" },
                    "chorus_pl": { "type": "string", "description": "An exact Polish chorus sentence" },
                    "style": { "type": "string", "description": "Musical style" },
                    "vocal_gender": { "type": "string", "enum": ["f", "m", ""] },
                    "length": { "type": "string", "enum": ["short", "medium", "long"] }
                },
                "required": ["about"]
            }),
        ),
        tool(
            "draft_podcast",
            "Writing the script…",
            "Have a podcast script written without recording it yet: returns the draft to check, \
             change with edit_song or revise_song, and make with sing_song. Takes the same \
             arguments as make_podcast.",
            json!({
                "type": "object",
                "properties": {
                    "about": { "type": "string" },
                    "articles": { "type": "array", "items": { "type": "string" } },
                    "story": { "type": "boolean" },
                    "news": { "type": "boolean" },
                    "cast": { "type": "string", "enum": ["narrator", "hosts", "characters"] },
                    "format": { "type": "string", "enum": ["pairs", "immersion"] },
                    "length": { "type": "string", "enum": ["short", "medium", "long"] },
                    "voice": { "type": "string", "enum": ["f", "m"] },
                    "requirements": { "type": "string", "description": "What the script must meet, every point" },
                    "based_on": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$" }
                },
                "required": []
            }),
        ),
        tool(
            "revise_song",
            "Rewriting it…",
            "Ask the writer to rewrite a song's lyrics or a podcast's script: what to change, the \
             rest kept (its requirements still apply). A draft changes in place; one already made \
             gives a new draft based on it. Returns the draft.",
            json!({
                "type": "object",
                "properties": {
                    "song_id": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                    "feedback": { "type": "string", "description": "What to change" }
                },
                "required": ["song_id", "feedback"]
            }),
        ),
        tool(
            "edit_song",
            "Changing it…",
            "Change a song or podcast directly — only what is given changes. sections replaces all \
             the lyrics (a song: verse/chorus/bridge/intro/outro, each line Polish with English; a \
             podcast: its parts, lines with speaker). A draft changes in place; one already made \
             gives a new draft based on it. Returns the draft.",
            json!({
                "type": "object",
                "properties": {
                    "song_id": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                    "title": { "type": "string" },
                    "chorus_pl": { "type": "string" },
                    "chorus_en": { "type": "string" },
                    "style": { "type": "string", "description": "Song: the musical style for Suno" },
                    "vocal_gender": { "type": "string", "enum": ["f", "m", ""] },
                    "sections": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string" },
                                "lines": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "pl": { "type": "string" },
                                            "en": { "type": "string" },
                                            "speaker": { "type": "string" },
                                            "how": { "type": "string" }
                                        },
                                        "required": ["pl", "en"]
                                    }
                                }
                            },
                            "required": ["lines"]
                        }
                    },
                    "note": { "type": "string", "description": "Why, in a few words (kept with it)" }
                },
                "required": ["song_id"]
            }),
        ),
        tool(
            "sing_song",
            "Making it…",
            "Make a draft: a song is sung by Suno (2–4 minutes, costs kie.ai credits), a podcast is \
             read aloud (a few minutes). It then appears in the Music tab.",
            json!({
                "type": "object",
                "properties": {
                    "song_id": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                    "listening_lesson": { "type": "boolean", "description": "Also make a listening lesson for it (default true)" },
                    "course_id": { "type": "string", "pattern": "^crs_[0-7][0-9a-hjkmnp-tv-z]{25}$", "description": "The course it belongs to: the song is filed under it, and so is its listening lesson" }
                },
                "required": ["song_id"]
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
                "properties": { "song_id": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$" } },
                "required": ["song_id"]
            }),
        ),
        tool(
            "delete_songs",
            "Deleting songs…",
            "Permanently delete songs (all their takes). Only after the learner confirmed.",
            json!({
                "type": "object",
                "properties": { "song_ids": { "type": "array", "items": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$" } } },
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
                    "song_ids": { "type": "array", "items": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$" } }
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
                    "playlist_id": { "type": "string", "pattern": "^pl_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                    "song_ids": { "type": "array", "items": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$" } }
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
                    "playlist_id": { "type": "string", "pattern": "^pl_[0-7][0-9a-hjkmnp-tv-z]{25}$" },
                    "song_ids": { "type": "array", "items": { "type": "string", "pattern": "^sg_[0-7][0-9a-hjkmnp-tv-z]{25}$" } }
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
                "properties": { "playlist_id": { "type": "string", "pattern": "^pl_[0-7][0-9a-hjkmnp-tv-z]{25}$" }, "name": { "type": "string" } },
                "required": ["playlist_id", "name"]
            }),
        ),
        tool(
            "delete_playlist",
            "Updating your playlists…",
            "Delete a playlist (its songs stay in the library). Only after the learner confirmed.",
            json!({
                "type": "object",
                "properties": { "playlist_id": { "type": "string", "pattern": "^pl_[0-7][0-9a-hjkmnp-tv-z]{25}$" } },
                "required": ["playlist_id"]
            }),
        ),
    ]
    .into_iter()
    .chain(crate::tutor_analytics::tools())
    .map(|mut t| {
        // Every tool that makes lessons takes their topics too.
        if LESSON_MAKERS.contains(&t.name) {
            t.definition["function"]["parameters"]["properties"]["topics"] = topics_prop();
        }
        t
    })
    .collect()
}

/// The tools that make lessons (create_course: every lesson it makes):
/// their `topics` go on the lessons made.
const LESSON_MAKERS: &[&str] = &[
    "create_course",
    "create_writing_lesson",
    "create_study_lesson",
    "create_drill_lesson",
    "create_picture_lesson",
    "create_scene_lesson",
    "create_story_lesson",
    "create_listening_lesson",
];

/// What a tool produced: JSON for the model, and, when the app changed, an
/// action for the learner to see.
pub(crate) struct Outcome {
    result: Value,
    action: Option<TutorAction>,
}

impl Outcome {
    fn data(result: Value) -> Self {
        Outcome { result, action: None }
    }
    fn acted(result: Value, kind: &str, label: String, course_id: Option<CourseId>) -> Self {
        Outcome {
            result,
            action: Some(TutorAction { kind: kind.into(), label, course_id, song_id: None, lesson_id: None }),
        }
    }
    fn lesson(result: Value, kind: &str, label: String, lesson_id: LessonId) -> Self {
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
    fn song(result: Value, kind: &str, label: String, song_id: Option<SongId>) -> Self {
        Outcome {
            result,
            action: Some(TutorAction { kind: kind.into(), label, course_id: None, song_id, lesson_id: None }),
        }
    }
}

#[derive(Deserialize)]
struct CourseArg {
    #[serde(rename = "course_id", alias = "topic_id")]
    course_id: CourseId,
}

#[derive(Deserialize)]
struct CreateArgs {
    #[serde(flatten)]
    course: NewCourse,
    #[serde(default)]
    level: Option<String>,
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
    course_id: CourseId,
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
    course_id: CourseId,
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
    #[serde(default)]
    level: Option<String>,
}

#[derive(Deserialize)]
struct SettingsArgs {
    #[serde(default, alias = "daily_lessons")]
    lessons_per_day: Option<i64>,
}

#[derive(Deserialize)]
struct LessonArg {
    lesson_id: LessonId,
}

#[derive(Deserialize)]
struct MembershipArgs {
    lesson_id: LessonId,
    course_id: CourseId,
}

#[derive(Deserialize)]
struct OrderArgs {
    course_id: CourseId,
    #[serde(default)]
    lesson_ids: Vec<LessonId>,
}

#[derive(Deserialize)]
struct SourceArg {
    source_id: SourceId,
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
    lesson_id: Option<LessonId>,
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
    course_id: Option<CourseId>,
}

#[derive(Deserialize)]
struct PictureLessonArgs {
    title: String,
    #[serde(default)]
    summary: String,
    words: Vec<NewWord>,
    #[serde(default, alias = "topic_id")]
    course_id: Option<CourseId>,
}

#[derive(Deserialize)]
struct ListLessonsArgs {
    which: String,
    #[serde(default, rename = "course_id", alias = "topic_id")]
    course_id: Option<CourseId>,
}

#[derive(Deserialize)]
struct UpdateLessonArgs {
    lesson_id: LessonId,
    #[serde(default)]
    level: Option<String>,
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
        "type": lesson.lesson_type,
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
        "writing_support": (lesson.lesson_type == "writing").then(|| ["guided", "supported", "free"][crate::convert::index(lesson.support_level.clamp(0, 2))]),
    })
}


#[derive(Deserialize)]
struct SongArgs {
    about: String,
    /// Where its listening lesson is filed.
    #[serde(default, rename = "course_id", alias = "topic_id")]
    course_id: Option<CourseId>,
    #[serde(default)]
    requirements: String,
    #[serde(default)]
    based_on: Option<SongId>,
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
    #[serde(default)]
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
    news: bool,
    #[serde(default)]
    format: String,
    #[serde(default)]
    length: String,
    #[serde(default)]
    course_id: Option<CourseId>,
    #[serde(default)]
    voice: String,
    #[serde(default = "yes")]
    listening_lesson: bool,
    #[serde(default)]
    requirements: String,
    #[serde(default)]
    based_on: Option<SongId>,
}

#[derive(Deserialize)]
struct ReviseSongArgs {
    song_id: SongId,
    feedback: String,
}

#[derive(Deserialize)]
struct EditLine {
    #[serde(default)]
    pl: String,
    #[serde(default)]
    en: String,
    #[serde(default)]
    speaker: String,
    #[serde(default)]
    how: String,
}

#[derive(Deserialize)]
struct EditSection {
    #[serde(default)]
    kind: String,
    lines: Vec<EditLine>,
}

#[derive(Deserialize)]
struct EditSongArgs {
    song_id: SongId,
    title: Option<String>,
    chorus_pl: Option<String>,
    chorus_en: Option<String>,
    style: Option<String>,
    vocal_gender: Option<String>,
    sections: Option<Vec<EditSection>>,
    /// Why, in a few words: kept with the song.
    #[serde(default)]
    note: String,
}

#[derive(Deserialize)]
struct SingSongArgs {
    song_id: SongId,
    #[serde(default = "yes")]
    listening_lesson: bool,
    #[serde(default, alias = "topic_id")]
    course_id: Option<CourseId>,
}

#[derive(Deserialize)]
struct PodcastSoundArgs {
    podcast_id: SongId,
    pause: Option<f64>,
    repeats: Option<u32>,
    slow: Option<bool>,
    voices: Option<String>,
    english: Option<bool>,
}

#[derive(Deserialize)]
struct ListeningArgs {
    song_id: SongId,
    #[serde(default, alias = "topic_id")]
    course_id: Option<CourseId>,
    #[serde(default)]
    complexity: Option<String>,
    #[serde(default)]
    focus: Option<String>,
}

#[derive(Deserialize)]
struct SongArg {
    song_id: SongId,
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
fn song_placement(conn: &mut Conn, song_id: SongId) -> Result<Value> {
    let course: Option<(CourseId, String)> = songs_table::table
        .inner_join(courses_table::table)
        .filter(songs_table::id.eq(song_id))
        .select((courses_table::id, courses_table::title_en))
        .first(conn)
        .optional()?;
    let lesson: Option<LessonId> = lessons_table::table
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
    song_ids: Vec<SongId>,
}

#[derive(Deserialize)]
struct PlaylistArgs {
    #[serde(default)]
    playlist_id: Option<PlaylistId>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    song_ids: Vec<SongId>,
}

impl PlaylistArgs {
    fn id(&self) -> Result<PlaylistId> {
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
    serde_json::from_str(arguments).map_err(|e| {
        crate::diagnostics::unreadable_reply(&format!("A tool call's arguments ({e})"), arguments);
        Error::msg(format!(
            "The tool arguments were not valid: {e}. Ids are strings with their kind as a prefix \
             (a lesson is ls_…, a course crs_…, a song sg_…), copied exactly from a tool's results."
        ))
    })
}

/// Runs one tool call. Errors are returned to the model as text rather than
/// failing the turn, so it can correct itself or explain.
/// Runs one tool call as one transaction: a call that fails changes nothing
/// (a course is never left half made).
fn execute(conn: &mut Conn, name: &str, arguments: &str) -> Outcome {
    match conn.transaction(|tx| run_tool(tx, name, arguments)) {
        Ok(outcome) => outcome,
        Err(error) => Outcome::data(json!({ "error": error.to_string() })),
    }
}

fn run_tool(conn: &mut Conn, name: &str, arguments: &str) -> Result<Outcome> {
    // A lesson's topics are checked before anything is made (an unknown one
    // fails the call by name) and given to what was made after.
    let topics: Vec<&'static crate::topics::Topic> = if LESSON_MAKERS.contains(&name) {
        let keys: Vec<String> = serde_json::from_str::<Value>(arguments)
            .ok()
            .and_then(|v| v.get("topics").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        crate::topics::validate(&keys)?
    } else {
        vec![]
    };
    let outcome = run_tool_steps(conn, name, arguments)?;
    if !topics.is_empty() {
        if let Some(action) = &outcome.action {
            let lessons: Vec<LessonId> = match (action.lesson_id, action.course_id) {
                (Some(lesson), _) => vec![lesson],
                (None, Some(course)) if name == "create_course" => crate::schema::course_lessons::table
                    .filter(crate::schema::course_lessons::course_id.eq(course))
                    .select(crate::schema::course_lessons::lesson_id)
                    .load(conn)?,
                _ => vec![],
            };
            for lesson in lessons {
                crate::topics::set_lesson(conn, lesson, &topics)?;
            }
        }
    }
    Ok(outcome)
}

fn run_tool_steps(conn: &mut Conn, name: &str, arguments: &str) -> Result<Outcome> {
    if let Some(result) = crate::tutor_analytics::run(conn, name, arguments) {
        let result = result?;
        let plural = |n: i64, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
        let n = |key: &str| result[key].as_i64().unwrap_or(0);
        if name == "tag_grammar" {
            let label = format!("Tagged the grammar of {} and {}", plural(n("tagged_words"), "word"), plural(n("tagged_sentences"), "sentence"));
            return Ok(Outcome::acted(result, "courseUpdated", label, None));
        }
        if name == "tag_topics" {
            let label = format!(
                "Tagged the topics of {}, {} and {}",
                plural(n("tagged_lessons"), "lesson"),
                plural(n("tagged_words"), "word"),
                plural(n("tagged_sentences"), "sentence")
            );
            return Ok(Outcome::acted(result, "courseUpdated", label, None));
        }
        if name == "annotate_sentences" {
            let label = format!("Annotated {} word by word", plural(n("annotated"), "sentence"));
            return Ok(Outcome::acted(result, "courseUpdated", label, None));
        }
        return Ok(Outcome::data(result));
    }
    match name {
        "create_course" => {
            let args: CreateArgs = parse(arguments)?;
            let created = courses::create(conn, args.course, "tutor")?;
            let course_id = created.course_id;
            if let Some(level) = &args.level {
                crate::levels::set_course_level(conn, course_id, level)?;
            }
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
                json!({ "created": created, "total_lessons": lessons_made, "study_lessons_made": lessons, "writing": writing, "problems": problems }),
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
            // A closed course is not changed at all: check before writing
            // anything, so an edit never half-applies.
            courses::require_open_course(conn, args.course_id)?;
            if let Some(level) = &args.level {
                crate::levels::set_course_level(conn, args.course_id, level)?;
            }
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
                None => courses::other_course(conn)?,
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
        "create_picture_lesson" => {
            let args: PictureLessonArgs = parse(arguments)?;
            if args.words.len() < 4 {
                return Err(Error::msg("A picture lesson needs at least four objects, to choose between."));
            }
            let course_id = match args.course_id {
                Some(id) => id,
                None => courses::other_course(conn)?,
            };
            let (lesson, skipped) = lessons::create_drill(
                conn,
                course_id,
                &args.title,
                &args.summary,
                "pictures",
                "low",
                &courses::Items { words: args.words, sentences: vec![], dialogue: vec![] },
                None,
                "tutor",
            )?;
            Ok(Outcome::lesson(
                json!({
                    "created": lesson_json(&lesson),
                    "course_id": course_id,
                    "skipped_duplicates": skipped,
                    "pictures": "being drawn in the background; the lesson can be started now",
                }),
                "lessonCreated",
                format!("Made “{}” — a picture lesson, {} objects", lesson.title, lesson.items),
                lesson.id,
            ))
        }
        "create_scene_lesson" => {
            let args: crate::scenes::NewScene = parse(arguments)?;
            let lesson = crate::scenes::create(conn, args, "tutor")?;
            Ok(Outcome::lesson(
                json!({
                    "created": lesson_json(&lesson),
                    "scene": "being drawn in the background, then its things are found in it; until then the lesson is a word drill",
                }),
                "lessonCreated",
                format!("Made “{}” — a scene, {} things", lesson.title, lesson.items),
                lesson.id,
            ))
        }
        "create_story_lesson" => {
            let args: crate::stories::NewStory = parse(arguments)?;
            let panels = args.panels.len();
            let lesson = crate::stories::create(conn, args, "tutor")?;
            Ok(Outcome::lesson(
                json!({
                    "created": lesson_json(&lesson),
                    "story": "readable now; its pictures are drawn in the background, panel by panel",
                }),
                "lessonCreated",
                format!("Made “{}” — a story in {panels} pictures, {} words", lesson.title, lesson.items),
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
                    let ids: Vec<LessonId> = lessons_table::table
                        .filter(lessons_table::lesson_type.eq("study"))
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
                // A story lesson's story, page by page.
                "story": detail.story.as_ref().map(|s| json!({
                    "theme": s.theme,
                    "characters": s.characters,
                    "panels": s.panels.iter().map(|p| json!({
                        "picture": p.picture,
                        "drawn": p.status,
                        "lines": p.lines.iter().map(|l| json!({ "pl": l.pl, "en": l.en, "speaker": l.speaker })).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                })),
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
            if let Some(level) = &args.level {
                crate::levels::set_lesson_level(conn, args.lesson_id, Some(level))?;
            }
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
        "remove_lesson_from_course" => {
            let args: MembershipArgs = parse(arguments)?;
            let title = lessons::load(conn, args.lesson_id)?.title;
            let course = courses::require_course(conn, args.course_id)?;
            courses::remove_lesson(conn, args.course_id, args.lesson_id)?;
            Ok(Outcome::acted(
                json!({ "removed": title, "lesson_deleted": true }),
                "lessonDeleted",
                format!("Deleted “{title}” from “{course}”"),
                None,
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
            if let Some(count) = args.lessons_per_day {
                let count = count.clamp(1, 20);
                db::set_setting(conn, "daily_lessons", &count.to_string())?;
                db::delete_setting(conn, "daily_day")?;
                changed.push(format!("{count} lessons a day"));
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
            let list: Vec<Value> = songs::list_songs(conn, true)?
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
                "made_by": song.origin.made_by,
                "requirements": song.origin.requirements,
                "based_on": song.origin.based_on,
                "steps": song.origin.steps.iter().map(|s| json!({ "kind": s.kind, "by": s.by, "what": s.text })).collect::<Vec<_>>(),
                "given_to_writer": {
                    "words": song.origin.given.words,
                    "sentences": song.origin.given.sentences,
                    "weak_words": song.origin.given.weak_words,
                    "memories": song.origin.given.memories,
                },
                "instructions_version": song.origin.instructions_version,
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

fn course_detail(conn: &mut Conn, course_id: CourseId) -> Result<Value> {
    let course = session::load_course(conn, course_id)?;
    let rows: Vec<(WordId, String, String, String, LessonId)> = course_words::table
        .filter(course_words::course_id.eq(course_id))
        .order(course_words::ord)
        .select((course_words::id, course_words::pl, course_words::en, course_words::pos, course_words::lesson_id))
        .load(conn)?;
    let history: std::collections::HashMap<String, (i64, i64)> = reviews::table
        .filter(reviews::kind.eq("word"))
        .filter(reviews::item_id.eq_any(rows.iter().map(|r| r.0.to_string()).collect::<Vec<_>>()))
        .select((reviews::item_id, reviews::reps, reviews::lapses))
        .load::<(String, i64, i64)>(conn)?
        .into_iter()
        .map(|(id, reps, lapses)| (id, (reps, lapses)))
        .collect();
    let tags = crate::grammar::all_links(conn)?;
    let tag_keys = |id: &str| -> Vec<&'static str> {
        tags.get(id).map(|ts| ts.iter().filter_map(|t| crate::grammar::by_id(*t)).map(|t| t.key).collect()).unwrap_or_default()
    };
    let words: Vec<Value> = rows
        .into_iter()
        .map(|(id, pl, en, pos, lesson_id)| {
            let seen = history.get(&id.to_string());
            json!({
                "id": id,
                "grammar": tag_keys(&id.to_string()),
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
        .select((course_sentences::id, course_sentences::pl, course_sentences::en, course_sentences::lesson_id))
        .load::<(SentenceId, String, String, LessonId)>(conn)?
        .into_iter()
        .map(|(id, pl, en, lesson_id)| json!({ "id": id, "grammar": tag_keys(&id.to_string()), "pl": pl, "en": en, "lesson_id": lesson_id }))
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
    let met: HashSet<String> = reviews::table
        .filter(reviews::kind.eq("word"))
        .select(reviews::item_id)
        .load::<String>(conn)?
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
        .load::<(WordId, String, String, String, String, CourseId, LessonId)>(conn)?
        .into_iter()
        // A word in a lesson two courses list is one word.
        .filter(|row| seen.insert(row.0))
        .take(40)
        .map(|(id, pl, en, pos, course, course_id, lesson_id)| {
            json!({
                "pl": pl, "en": en, "pos": pos, "course": course, "course_id": course_id,
                "met": met.contains(&id.to_string()), "lesson_id": lesson_id,
            })
        })
        .collect();
    let mut seen = HashSet::new();
    let sentences: Vec<Value> = course_sentences::table
        .inner_join(courses_table::table)
        .filter(course_sentences::pl.like(&pattern).or(course_sentences::en.like(&pattern)))
        .select((course_sentences::id, course_sentences::pl, course_sentences::en, courses_table::title_en))
        .load::<(SentenceId, String, String, String)>(conn)?
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
        .inner_join(reviews::table.on(diesel::dsl::sql::<diesel::sql_types::Bool>("reviews.item_id = words.id").and(reviews::kind.eq("word"))))
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
        "total_xp": s.total_xp,
        "xp_today": s.day_xp,
        "lessons_per_day": s.daily_lessons,
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

/// A draft as the tutor reads it: everything it can change.
fn draft_view(id: SongId, draft: &songs::SongDraft) -> Value {
    json!({
        "song_id": id,
        "status": "draft — not made yet; sing_song makes it",
        "kind": if draft.kind == "podcast" { "podcast" } else { "song" },
        "title": draft.title,
        "chorus_pl": draft.chorus_pl,
        "chorus_en": draft.chorus_en,
        "style": draft.style,
        "singer": singer(&draft.vocal_gender),
        "requirements": draft.origin.requirements,
        "based_on": draft.origin.based_on,
        "sections": draft.sections.iter().map(|s| json!({
            "kind": s.kind,
            "lines": s.lines.iter().map(|l| {
                let mut line = json!({ "pl": l.pl, "en": l.en });
                if !l.speaker.is_empty() { line["speaker"] = json!(l.speaker); }
                if !l.how.is_empty() { line["how"] = json!(l.how); }
                line
            }).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn draft_outcome(app: &tauri::AppHandle, id: SongId, draft: &songs::SongDraft, label: String) -> Outcome {
    songs::notify(app, id);
    Outcome { result: draft_view(id, draft), action: Some(TutorAction { kind: "songDraft".into(), label, course_id: None, song_id: Some(id), lesson_id: None }) }
}

async fn draft_song(db: &Db, arguments: &str) -> Outcome {
    let result: Result<Outcome> = async {
        let args: SongArgs = parse(arguments)?;
        let draft = songs::write_draft(
            db,
            songs::DraftRequest {
                brief: args.about,
                requirements: args.requirements,
                based_on: args.based_on,
                made_by: "tutor".into(),
                chorus_pl: args.chorus_pl,
                style: args.style,
                vocal_gender: args.vocal_gender,
                length: args.length,
                ..Default::default()
            },
        )
        .await?;
        let draft = songs::normalize_draft(draft)?;
        let id = songs::insert_draft(&mut db.lock(), &draft)?;
        Ok(Outcome::data(draft_view(id, &draft)))
    }
    .await;
    result.unwrap_or_else(|error| Outcome::data(json!({ "error": error.to_string() })))
}

async fn draft_podcast(db: &Db, arguments: &str) -> Outcome {
    let result: Result<Outcome> = async {
        let args: PodcastArgs = parse(arguments)?;
        let request = crate::podcasts::PodcastRequest {
            topic: args.about,
            articles: if args.article.trim().is_empty() {
                args.articles
            } else {
                std::iter::once(args.article).chain(args.articles).collect()
            },
            story: args.story,
            cast: args.cast,
            news: args.news,
            format: args.format,
            length: args.length,
            vocal_gender: args.voice,
            requirements: args.requirements,
            based_on: args.based_on,
            made_by: "tutor".into(),
            ..Default::default()
        };
        let draft = crate::podcasts::write_episode(db, request).await?;
        let id = songs::insert_draft(&mut db.lock(), &draft)?;
        Ok(Outcome::data(draft_view(id, &draft)))
    }
    .await;
    result.unwrap_or_else(|error| Outcome::data(json!({ "error": error.to_string() })))
}

async fn revise_song(app: &tauri::AppHandle, db: &Db, arguments: &str) -> Outcome {
    let result: Result<Outcome> = async {
        let args: ReviseSongArgs = parse(arguments)?;
        let (id, previous) = songs::editable(&mut db.lock(), args.song_id, "tutor")?;
        let revised = if previous.kind == "podcast" {
            crate::podcasts::write_episode(
                db,
                crate::podcasts::PodcastRequest {
                    previous: Some(previous),
                    feedback: args.feedback,
                    made_by: "tutor".into(),
                    ..Default::default()
                },
            )
            .await?
        } else {
            let draft = songs::write_draft(
                db,
                songs::DraftRequest {
                    previous: Some(previous),
                    feedback: args.feedback,
                    made_by: "tutor".into(),
                    ..Default::default()
                },
            )
            .await?;
            songs::normalize_draft(draft)?
        };
        let id = songs::store_draft(&mut db.lock(), id, &revised)?;
        Ok(draft_outcome(app, id, &revised, format!("Rewrote “{}”", revised.title)))
    }
    .await;
    result.unwrap_or_else(|error| Outcome::data(json!({ "error": error.to_string() })))
}

fn edit_song(app: &tauri::AppHandle, db: &Db, arguments: &str) -> Outcome {
    let result: Result<Outcome> = (|| {
        let args: EditSongArgs = parse(arguments)?;
        let given = [args.title.is_some(), args.chorus_pl.is_some(), args.chorus_en.is_some(), args.style.is_some(), args.vocal_gender.is_some(), args.sections.is_some()];
        if !given.contains(&true) {
            return Err(Error::msg("Nothing to change: give a title, chorus, style, singer or sections."));
        }
        let mut guard = db.lock();
        let conn = &mut *guard;
        let (id, mut draft) = songs::editable(conn, args.song_id, "tutor")?;
        let before = songs::bare(&draft);
        let mut changed = Vec::new();
        if let Some(title) = args.title {
            draft.title = title;
            changed.push("title");
        }
        if let Some(chorus) = args.chorus_pl {
            draft.chorus_pl = chorus;
            changed.push("chorus");
        }
        if let Some(chorus) = args.chorus_en {
            draft.chorus_en = chorus;
            changed.push("chorus meaning");
        }
        if let Some(style) = args.style {
            draft.style = style;
            changed.push("style");
        }
        if let Some(gender) = args.vocal_gender {
            draft.vocal_gender = gender;
            changed.push("singer");
        }
        if let Some(sections) = args.sections {
            draft.sections = sections
                .into_iter()
                .map(|s| songs::SongSection {
                    kind: if s.kind.is_empty() { "verse".into() } else { s.kind },
                    lines: s
                        .lines
                        .into_iter()
                        .map(|l| songs::SongLine { pl: l.pl, en: l.en, speaker: l.speaker, how: l.how })
                        .collect(),
                })
                .collect();
            changed.push("lyrics");
        }
        let what = match args.note.trim() {
            "" => format!("Edited the {}", changed.join(", ")),
            note => format!("Edited the {}: {note}", changed.join(", ")),
        };
        draft.origin.steps.push(songs::OriginStep { kind: "edit".into(), by: "tutor".into(), text: what, model: String::new(), before });
        let draft = if draft.kind == "podcast" {
            crate::podcasts::normalize_episode(draft)?
        } else {
            songs::normalize_draft(draft)?
        };
        let id = songs::store_draft(conn, id, &draft)?;
        drop(guard);
        Ok(draft_outcome(app, id, &draft, format!("Changed “{}”", draft.title)))
    })();
    result.unwrap_or_else(|error| Outcome::data(json!({ "error": error.to_string() })))
}

/// Files a song (or podcast) under the course it was made for, when one is
/// given: its listening lesson then goes there too, now or later.
fn file_song(conn: &mut db::Conn, song_id: SongId, course_id: Option<CourseId>) -> Result<()> {
    if let Some(course_id) = course_id {
        courses::require_open_course(conn, course_id)?;
        diesel::update(crate::schema::songs::table.find(song_id))
            .set(crate::schema::songs::course_id.eq(course_id))
            .execute(conn)?;
    }
    Ok(())
}

fn sing_song(app: &tauri::AppHandle, db: &Db, arguments: &str) -> Outcome {
    let result: Result<Outcome> = (|| {
        let args: SingSongArgs = parse(arguments)?;
        songs::sing(app, db, args.song_id)?;
        let song = songs::load_song(&mut db.lock(), args.song_id)?;
        file_song(&mut db.lock(), song.id, args.course_id)?;
        let lesson = if args.listening_lesson {
            Some(lessons::create_listening(&mut db.lock(), song.id, None, None, args.course_id, "tutor")?)
        } else {
            None
        };
        let podcast = song.kind == "podcast";
        Ok(Outcome::song(
            json!({
                "song_id": song.id,
                "status": if podcast { "being read aloud; ready in a few minutes in the Music tab" } else { "Suno is singing it now; ready in 2–4 minutes" },
                "listening_lesson_id": lesson.as_ref().map(|l| l.id),
            }),
            "songStarted",
            if podcast { format!("Recording “{}”", song.title) } else { format!("Singing “{}”", song.title) },
            Some(song.id),
        ))
    })();
    result.unwrap_or_else(|error| Outcome::data(json!({ "error": error.to_string() })))
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
                requirements: args.requirements,
                based_on: args.based_on,
                made_by: "tutor".into(),
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
        file_song(&mut db.lock(), song_id, args.course_id)?;
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
            news: args.news,
            format: args.format,
            length: args.length,
            vocal_gender: args.voice,
            requirements: args.requirements,
            based_on: args.based_on,
            made_by: "tutor".into(),
            ..Default::default()
        };
        // Written and recorded in the background; the episode is in the
        // library already, as "being made".
        let song_id = crate::podcasts::start_from_request(app, db, request)?;
        file_song(&mut db.lock(), song_id, args.course_id)?;
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
            match (args.news, args.about.trim()) {
                (true, "") => "Making a podcast of today's news".to_string(),
                (true, about) => format!("Making a news podcast about {about}"),
                (false, about) => format!("Making a podcast about {about}"),
            },
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
        .load::<(LessonId, String, Option<String>, Option<String>)>(conn)?
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

pub fn load_history(conn: &mut Conn, conversation_id: ConversationId) -> Result<Vec<ChatMessage>> {
    Ok(tutor_messages::table
        .filter(tutor_messages::conversation_id.eq(conversation_id))
        .order(tutor_messages::id)
        .select((tutor_messages::id, tutor_messages::role, tutor_messages::content, tutor_messages::actions))
        .load::<(MessageId, String, String, Option<String>)>(conn)?
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
fn recent_history(conn: &mut Conn, conversation_id: ConversationId) -> Result<Vec<Wire>> {
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
    conversation_id: ConversationId,
    role: &str,
    content: &str,
    actions: &[TutorAction],
) -> Result<MessageId> {
    insert_message(conn, conversation_id, role, content, actions, None, &[])
}

/// Stores the tutor's answer with the model that wrote it and every tool
/// call it made, with its result.
// Spec: counsel.CHT-6
pub(crate) fn append_answer(
    conn: &mut Conn,
    conversation_id: ConversationId,
    content: &str,
    actions: &[TutorAction],
    model: Option<&str>,
    calls: &[crate::agent::CallRecord],
) -> Result<MessageId> {
    insert_message(conn, conversation_id, "assistant", content, actions, model, calls)
}

fn insert_message(
    conn: &mut Conn,
    conversation_id: ConversationId,
    role: &str,
    content: &str,
    actions: &[TutorAction],
    model: Option<&str>,
    calls: &[crate::agent::CallRecord],
) -> Result<MessageId> {
    let actions = if actions.is_empty() { None } else { Some(serde_json::to_string(actions)?) };
    let calls = if calls.is_empty() { None } else { Some(serde_json::to_string(calls)?) };
    let now = db::now_secs();
    let id = diesel::insert_into(tutor_messages::table)
        .values((
            tutor_messages::id.eq(MessageId::new()),
            tutor_messages::conversation_id.eq(conversation_id),
            tutor_messages::role.eq(role),
            tutor_messages::content.eq(content),
            tutor_messages::actions.eq(actions),
            tutor_messages::model.eq(model),
            tutor_messages::tool_calls.eq(calls),
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

pub fn create_conversation(conn: &mut Conn, first_message: &str) -> Result<ConversationId> {
    let now = db::now_secs();
    Ok(diesel::insert_into(tutor_conversations::table)
        .values((
            tutor_conversations::id.eq(ConversationId::new()),
            tutor_conversations::title.eq(title_from(first_message)),
            tutor_conversations::created_at.eq(now),
            tutor_conversations::updated_at.eq(now),
        ))
        .returning(tutor_conversations::id)
        .get_result(conn)?)
}

pub fn list_conversations(conn: &mut Conn) -> Result<Vec<Conversation>> {
    let heads: Vec<(ConversationId, String, i64, Option<i64>, Option<String>, Option<String>)> = tutor_conversations::table
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
pub async fn tutor_history(db: State<'_, Db>, conversation_id: ConversationId) -> Result<Vec<ChatMessage>> {
    load_history(&mut db.lock(), conversation_id)
}

#[tauri::command]
pub async fn tutor_rename(db: State<'_, Db>, conversation_id: ConversationId, title: String) -> Result<()> {
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
pub async fn tutor_delete(db: State<'_, Db>, conversation_id: ConversationId) -> Result<()> {
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

/// The switch that stops each reply being written, by conversation.
fn stops() -> &'static Mutex<HashMap<ConversationId, watch::Sender<bool>>> {
    static STOPS: OnceLock<Mutex<HashMap<ConversationId, watch::Sender<bool>>>> = OnceLock::new();
    STOPS.get_or_init(Mutex::default)
}

/// Stops the reply the tutor is writing in a conversation. What it wrote so
/// far is kept as the reply, and so is what it already did in the app. Does
/// nothing when no reply is being written.
// Spec: counsel.AGT-2
#[tauri::command]
pub async fn tutor_stop(conversation_id: ConversationId) -> Result<()> {
    if let Some(stop) = stops().lock().map_err(|_| Error::msg("The tutor's state is broken; restart the app."))?.get(&conversation_id) {
        stop.send_replace(true);
    }
    Ok(())
}

/// The stream id a conversation's background reply streams under, so a
/// screen opened later can follow it.
fn job_stream(conversation_id: ConversationId) -> String {
    format!("tutor-{conversation_id}")
}

/// Sent as `tutor://done` when a background reply is finished or failed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDone {
    pub conversation_id: ConversationId,
    pub message: Option<ChatMessage>,
    pub error: Option<String>,
}

/// What `tutor_send` returns at once: the conversation (new or not) and the
/// stored question. The reply follows in the background.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub conversation_id: ConversationId,
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
    conversation_id: Option<ConversationId>,
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
pub async fn tutor_retry(app: tauri::AppHandle, db: State<'_, Db>, conversation_id: ConversationId) -> Result<()> {
    start_job(&app, &db, conversation_id)
}

/// Marks the conversation busy and starts the reply in the background.
fn start_job(app: &tauri::AppHandle, db: &Db, conversation_id: ConversationId) -> Result<()> {
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
    let (stop, stopped) = watch::channel(false);
    if let Ok(mut stops) = stops().lock() {
        stops.insert(conversation_id, stop);
    }
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        let db = app.state::<Db>();
        let result = answer(&app, &db, conversation_id, stopped).await;
        if let Ok(mut stops) = stops().lock() {
            stops.remove(&conversation_id);
        }
        let done = {
            let mut guard = db.lock();
            let conn = &mut *guard;
            let error = result.as_ref().err().map(|e| e.to_string());
            if let Some(error) = &error {
                crate::diagnostics::note("tutor", format!("Conversation {conversation_id}: {error}"));
            }
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
    // Tapped, it opens the conversation (the app's address for it).
    // Spec: SHELL-20
    let _ = app
        .notification()
        .builder()
        .title(format!("Tutor: {title}"))
        .body(body)
        .extra("address", format!("/tutor/{}", done.conversation_id))
        .show();
}

/// Runs one reply to the conversation's last message and stores it.
async fn answer(
    app: &tauri::AppHandle,
    db: &Db,
    conversation_id: ConversationId,
    stop: watch::Receiver<bool>,
) -> Result<ChatMessage> {
    let stream_id = job_stream(conversation_id);
    let (wire, model, word_chat) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let mut prompt = rules();
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
        max_calls: Some(MAX_STEPS),
        stop: Some(stop),
        stop_once_answered: false,
        show_status: true,
        model,
        memory: true,
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
                } else if name == "draft_song" {
                    draft_song(db, &arguments).await
                } else if name == "draft_podcast" {
                    draft_podcast(db, &arguments).await
                } else if name == "revise_song" {
                    revise_song(app, db, &arguments).await
                } else if name == "edit_song" {
                    edit_song(app, db, &arguments)
                } else if name == "sing_song" {
                    sing_song(app, db, &arguments)
                } else if name == "set_podcast_sound" {
                    // Starts work in the background, which takes the lock itself.
                    set_podcast_sound(app, db, &arguments)
                } else if is_wiki_tool(&name) {
                    run_wiki_tool(db, &name, &arguments).await
                } else {
                    let (done, wanted) = run_app_tool(db, &name, &arguments);
                    // Work in the background starts once the lock is free:
                    // each of these takes it too, and the database's lock is
                    // not reentrant (taken twice here, it would wait for
                    // itself forever, and every screen with it).
                    for course_id in wanted {
                        crate::pictures::draw_in_background(app, course_id);
                    }
                    // A course kept offline has its new lessons' audio fetched.
                    if done.action.is_some() {
                        crate::offline::keep_up_in_background(app);
                    }
                    match name.as_str() {
                        "create_picture_lesson" => crate::pictures::draw_missing_in_background(app),
                        "create_scene_lesson" => crate::scenes::resume_pending(app),
                        "create_story_lesson" => crate::stories::resume_pending(app),
                        _ => {}
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
    if outcome.stopped {
        // What it wrote and did before the learner stopped it stays.
        if !reply.is_empty() {
            reply.push_str("\n\n");
        }
        reply.push_str("_(Stopped.)_");
    } else if let Some(error) = outcome.failed {
        // Even when the conversation fails half-way, whatever the tools
        // already did in the app is real, so it is recorded and shown.
        if actions.is_empty() {
            // Noted in Diagnostics by the job (start_job).
            return Err(error);
        }
        crate::diagnostics::note("tutor", format!("Conversation {conversation_id}: {error}"));
        reply.push_str(&format!("\n\n_(The answer was cut short: {error})_"));
    }
    if reply.trim().is_empty() {
        reply = "Done.".into();
    }
    let mut guard = db.lock();
    let conn = &mut *guard;
    let model = Some(outcome.model.as_str()).filter(|m| !m.is_empty());
    let id = append_answer(conn, conversation_id, &reply, &actions, model, &outcome.calls)?;
    Ok(ChatMessage { id, conversation_id, role: "assistant".into(), content: reply, actions })
}

/// Runs one of the tools that only write to the database, in one
/// transaction, under the lock — released when this returns. Returns the
/// courses it made that want a picture (`pictures::want`, recorded here so
/// the drawing survives the app closing before it is done).
pub(crate) fn run_app_tool(db: &Db, name: &str, arguments: &str) -> (Outcome, Vec<CourseId>) {
    let mut guard = db.lock();
    let conn = &mut *guard;
    // Every new course gets a picture, whichever tool made it.
    let before = course_ids(conn);
    let done = execute(conn, name, arguments);
    let mut wanted = Vec::new();
    for course_id in course_ids(conn).difference(&before) {
        match crate::pictures::want(conn, *course_id) {
            Ok(true) => wanted.push(*course_id),
            Ok(false) => {}
            Err(error) => crate::diagnostics::note("background", format!("Could not ask for course {course_id}'s picture: {error}")),
        }
    }
    (done, wanted)
}

fn course_ids(conn: &mut Conn) -> HashSet<CourseId> {
    courses_table::table.select(courses_table::id).load::<CourseId>(conn).map(|ids| ids.into_iter().collect()).unwrap_or_default()
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
- Grammar only when it helps them say the next thing. If practice on it would help, offer a \
  lesson in one line; make it when they say yes."
    )
}

/// What \"Talk about it\" opens.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalkAbout {
    pub conversation_id: ConversationId,
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
    item_id: String,
    // For a line of a conversation: its Polish and meaning.
    polish: Option<String>,
    meaning: Option<String>,
) -> Result<TalkAbout> {
    let conversation_id = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let (polish, meaning): (String, String) = match item_kind.as_str() {
            "word" => crate::schema::words::table
                .find(WordId::parse(&item_id).map_err(|e| Error::msg(e.to_string()))?)
                .select((crate::schema::words::pl, crate::schema::words::en))
                .first(conn)?,
            "sentence" => crate::schema::sentences::table
                .find(SentenceId::parse(&item_id).map_err(|e| Error::msg(e.to_string()))?)
                .select((crate::schema::sentences::pl, crate::schema::sentences::en))
                .first(conn)?,
            // A line of a conversation, or a word of a song or a story (its
            // line as the meaning: the word itself is what is not known).
            "line" | "lyric" | "story" => match (polish, meaning) {
                (Some(pl), Some(en)) if !pl.trim().is_empty() => (pl.trim().to_string(), en.trim().to_string()),
                _ => return Err(Error::msg("A line needs its Polish and meaning.")),
            },
            other => return Err(Error::msg(format!("Cannot talk about a '{other}'."))),
        };
        let about = format!("{polish} — {meaning}");
        let existing: Option<ConversationId> = tutor_conversations::table
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
        } else if item_kind == "lyric" {
            format!("What does “{polish}” mean in this line of a song: {meaning}?")
        } else if item_kind == "story" {
            format!("What does “{polish}” mean in this sentence of a story I am reading: {meaning}?")
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
///
/// Only replies this device was writing (it set `job_started_at`, so it
/// wrote the conversation's current version: `sync::written_here`). One
/// another device is writing right now synced here as started; marking it
/// failed would tell that device its own reply had died.
pub fn recover_interrupted(conn: &mut Conn) -> Result<()> {
    let running: Vec<ConversationId> = tutor_conversations::table
        .filter(tutor_conversations::job_started_at.is_not_null())
        .select(tutor_conversations::id)
        .load(conn)?;
    let ours: Vec<ConversationId> =
        running.into_iter().filter(|id| crate::sync::written_here(conn, "tutor_conversations", &id.to_string())).collect();
    diesel::update(tutor_conversations::table.filter(tutor_conversations::id.eq_any(ours)))
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
