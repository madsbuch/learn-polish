use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub id: i64,
    pub ord: i64,
    pub slug: String,
    /// Always "course" now (older databases told built-in topics from
    /// "programme"s; they were merged).
    pub kind: String,
    pub title_pl: String,
    /// The English title.
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
    /// "f" or "m" — decides which voice reads the line.
    pub gender: String,
    pub pl: String,
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
    /// intro | recognise | produce | type | sentenceIntro | wordOrder |
    /// dialogueGap | gapFill | memory
    pub kind: String,
    /// word | sentence | dialogue | set (several items at once, not scheduled)
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
    /// Fill-the-gaps: the sentences, each with one word missing. The word
    /// bank is `options`.
    pub gaps: Vec<Gap>,
    /// Memory cards: the Polish/English pairs to find.
    pub pairs: Vec<Pair>,
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
    /// The drill block this session practises.
    pub block_id: i64,
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
    pub total_blocks: i64,
    /// Blocks done at least once.
    pub started_blocks: i64,
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
    pub daily_goal: i64,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: i64,
    pub conversation_id: i64,
    pub role: String,
    pub content: String,
    /// What the tutor did in the app during this turn.
    pub actions: Vec<TutorAction>,
}

/// A conversation with the tutor, as listed on the Tutor tab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: i64,
    pub title: String,
    /// The last thing said, shortened, for the list.
    pub preview: String,
    pub messages: i64,
    pub updated_at: i64,
}

/// Something the tutor did in the app, shown under its reply (and, for a
/// course, with a button to open it).
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TutorAction {
    /// courseCreated | courseUpdated | courseDeleted | settingsChanged |
    /// songStarted | songsDeleted | block… | memory. Older messages may hold
    /// programmeCreated / programmeUpdated / programmeDeleted.
    pub kind: String,
    /// One line for the learner, e.g. "Created “Colours” — 24 words".
    pub label: String,
    #[serde(default)]
    pub topic_id: Option<i64>,
    /// Set when the action is about a song (one the tutor started making).
    #[serde(default)]
    pub song_id: Option<i64>,
    /// Set when the action is about a block.
    #[serde(default)]
    pub block_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub id: String,
    pub name: String,
    pub context_length: i64,
    pub prompt_price: String,
    /// Non-empty for speech models: the voice names the model accepts.
    pub voices: Vec<String>,
    /// True when the model can call tools, which the tutor needs to act in
    /// the app.
    pub tools: bool,
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
