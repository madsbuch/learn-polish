// Mirrors the serde structs in src-tauri/src/models.rs.

export type SourceLanguage = "en" | "da";

export interface Topic {
  id: number;
  ord: number;
  slug: string;
  titlePl: string;
  title: string;
  summary: string;
}

export interface Word {
  id: number;
  pl: string;
  meaning: string;
  pos: string;
}

export interface Sentence {
  id: number;
  pl: string;
  meaning: string;
}

export interface DialogueLine {
  ord: number;
  speaker: string;
  pl: string;
  meaning: string;
}

export interface Dialogue {
  topic: Topic;
  lines: DialogueLine[];
}

export type ExerciseKind =
  | "dialogueRead"
  | "intro"
  | "recognise"
  | "produce"
  | "type"
  | "sentenceIntro"
  | "wordOrder"
  | "dialogueGap";

export interface Exercise {
  key: string;
  kind: ExerciseKind;
  itemKind: "word" | "sentence" | "dialogue";
  itemId: number;
  isNew: boolean;
  prompt: string;
  hint: string;
  answer: string;
  options: string[];
  tokens: string[];
  pl: string;
  meaning: string;
}

export interface SessionPlan {
  day: number;
  topic: Topic;
  dialogue: DialogueLine[];
  exercises: Exercise[];
  newWords: number;
  reviewWords: number;
  newSentences: number;
  reviewSentences: number;
  alreadyCompleted: boolean;
}

export interface AnswerResult {
  correct: boolean;
  expected: string;
  xpGained: number;
  totalXp: number;
  dayXp: number;
  level: number;
}

export interface Achievement {
  code: string;
  title: string;
  description: string;
  icon: string;
  unlocked: boolean;
  progress: number;
}

export interface DayStat {
  day: number;
  xp: number;
  answered: number;
  correct: number;
  completed: boolean;
}

export interface TopicProgress {
  topic: Topic;
  totalWords: number;
  seenWords: number;
  masteredWords: number;
  totalSentences: number;
  seenSentences: number;
}

export interface Stats {
  day: number;
  totalXp: number;
  dayXp: number;
  dailyGoal: number;
  level: number;
  xpIntoLevel: number;
  xpForNextLevel: number;
  currentStreak: number;
  longestStreak: number;
  wordsSeen: number;
  wordsMastered: number;
  wordsTotal: number;
  sentencesSeen: number;
  sentencesTotal: number;
  dialoguesSeen: number;
  dialoguesTotal: number;
  dueToday: number;
  accuracy: number;
  completedToday: boolean;
  history: DayStat[];
  achievements: Achievement[];
}

export interface Settings {
  sourceLanguage: SourceLanguage;
  dailyGoal: number;
  newWordsPerDay: number;
  reminderHour: number;
  reminderMinute: number;
  remindersEnabled: boolean;
  hasApiKey: boolean;
  /** Empty until the learner loads the catalogue and picks one. */
  aiModel: string;
  ttsModel: string;
  ttsVoice: string;
  ttsSpeed: number;
  ttsPrefetch: boolean;
  tzOffsetMinutes: number;
}

export interface SettingsPatch {
  sourceLanguage?: SourceLanguage;
  dailyGoal?: number;
  newWordsPerDay?: number;
  reminderHour?: number;
  reminderMinute?: number;
  remindersEnabled?: boolean;
  aiModel?: string;
  ttsModel?: string;
  ttsVoice?: string;
  ttsSpeed?: number;
  ttsPrefetch?: boolean;
}

export interface SessionResult {
  firstCompletionToday: boolean;
  newlyUnlocked: Achievement[];
  stats: Stats;
}

export interface ChatMessage {
  role: string;
  content: string;
}

export interface AiModel {
  id: string;
  name: string;
  contextLength: number;
  promptPrice: string;
  /** Non-empty for speech models: the voice names the model accepts. */
  voices: string[];
}

export interface AudioClip {
  mime: string;
  base64: string;
  cached: boolean;
}

export interface AudioCacheStats {
  clips: number;
  bytes: number;
  sentencesCached: number;
  sentencesTotal: number;
}

export interface CacheProgress {
  done: number;
  total: number;
  failed: number;
  finished: boolean;
  error: string | null;
}
