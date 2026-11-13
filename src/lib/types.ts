// Mirrors the serde structs in src-tauri/src/models.rs.

export interface Topic {
  id: number;
  ord: number;
  slug: string;
  /** Always "course": every topic is a course, a folder of blocks. */
  kind: string;
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
  | "dialogueGap"
  | "gapFill"
  | "memory";

export interface Exercise {
  key: string;
  kind: ExerciseKind;
  itemKind: "word" | "sentence" | "dialogue" | "set";
  itemId: number;
  isNew: boolean;
  prompt: string;
  hint: string;
  answer: string;
  options: string[];
  tokens: string[];
  pl: string;
  meaning: string;
  /** Fill-the-gaps: sentences with one word missing; the bank is `options`. */
  gaps: Gap[];
  /** Memory cards: the Polish/English pairs to find. */
  pairs: Pair[];
}

export interface Gap {
  before: string;
  answer: string;
  after: string;
  meaning: string;
}

export interface Pair {
  pl: string;
  en: string;
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
  /** The drill block this session practises. */
  blockId: number;
}

// ------------------------------------------------------------------ blocks

export type Complexity = "low" | "medium" | "high";
export type BlockResult = "pass" | "partial" | "fail";

/** A unit of learning: a small drill, or a study lesson checked by an examiner. */
export interface Block {
  id: number;
  kind: "drill" | "study";
  /** Drills: words | memory | typing | sentences | gaps | dialogue. */
  format: string;
  title: string;
  summary: string;
  complexity: Complexity;
  /** The first course it was listed in, for display. */
  topicId: number | null;
  topicTitle: string | null;
  /** Every course that lists it. */
  courseIds: number[];
  /** Who made it; informational — every block can be edited and deleted. */
  origin: "starter" | "tutor" | "you";
  periodDays: number;
  intervalDays: number;
  /** Day number; null until done once. */
  dueDay: number | null;
  lastDay: number | null;
  lastResult: BlockResult | null;
  reps: number;
  items: number;
  /** A listening block's song. */
  songId: number | null;
}

export interface BlockDetail extends Block {
  lesson: string;
  objectives: string[];
  words: [string, string][];
  sentences: [string, string][];
  lastFeedback: string | null;
  /** The courses that list it: [id, English title, Polish title]. */
  courses: [number, string, string][];
}

export interface BlockOverview {
  today: number;
  due: Block[];
  next: Block[];
  doneToday: number;
}

export interface Assessment {
  result: BlockResult;
  summary: string;
  strengths: string[];
  toReview: string[];
  nextDueDay: number;
  xp: number;
}

export interface CheckinTurn {
  checkinId: number;
  reply: string;
  assessment: Assessment | null;
  firstCompletionToday: boolean;
  /** What the examiner saved to or changed in memory this turn. */
  remembered: string[];
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
  totalBlocks: number;
  /** Blocks done at least once. */
  startedBlocks: number;
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
  dailyGoal: number;
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
  /** What the songwriter writes for: about 2, 3 or 4–5 minutes. */
  songLength: SongLength;
  /** Empty until first stored. */
  theme: "" | "system" | "light" | "dark";
  playerRepeat: "" | "off" | "all" | "one";
}

export interface SettingsPatch {
  dailyGoal?: number;
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
  songLength?: SongLength;
  theme?: "system" | "light" | "dark";
  playerRepeat?: "off" | "all" | "one";
}

export interface BlockFinished extends SessionResult {
  block: Block;
  result: BlockResult;
}

export interface SessionResult {
  firstCompletionToday: boolean;
  newlyUnlocked: Achievement[];
  stats: Stats;
}

export interface TutorAction {
  kind:
    | "courseCreated"
    | "courseUpdated"
    | "courseDeleted"
    // Older conversations, from before programmes became courses.
    | "programmeCreated"
    | "programmeUpdated"
    | "programmeDeleted"
    | "focusSet"
    | "focusCleared"
    | "settingsChanged"
    | "practiceSuggested"
    | "songStarted"
    | "songsDeleted"
    | "blockCreated"
    | "blockUpdated"
    | "blockDeleted"
    | "blockScheduled"
    | "blockSuggested"
    | "memory";
  label: string;
  topicId: number | null;
  songId: number | null;
  blockId: number | null;
}

export interface Conversation {
  id: number;
  title: string;
  /** The last thing said, shortened. */
  preview: string;
  messages: number;
  /** Seconds since the epoch. */
  updatedAt: number;
}

export interface ChatMessage {
  id: number;
  conversationId: number;
  role: "user" | "assistant";
  content: string;
  actions: TutorAction[];
}

export interface TutorStatus {
  id: string;
  label: string;
}

export interface AiModel {
  id: string;
  name: string;
  contextLength: number;
  promptPrice: string;
  /** Non-empty for speech models: the voice names the model accepts. */
  voices: string[];
  /** Can call tools — the tutor needs this to act in the app. */
  tools: boolean;
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

export interface PlaylistEntry {
  /** The entry itself; the same take may be in a playlist twice. */
  id: number;
  trackId: number;
}

export interface Playlist {
  id: number;
  name: string;
  entries: PlaylistEntry[];
}

export type SongLength = "short" | "medium" | "long";

export interface DraftRequest {
  length?: SongLength;
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

/** What one AI feature cost, over a day or a window. */
export interface FeatureCost {
  feature: string;
  costUsd: number;
  calls: number;
  /** Requests whose price the provider did not report (speech). */
  unpriced: number;
}

export interface DayCost {
  day: number;
  costUsd: number;
  calls: number;
  features: FeatureCost[];
}

export interface CostSummary {
  today: number;
  days: number;
  todayUsd: number;
  totalUsd: number;
  allTimeUsd: number;
  features: FeatureCost[];
  /** Days with any AI use, newest first. */
  perDay: DayCost[];
}

export type MemoryCategory = "goal" | "preference" | "context" | "strength" | "struggle";

/** One fact the agents remember about the learner. */
export interface Memory {
  id: number;
  content: string;
  category: MemoryCategory;
  /** "tutor", "examiner" or "you". */
  source: string;
  createdAt: number;
  updatedAt: number;
}

export interface MemoryOverview {
  enabled: boolean;
  memories: Memory[];
}
