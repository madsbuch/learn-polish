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
  /** "f" or "m" — decides which voice reads the line. */
  gender: string;
  pl: string;
  meaning: string;
}

export interface CataloguePage {
  words: Word[];
  sentences: Sentence[];
  totalWords: number;
  totalSentences: number;
}

export interface StreamEvent {
  id: string;
  delta: string;
  done: boolean;
  error: string | null;
}

export interface Voice {
  name: string;
  gender: string;
  description: string;
}

/** A line queued for synthesis, with the voice gender to read it in. */
export interface SpeechLine {
  text: string;
  gender: string;
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
  ttsVoiceFemale: string;
  ttsVoiceMale: string;
  ttsSpeed: number;
  ttsPrefetch: boolean;
  tzOffsetMinutes: number;
  /** Empty means the songwriter uses the tutor model. */
  songwriterModel: string;
  hasKieKey: boolean;
  sunoModel: string;
  sunoModels: string[];
}

export interface SettingsPatch {
  sourceLanguage?: SourceLanguage;
  dailyGoal?: number;
  newWordsPerDay?: number;
  reminderHour?: number;
  reminderMinute?: number;
  remindersEnabled?: boolean;
  aiModel?: string;
  ttsVoiceFemale?: string;
  ttsVoiceMale?: string;
  ttsSpeed?: number;
  ttsPrefetch?: boolean;
  songwriterModel?: string;
  sunoModel?: string;
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

// ------------------------------------------------------------------ songs

/** One phrase: sung in English, then in Polish (choruses: Polish only). */
export interface SongLine {
  en: string;
  pl: string;
}

export type SongSectionKind = "verse" | "chorus" | "bridge" | "intro" | "outro";

export interface SongSection {
  kind: SongSectionKind;
  lines: SongLine[];
}

export interface SongDraft {
  title: string;
  chorusPl: string;
  chorusEn: string;
  style: string;
  /** "f", "m" or "" for no preference. */
  vocalGender: string;
  sections: SongSection[];
}

export interface SongTrack {
  id: number;
  songId: number;
  ord: number;
  audioPath: string;
  imagePath: string | null;
  duration: number;
  playCount: number;
  /** Lyrics timed to this take, once Suno's word timings are known. */
  lines: TimedLine[] | null;
}

/** One sung line and when it is sung. */
export interface TimedLine {
  start: number;
  end: number;
  text: string;
  /** "pl", "en", or "" for ad-libs that are not in the lyrics. */
  lang: string;
  /** For a Polish line, its English meaning. */
  gloss: string | null;
  /** Set on the first line of a section, e.g. "Chorus". */
  section: string | null;
}

export interface Song extends SongDraft {
  id: number;
  status: "generating" | "ready" | "failed";
  error: string | null;
  createdAt: number;
  tracks: SongTrack[];
}

export interface DraftRequest {
  brief?: string;
  topicId?: number | null;
  chorusPl?: string;
  style?: string;
  vocalGender?: string;
  previous?: SongDraft;
  feedback?: string;
}

// ------------------------------------------------------------------ casting

/** A Sonos room or group. */
export interface CastDevice {
  id: string;
  name: string;
  baseUrl: string;
}

export interface CastItem {
  trackId: number;
  title: string;
  artist: string;
}

export interface CastState {
  device: CastDevice;
  items: CastItem[];
  index: number;
  trackId: number | null;
  playing: boolean;
  transport: string;
  position: number;
  duration: number;
  /** 0–100, or -1 when the speaker did not say. */
  volume: number;
  repeat: string;
  /** The speaker has moved on to something that is not ours. */
  detached: boolean;
}
