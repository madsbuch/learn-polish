//! What commands return to the frontend. Some are read straight from the
//! database: those derive Diesel's `Queryable` (by position) or `Selectable`
//! (by column, checked against `crate::schema` at compile time).

use diesel::prelude::*;
use serde::Serialize;

use crate::schema::{conversation_lines, courses};
#[allow(unused_imports)]
use crate::ids::*;

#[derive(Debug, Clone, Serialize, Queryable, Selectable)]
#[diesel(table_name = courses, check_for_backend(diesel::sqlite::Sqlite))]
#[serde(rename_all = "camelCase")]
pub struct Course {
    pub id: CourseId,
    pub ord: i64,
    pub slug: String,
    /// Always "course" now (older databases told built-in courses from
    /// "programme"s; they were merged).
    pub kind: String,
    pub title_pl: String,
    /// The English title.
    #[diesel(column_name = title_en)]
    pub title: String,
    #[diesel(column_name = summary_en)]
    pub summary: String,
    /// How advanced it is: A1–C2 (levels.rs). None until it is levelled.
    pub level: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Word {
    pub id: WordId,
    pub pl: String,
    pub meaning: String,
    pub pos: String,
    /// The drill it belongs to, and that drill's title.
    pub lesson_id: LessonId,
    pub lesson: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sentence {
    pub id: SentenceId,
    pub pl: String,
    pub meaning: String,
    pub lesson_id: LessonId,
    pub lesson: String,
}

#[derive(Debug, Clone, Serialize, Queryable, Selectable)]
#[diesel(table_name = conversation_lines, check_for_backend(diesel::sqlite::Sqlite))]
#[serde(rename_all = "camelCase")]
pub struct DialogueLine {
    pub ord: i64,
    pub speaker: String,
    /// "f" or "m" — decides which voice reads the line.
    pub gender: String,
    pub pl: String,
    #[diesel(column_name = en)]
    pub meaning: String,
}

/// A page of catalogue results, with the totals behind them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CataloguePage {
    pub words: Vec<Word>,
    pub sentences: Vec<Sentence>,
    pub total_words: i64,
    pub total_sentences: i64,
}

/// One thing to do in a session. The backend decides the exercise type from the
/// item's review history, so the UI only has to render six shapes.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exercise {
    pub key: String,
    /// intro | recognise | produce | type | sentenceIntro | wordOrder |
    /// dialogueGap | gapFill | memory | pictureIntro | wordPick |
    /// picturePick | dictation | soundPair | sceneExplore | sceneFind |
    /// sceneName | sceneType | sceneWhere | storyPanel | storyQuestion |
    /// storyEnd | sayIt | dialogueSay
    pub kind: String,
    /// word | sentence | dialogue | set (several items at once, not scheduled)
    pub item_kind: String,
    /// The word, sentence or lesson it is about (a TypeID).
    pub item_id: String,
    pub is_new: bool,
    /// What the learner is shown.
    pub prompt: String,
    /// Extra context under the prompt (part of speech, speaker name, …).
    pub hint: String,
    /// The expected answer, in the language the learner must produce.
    pub answer: String,
    /// Multiple-choice options, already shuffled and including the answer.
    pub options: Vec<String>,
    /// Shuffled tokens for word-order exercises.
    pub tokens: Vec<String>,
    pub pl: String,
    pub meaning: String,
    /// Fill-the-gaps: the sentences, each with one word missing. The word
    /// bank is `options`.
    pub gaps: Vec<Gap>,
    /// Memory cards: the Polish/English pairs to find.
    pub pairs: Vec<Pair>,
    /// The item's picture (a file id), when it has one.
    pub image: Option<FileId>,
    /// Picture lessons: each option's picture, in the order of `options`.
    pub option_images: Vec<Option<FileId>>,
    /// Dialogue gaps: the conversation just before the gap, oldest first —
    /// enough to know where you are without remembering the whole script.
    pub context: Vec<DialogueLine>,
    /// Word order: the tiles whose ending is to be picked (tiles.rs); empty
    /// for a sentence built by plain ordering.
    pub endings: Vec<crate::tiles::TileEnding>,
    /// Scene lessons: the picture and where each thing is in it (scenes.rs).
    pub scene: Option<crate::scenes::SceneCard>,
    /// Story lessons: the panel to read, what a question is about, or the
    /// story's words at its end (stories.rs).
    pub story: Option<crate::stories::StoryCard>,
    /// A spoken dialogue turn (dialogueSay): the other speaker's answer to
    /// the line, said in their voice once it has been said.
    pub reply: Option<DialogueLine>,
}

/// One sentence with a word taken out of it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Gap {
    pub before: String,
    pub answer: String,
    pub after: String,
    /// The whole sentence in English.
    pub meaning: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pair {
    pub pl: String,
    pub en: String,
    /// Picture lessons: the card shows this picture instead of the English.
    pub image: Option<FileId>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPlan {
    pub day: i64,
    pub course: Course,
    pub dialogue: Vec<DialogueLine>,
    pub exercises: Vec<Exercise>,
    pub new_words: usize,
    pub review_words: usize,
    pub new_sentences: usize,
    pub review_sentences: usize,
    /// True when today's session has already been finished.
    pub already_completed: bool,
    /// The drill lesson this session practises.
    pub lesson_id: LessonId,
    /// Done before and not due: extra practice — half XP, schedule unchanged.
    pub extra: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub code: String,
    pub title: String,
    /// What it is about, in a line.
    pub description: String,
    /// Exactly what to do to earn it, in plain words.
    pub requirement: String,
    pub icon: String,
    pub unlocked: bool,
    /// How far along: `current` of `target` (in `unit`s), from the same
    /// numbers the unlock check reads. Full once unlocked.
    pub current: i64,
    pub target: i64,
    /// What is counted, singular: "day", "word", "lesson".
    pub unit: String,
    /// The day it was unlocked (a day number, like `Stats::day`).
    pub unlocked_day: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    pub day: i64,
    pub xp: i64,
    pub answered: i64,
    pub correct: i64,
    pub completed: bool,
    /// Missed, but a streak freeze kept the run going.
    pub frozen: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseProgress {
    pub course: Course,
    pub total_words: i64,
    pub seen_words: i64,
    pub mastered_words: i64,
    pub total_sentences: i64,
    pub seen_sentences: i64,
    pub total_lessons: i64,
    /// Lessons done at least once.
    pub started_lessons: i64,
    /// The course's picture: a file id, shown through the media protocol.
    pub image_file_id: Option<FileId>,
    /// Active, completed or mastered, and how far along.
    #[serde(flatten)]
    pub status: crate::courses::CourseStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub day: i64,
    pub total_xp: i64,
    pub day_xp: i64,
    /// Lessons a day (see plan.rs).
    pub daily_lessons: i64,
    pub level: i64,
    pub xp_into_level: i64,
    pub xp_for_next_level: i64,
    pub current_streak: i64,
    /// Freezes kept: each covers one missed day (at most two).
    pub streak_freezes: i64,
    pub longest_streak: i64,
    pub words_seen: i64,
    pub words_mastered: i64,
    pub words_total: i64,
    pub sentences_seen: i64,
    pub sentences_total: i64,
    pub dialogues_seen: i64,
    pub dialogues_total: i64,
    pub due_today: i64,
    pub accuracy: f64,
    pub completed_today: bool,
    pub history: Vec<DayStat>,
    pub achievements: Vec<Achievement>,
    /// What the courses closed as mastered add up to, each counted once.
    pub mastered: MasteredSummary,
}

/// The Mastered section's header: courses closed as mastered, their lessons
/// and the words those lessons own.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MasteredSummary {
    pub courses: i64,
    pub lessons: i64,
    pub words: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Lessons a day: how many today's list holds (see plan.rs).
    pub daily_lessons: i64,
    pub reminder_hour: i64,
    pub reminder_minute: i64,
    pub reminders_enabled: bool,
    pub has_api_key: bool,
    /// Empty until the learner loads the catalogue and picks one.
    pub ai_model: String,
    pub tts_voice_female: String,
    pub tts_voice_male: String,
    pub tts_speed: f32,
    /// Cache a session's audio up front so taps play instantly.
    pub tts_prefetch: bool,
    pub tz_offset_minutes: i64,
    /// Writes song lyrics; empty means "same as the tutor model".
    pub songwriter_model: String,
    /// Plays the other side in writing lessons; empty means the tutor model.
    pub partner_model: String,
    /// "Talk about it" chats; empty means the tutor model.
    pub word_chat_model: String,
    /// Models starred in the model picker, shown first.
    pub favorite_models: Vec<String>,
    /// Draws course pictures; empty means the default image model.
    pub image_model: String,
    /// Finds the things in a scene lesson's picture; empty means the default.
    pub vision_model: String,
    /// The model that hears the learner speak; empty for the default.
    pub hearing_model: String,
    /// Speaking cards in lessons (a setting of this device: a phone in a
    /// quiet room, a computer with no microphone).
    pub speaking: bool,
    pub has_kie_key: bool,
    /// The Suno version songs are sung with.
    pub suno_model: String,
    pub suno_models: Vec<String>,
    /// "short" | "medium" | "long" — what the songwriter writes for.
    pub song_length: String,
    /// "system" | "light" | "dark", or empty if never stored (the webview's
    /// own copy is then written back).
    pub theme: String,
    /// The music player's repeat mode: "off" | "all" | "one", or empty.
    pub player_repeat: String,
    /// Taps and buzzes with the answers, on a phone (a device setting).
    pub haptics: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    pub role: String,
    pub content: String,
    /// What the tutor did in the app during this turn.
    pub actions: Vec<TutorAction>,
}

/// A conversation with the tutor, as listed on the Tutor tab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: ConversationId,
    pub title: String,
    /// The last thing said, shortened, for the list.
    pub preview: String,
    pub messages: i64,
    pub updated_at: i64,
    /// The tutor is working on a reply in the background.
    pub working: bool,
    /// Why the last reply failed, if it did.
    pub error: Option<String>,
    /// "word" for a chat about a word or sentence; None for an ordinary one.
    pub tag: Option<String>,
}

/// Something the tutor did in the app, shown under its reply (and, for a
/// course, with a button to open it).
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TutorAction {
    /// courseCreated | courseUpdated | courseDeleted | settingsChanged |
    /// songStarted | songsDeleted | lesson… | memory. Older messages may hold
    /// programmeCreated / programmeUpdated / programmeDeleted.
    pub kind: String,
    /// One line for the learner, e.g. "Created “Colours” — 24 words".
    pub label: String,
    #[serde(default)]
    pub course_id: Option<CourseId>,
    /// Set when the action is about a song (one the tutor started making).
    #[serde(default)]
    pub song_id: Option<SongId>,
    /// Set when the action is about a lesson.
    #[serde(default)]
    pub lesson_id: Option<LessonId>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub id: String,
    pub name: String,
    pub context_length: i64,
    /// Dollars per input token, as OpenRouter writes it ("0.0000003").
    pub prompt_price: String,
    /// Dollars per output token.
    pub completion_price: String,
    /// Dollars per input token read from the provider's cache; empty when
    /// not reported.
    pub cache_read_price: String,
    /// When it was added to OpenRouter (seconds since the epoch).
    pub created: i64,
    /// Non-empty for speech models: the voice names the model accepts.
    pub voices: Vec<String>,
    /// True when the model can call tools, which the tutor needs to act in
    /// the app.
    pub tools: bool,
    /// True when the model can make images (course pictures).
    pub images: bool,
    /// True when the model can read images (finding things in a scene).
    pub vision: bool,
    /// True when the model takes sound in (hearing the learner speak).
    pub hears: bool,
}

/// One clip of generated speech, ready to become a `data:` URI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClip {
    pub mime: String,
    pub base64: String,
    /// True when this replay cost nothing because it came from the cache.
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioCacheStats {
    pub clips: i64,
    pub bytes: i64,
    /// How many of the app's 200 sentences already have audio stored.
    pub sentences_cached: i64,
    pub sentences_total: i64,
}

/// A selectable voice, with the character Google publishes for it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub name: String,
    pub gender: String,
    pub description: String,
}

/// Progress of a bulk pre-cache run, emitted as a `tts://progress` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheProgress {
    pub done: i64,
    pub total: i64,
    pub failed: i64,
    pub finished: bool,
    pub error: Option<String>,
}
