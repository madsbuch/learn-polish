// Mirrors the serde structs in src-tauri/src/models.rs.

export interface Course {
  id: number;
  ord: number;
  slug: string;
  /** Always "course": every course is a course, a folder of lessons. */
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
  /** The drill it belongs to. */
  lessonId: number;
  lesson: string;
}

export interface Sentence {
  id: number;
  pl: string;
  meaning: string;
  lessonId: number;
  lesson: string;
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
  | "memory"
  // Picture lessons: meet it (picture, sound, word); hear it and pick its
  // picture; read it and pick its picture; see the picture and pick the
  // word; type the word from the picture.
  | "pictureIntro"
  | "hearPick"
  | "wordPick"
  | "picturePick"
  | "pictureType";

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
  /** The item's picture (a file id), when it has one. */
  image: number | null;
  /** Picture lessons: each option's picture, in the order of `options`. */
  optionImages: (number | null)[];
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
  /** Picture lessons: the card shows this picture instead of the English. */
  image: number | null;
}

export interface SessionPlan {
  day: number;
  course: Course;
  dialogue: DialogueLine[];
  exercises: Exercise[];
  newWords: number;
  reviewWords: number;
  newSentences: number;
  reviewSentences: number;
  alreadyCompleted: boolean;
  /** The drill lesson this session practises. */
  lessonId: number;
  /** Done before and not due: extra practice — half XP, schedule unchanged. */
  extra: boolean;
}

// ------------------------------------------------------------------ lessons

export type Complexity = "low" | "medium" | "high";
export type LessonResult = "pass" | "partial" | "fail";

/** A unit of learning in a course: a drill, or a study, writing or listening lesson. */
export interface Lesson {
  id: number;
  kind: "drill" | "study";
  /** Drills: words | memory | typing | sentences | gaps | dialogue. */
  format: string;
  title: string;
  summary: string;
  complexity: Complexity;
  /** The first course it was listed in, for display. */
  courseId: number | null;
  courseTitle: string | null;
  /** Every course that lists it. */
  courseIds: number[];
  /** Who made it; informational — every lesson can be edited and deleted. */
  origin: "starter" | "tutor" | "you";
  periodDays: number;
  intervalDays: number;
  /** Day number; null until done once. */
  dueDay: number | null;
  lastDay: number | null;
  lastResult: LessonResult | null;
  reps: number;
  /** Writing lessons: 0 guided (word bank, hints), 1 supported (hints), 2 free. */
  supportLevel: number;
  /** Beyond the course's level: needed to master it, not to complete it. */
  challenge: boolean;
  /** Passed at least once. */
  passed: boolean;
  /** Mastery points, 0–100: every pass earns some, spaced passes more. */
  mastery: number;
  items: number;
  /** A listening lesson's song. */
  songId: number | null;
}

export interface LessonDetail extends Lesson {
  content: string;
  objectives: string[];
  words: [string, string][];
  sentences: [string, string][];
  lastFeedback: string | null;
  /** The courses that list it: [id, English title, Polish title]. */
  courses: [number, string, string][];
  /** A conversation drill's dialogue. */
  lines: DialogueLine[];
  /** What the last check against the lesson's Wikipedia sources changed. */
  factNote: string | null;
  /** When the lesson was last checked against its sources (seconds). */
  factCheckedAt: number | null;
  /** Its words with their ids and pictures. */
  wordPictures: WordPicture[];
}

/** A lesson's word, with its picture if it has one. */
export interface WordPicture {
  wordId: number;
  word: string;
  meaning: string;
  image: number | null;
  /** A noun: worth a picture in a word drill. */
  picturable: boolean;
}

export interface LessonOverview {
  today: number;
  /** Today's list, still to do: the most at risk first. */
  due: Lesson[];
  next: Lesson[];
  doneToday: number;
  /** For a big day: the rest of what is due, then what is due soonest. */
  more: Lesson[];
  planTotal: number;
  planDone: number;
  budgetMinutes: number;
  /** Due but not on today's list, in minutes. */
  behindMinutes: number;
  /** Done before their day this past week, in minutes. */
  aheadMinutes: number;
}

export interface Assessment {
  result: LessonResult;
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
  /** Missed, but a streak freeze kept the run going. */
  frozen: boolean;
}

export interface CourseProgress {
  course: Course;
  totalWords: number;
  seenWords: number;
  masteredWords: number;
  totalSentences: number;
  seenSentences: number;
  totalLessons: number;
  /** Lessons done at least once. */
  startedLessons: number;
  /** The course's picture: a file id, loaded with fileUrl(). */
  imageFileId: number | null;
  /** Completed: every core lesson passed once. Mastered: every lesson mastered, challenges too. */
  status: "active" | "completed" | "mastered";
  /** Lessons needed to complete it (not challenges), and how many are passed. */
  coreLessons: number;
  passedCore: number;
  challenges: number;
  /** Lessons, challenges too, already mastered. */
  masteredLessons: number;
  /** Mastery points earned across its lessons, out of masteryMax (100 a lesson). */
  masteryPoints: number;
  masteryMax: number;
  /** The first core lesson, in the course's order, not passed yet. */
  nextLesson: string | null;
  /** A built-in course that never ends (Grammar & culture, Listening, Writing): always active. */
  openEnded: boolean;
  /** When it was closed as mastered (Unix seconds), if it was. */
  closedAt: number | null;
}

/** What the courses closed as mastered add up to, each counted once. */
export interface MasteredSummary {
  courses: number;
  lessons: number;
  words: number;
}

/** A course just completed or mastered. */
export interface Milestone {
  courseId: number;
  title: string;
  mastered: boolean;
}

export interface Stats {
  /** The courses closed as mastered, together. */
  mastered: MasteredSummary;
  day: number;
  totalXp: number;
  dayXp: number;
  dailyGoal: number;
  level: number;
  xpIntoLevel: number;
  xpForNextLevel: number;
  currentStreak: number;
  /** Freezes kept: each covers one missed day (at most two). */
  streakFreezes: number;
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
  /** Plays the other side in writing lessons; empty means the tutor model. */
  partnerModel: string;
  /** "Talk about it" chats; empty means the tutor model. */
  wordChatModel: string;
  /** Models starred in the model picker, shown first. */
  favoriteModels: string[];
  /** The shortest a day's list of check-ins gets, in minutes. */
  reviewFloorMinutes: number;
  /** Draws course pictures; empty means the default image model. */
  imageModel: string;
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
  partnerModel?: string;
  wordChatModel?: string;
  favoriteModels?: string[];
  reviewFloorMinutes?: number;
  imageModel?: string;
  sunoModel?: string;
  songLength?: SongLength;
  theme?: "system" | "light" | "dark";
  playerRepeat?: "off" | "all" | "one";
}

export interface LessonFinished extends SessionResult {
  lesson: Lesson;
  result: LessonResult;
  /** Extra practice: the schedule did not move. */
  extra: boolean;
  /** This drill's score: right answers out of the scored cards. */
  correct: number;
  total: number;
}

export interface SessionResult {
  firstCompletionToday: boolean;
  newlyUnlocked: Achievement[];
  stats: Stats;
  /** Courses this completed or mastered. */
  milestones: Milestone[];
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
    | "settingsChanged"
    | "songStarted"
    | "songDraft"
    | "songsDeleted"
    | "lessonCreated"
    | "lessonUpdated"
    | "lessonDeleted"
    | "lessonScheduled"
    | "lessonSuggested"
    | "memory"
    | "playlistChanged";
  label: string;
  courseId: number | null;
  songId: number | null;
  lessonId: number | null;
}

export interface Conversation {
  id: number;
  title: string;
  /** The last thing said, shortened. */
  preview: string;
  messages: number;
  /** Seconds since the epoch. */
  updatedAt: number;
  /** The tutor is working on a reply in the background. */
  working: boolean;
  /** Why the last reply failed, if it did. */
  error: string | null;
  /** "word" for a chat about a word or sentence ("Talk about it"). */
  tag: string | null;
}

/** `tutor_talk_about`: the conversation about a word, new or carried on. */
export interface TalkAbout {
  conversationId: number;
  existing: boolean;
}

/** `tutor_send` returns at once; the reply follows in the background. */
export interface TutorStarted {
  conversationId: number;
  message: ChatMessage;
}

/** `tutor://done`: a background reply finished, or failed. */
export interface TutorDone {
  conversationId: number;
  message: ChatMessage | null;
  error: string | null;
}

export interface ChatMessage {
  id: number;
  conversationId: number;
  role: "user" | "assistant";
  content: string;
  actions: TutorAction[];
}

/** What a tool is doing while an agent runs (`agent://status`). */
export interface AgentStatus {
  id: string;
  label: string;
}

export interface AiModel {
  id: string;
  name: string;
  contextLength: number;
  /** Dollars per input token, e.g. "0.0000003". */
  promptPrice: string;
  /** Dollars per output token. */
  completionPrice: string;
  /** Dollars per input token read from the provider's cache; "" when not reported. */
  cacheReadPrice: string;
  /** When it was added to OpenRouter (seconds since the epoch). */
  created: number;
  /** Non-empty for speech models: the voice names the model accepts. */
  voices: string[];
  /** Can call tools — the tutor needs this to act in the app. */
  tools: boolean;
  /** Can make images — for course pictures. */
  images: boolean;
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

/** How much of one course can be played without the network. */
export interface CourseAudio {
  courseId: number;
  title: string;
  titlePl: string;
  total: number;
  stored: number;
  bytes: number;
}

export interface AudioOverview {
  clips: number;
  bytes: number;
  courses: CourseAudio[];
  /** Clips no course uses with the current voices. */
  unusedClips: number;
  unusedBytes: number;
}

export interface AudioItem {
  kind: "word" | "sentence" | "line";
  text: string;
  gender: string;
  stored: boolean;
  bytes: number;
}

/** A Wikipedia article a study lesson or a podcast is built on. */
export interface WikiSource {
  id: number;
  lessonId: number | null;
  songId: number | null;
  lang: string;
  title: string;
  url: string;
  summary: string;
  imageFileId: number | null;
  /** Polish Wikipedia has it too. */
  polishTitle: string | null;
  checkedAt: number;
}

/** A source's article, for the reader. */
export interface ArticleView {
  lang: string;
  title: string;
  url: string;
  /** Markdown: shown when the layout is not known. */
  content: string;
  /** The article as Wikipedia lays it out (HTML), drawn by WikiArticle. */
  html: string | null;
}

/** An article found on Wikipedia. */
export interface WikiHit {
  title: string;
  snippet: string;
}

/** The outcome of checking a lesson against its sources. */
export interface FactCheck {
  lessonId: number;
  title: string;
  updated: boolean;
  note: string | null;
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
  /** In a podcast with a cast: who says it (absent or "": the narrator). */
  speaker?: string;
  /** In a podcast: how it is said ("surprised"). */
  how?: string;
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
  /** The course it is filed in (through its listening lesson). */
  courseId?: number | null;
  /** How it came about — kept with the song; pass it back unchanged. */
  origin?: SongOrigin;
  /** "podcast" for a spoken episode; "song" (or absent) for a song. */
  kind?: "song" | "podcast" | "";
  /** For a podcast: how its audio is put together. */
  podcast?: PodcastAudio | null;
}

/** Everything that went into making a song, besides its lyrics and style. */
export interface SongOrigin {
  brief: string;
  requestedStyle: string;
  requestedVocalGender: string;
  requestedChorus: string;
  length: string;
  songwriterModel: string;
  revisions: string[];
  /** What a podcast was written from: its Wikipedia article (and the Polish one). */
  sources?: PodcastSource[];
  sourceImage?: string;
  requestedArticles?: string[];
  story?: boolean;
  requestedCast?: string;
  /** Made from today's news, and the stories it told. */
  news?: boolean;
  newsItems?: NewsItem[];
  /** "composer" or "tutor". */
  madeBy?: string;
  /** What the lyrics or script had to meet (the tutor's). */
  requirements?: string;
  /** The song or podcast this one varies. */
  basedOn?: number | null;
  /** Everything the writer was given besides the request. */
  given?: {
    words: string[];
    sentences: string[];
    weakWords: string[];
    existingChoruses: string[];
    memories: string[];
    reading: string;
  };
  instructionsVersion?: string;
  /** Revisions and edits after the first draft, each with the draft before it. */
  steps?: { kind: string; by: string; text: string; model: string; before: SongDraft | null }[];
}

/** A news story a podcast told, as the news index had it. */
export interface NewsItem {
  title: string;
  url: string;
  /** The outlet's address, e.g. "pap.pl". */
  outlet: string;
  /** "2026-11-16". */
  date: string;
  image: string;
  summary: string;
}

/** An article a podcast was written from. */
export interface PodcastSource {
  title: string;
  url: string;
  /** "en" or "pl". */
  lang: string;
}

/** How a podcast episode's audio is put together. */
export interface PodcastAudio {
  /** The English is said before each Polish sentence; off: only Polish is heard. */
  english: boolean;
  /** Seconds of silence after the English, before the Polish. */
  pause: number;
  /** How many times the Polish is said. */
  repeats: number;
  /** The Polish read slowly. */
  slow: boolean;
  /** "two" voices (English and Polish apart) or "one". */
  voices: "one" | "two";
  /** Who speaks besides the narrator, each in a voice of their own. */
  cast?: CastMember[];
}

/** A host or a character in a podcast. */
export interface CastMember {
  name: string;
  /** "f" or "m". */
  gender: string;
  /** How they come across, in a word ("curious"). */
  character: string;
  /** The voice they speak in; chosen when the episode is recorded. */
  voice: string;
}

/** Who talks in a podcast. */
export type PodcastCast = "narrator" | "hosts" | "characters";

/** "pairs": English then Polish; "immersion": only Polish, in simpler sentences. */
export type PodcastFormat = "pairs" | "immersion";

export interface PodcastRequest {
  topic?: string;
  /** English Wikipedia article titles to write from (0–5). */
  articles?: string[];
  /** A short made-up story, from no articles. */
  story?: boolean;
  /** Who talks; absent: hosts for articles, characters for a story. */
  cast?: PodcastCast;
  /** Today's news from Polish outlets (about `topic`, if given). */
  news?: boolean;
  format?: PodcastFormat;
  length?: SongLength;
  vocalGender?: string;
  previous?: SongDraft;
  feedback?: string;
}

/** A subject the learner might like an episode about. */
export interface TopicIdea {
  topic: string;
  /** None: a story. */
  articles: string[];
  why: string;
}

export interface SongTrack {
  id: number;
  songId: number;
  ord: number;
  /** The take's audio and cover art: file ids, loaded with fileUrl(). */
  audioFileId: number;
  imageFileId: number | null;
  /** Copies on disk, only for Android's native player. */
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
  /** In a podcast with a cast: who says it. */
  speaker?: string | null;
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

/** A phrase with its meaning. */
export interface Phrase {
  pl: string;
  en: string;
}

/** A writing goal's help: a way to start, and words to use. */
export interface GoalHelp {
  goal: string;
  starter: Phrase;
  words: Phrase[];
}

/** Tokens one feature used lately: what a month with another model is estimated from. */
export interface FeatureUsage {
  feature: string;
  requests: number;
  tokensIn: number;
  tokensCached: number;
  tokensOut: number;
  /** Days the counts cover. */
  days: number;
}

/** One benchmark item's outcome. */
export interface BenchItem {
  part: "polish" | "tools" | "rules";
  name: string;
  /** 0–1. */
  score: number;
  note: string;
  answer: string;
}

/** A benchmark run on one model. */
export interface BenchRun {
  id: number;
  model: string;
  createdAt: number;
  score: number;
  polish: number;
  tools: number;
  rules: number;
  /** Average seconds per reply. */
  seconds: number;
  costUsd: number | null;
  items: BenchItem[];
}

/** A sentence that uses a word, shown when the word is answered wrong. */
export interface WordExample {
  pl: string;
  en: string;
}
