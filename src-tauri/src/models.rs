use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub id: i64,
    pub ord: i64,
    pub slug: String,
    pub title_pl: String,
    /// Already resolved to the learner's source language by the backend.
    pub title: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Word {
    pub id: i64,
    pub pl: String,
    pub meaning: String,
    pub pos: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sentence {
    pub id: i64,
    pub pl: String,
    pub meaning: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogueLine {
    pub ord: i64,
    pub speaker: String,
    pub pl: String,
    pub meaning: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dialogue {
    pub topic: Topic,
    pub lines: Vec<DialogueLine>,
}

/// One thing to do in a session. The backend decides the exercise type from the
/// item's review history, so the UI only has to render six shapes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exercise {
    pub key: String,
    /// intro | recognise | produce | type | sentenceIntro | wordOrder | dialogueGap
    pub kind: String,
    /// word | sentence | dialogue
    pub item_kind: String,
    pub item_id: i64,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPlan {
    pub day: i64,
    pub topic: Topic,
    pub dialogue: Vec<DialogueLine>,
    pub exercises: Vec<Exercise>,
    pub new_words: usize,
    pub review_words: usize,
    pub new_sentences: usize,
    pub review_sentences: usize,
    /// True when today's session has already been finished.
    pub already_completed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub code: String,
    pub title: String,
    pub description: String,
    pub icon: String,
    pub unlocked: bool,
    /// 0.0–1.0 toward unlocking.
    pub progress: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    pub day: i64,
    pub xp: i64,
    pub answered: i64,
    pub correct: i64,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicProgress {
    pub topic: Topic,
    pub total_words: i64,
    pub seen_words: i64,
    pub mastered_words: i64,
    pub total_sentences: i64,
    pub seen_sentences: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub day: i64,
    pub total_xp: i64,
    pub day_xp: i64,
    pub daily_goal: i64,
    pub level: i64,
    pub xp_into_level: i64,
    pub xp_for_next_level: i64,
    pub current_streak: i64,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub source_language: String,
    pub daily_goal: i64,
    pub new_words_per_day: i64,
    pub reminder_hour: i64,
    pub reminder_minute: i64,
    pub reminders_enabled: bool,
    pub has_api_key: bool,
    pub ai_model: String,
    pub tz_offset_minutes: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub id: String,
    pub name: String,
    pub context_length: i64,
    pub prompt_price: String,
}
