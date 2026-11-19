// The shapes the Rust core sends (serde structs in src-tauri/src), as zod
// schemas: every command result and event payload is parsed with one
// (src/lib/api.ts, src/lib/events.ts), so a shape that drifts fails loudly
// at the boundary instead of deep in a screen. Each type is inferred from
// its schema.

import { z } from "zod";

/**
 * A tagged id (DATA.md): its kind's prefix, an underscore and 26 base32
 * characters — `ls_01jd6x7k2m8q9r4t5v6w7x8y9z`. Each kind is its own branded
 * type, so a course id where a lesson id belongs is a type error.
 */
function typeId<B extends string>(prefix: string) {
  return z
    .string()
    .regex(
      new RegExp(`^${prefix}_[0-7][0-9a-hjkmnp-tv-z]{25}$`),
      `not a ${prefix}_ id`,
    )
    .brand<B>();
}

export const CourseIdSchema = typeId<"CourseId">("crs");
export type CourseId = z.infer<typeof CourseIdSchema>;
export const LessonIdSchema = typeId<"LessonId">("ls");
export type LessonId = z.infer<typeof LessonIdSchema>;
export const WordIdSchema = typeId<"WordId">("wd");
export type WordId = z.infer<typeof WordIdSchema>;
export const SentenceIdSchema = typeId<"SentenceId">("sn");
export type SentenceId = z.infer<typeof SentenceIdSchema>;
export const LineIdSchema = typeId<"LineId">("dl");
export type LineId = z.infer<typeof LineIdSchema>;
export const SongIdSchema = typeId<"SongId">("sg");
export type SongId = z.infer<typeof SongIdSchema>;
export const TrackIdSchema = typeId<"TrackId">("tk");
export type TrackId = z.infer<typeof TrackIdSchema>;
export const PlaylistIdSchema = typeId<"PlaylistId">("pl");
export type PlaylistId = z.infer<typeof PlaylistIdSchema>;
export const EntryIdSchema = typeId<"EntryId">("pe");
export type EntryId = z.infer<typeof EntryIdSchema>;
export const FileIdSchema = typeId<"FileId">("fl");
export type FileId = z.infer<typeof FileIdSchema>;
export const CheckinIdSchema = typeId<"CheckinId">("ci");
export type CheckinId = z.infer<typeof CheckinIdSchema>;
export const CheckinMessageIdSchema = typeId<"CheckinMessageId">("cm");
export type CheckinMessageId = z.infer<typeof CheckinMessageIdSchema>;
export const ConversationIdSchema = typeId<"ConversationId">("cv");
export type ConversationId = z.infer<typeof ConversationIdSchema>;
export const MessageIdSchema = typeId<"MessageId">("msg");
export type MessageId = z.infer<typeof MessageIdSchema>;
export const MemoryIdSchema = typeId<"MemoryId">("mem");
export type MemoryId = z.infer<typeof MemoryIdSchema>;
export const SourceIdSchema = typeId<"SourceId">("src");
export type SourceId = z.infer<typeof SourceIdSchema>;
export const XpEntryIdSchema = typeId<"XpEntryId">("xp");
export type XpEntryId = z.infer<typeof XpEntryIdSchema>;
export const CostIdSchema = typeId<"CostId">("cost");
export type CostId = z.infer<typeof CostIdSchema>;
export const BenchRunIdSchema = typeId<"BenchRunId">("bench");
export type BenchRunId = z.infer<typeof BenchRunIdSchema>;
export const DeviceIdSchema = typeId<"DeviceId">("dev");
export type DeviceId = z.infer<typeof DeviceIdSchema>;
/** A built-in sound pair (sounds.rs): derived from its words, not a row. */
export const SoundPairIdSchema = typeId<"SoundPairId">("sp");
export type SoundPairId = z.infer<typeof SoundPairIdSchema>;
/** One panel of a story lesson. */
export const PanelIdSchema = typeId<"PanelId">("pnl");
export type PanelId = z.infer<typeof PanelIdSchema>;
/** One graded answer in the learning log. */
export const AttemptIdSchema = typeId<"AttemptId">("att");
export type AttemptId = z.infer<typeof AttemptIdSchema>;
/** A grammar structure from the built-in list (grammar.rs): derived, not a row. */
export const GrammarTagIdSchema = typeId<"GrammarTagId">("gr");
export type GrammarTagId = z.infer<typeof GrammarTagIdSchema>;

// Mirrors the serde structs in src-tauri/src/models.rs.
export const CourseSchema = z.object({
  id: CourseIdSchema,
  ord: z.number(),
  slug: z.string(),
  /** Always "course": every course is a course, a folder of lessons. */
  kind: z.string(),
  titlePl: z.string(),
  title: z.string(),
  summary: z.string(),
  /** How advanced it is, A1–C2; null until it is levelled. */
  level: z.string().nullable(),
});
export type Course = z.infer<typeof CourseSchema>;

export const WordSchema = z.object({
  id: WordIdSchema,
  pl: z.string(),
  meaning: z.string(),
  pos: z.string(),
  /** The drill it belongs to. */
  lessonId: LessonIdSchema,
  lesson: z.string(),
});
export type Word = z.infer<typeof WordSchema>;

export const SentenceSchema = z.object({
  id: SentenceIdSchema,
  pl: z.string(),
  meaning: z.string(),
  lessonId: LessonIdSchema,
  lesson: z.string(),
});
export type Sentence = z.infer<typeof SentenceSchema>;

export const DialogueLineSchema = z.object({
  ord: z.number(),
  speaker: z.string(),
  /** "f" or "m" — decides which voice reads the line. */
  gender: z.string(),
  pl: z.string(),
  meaning: z.string(),
});
export type DialogueLine = z.infer<typeof DialogueLineSchema>;

export const CataloguePageSchema = z.object({
  words: z.array(WordSchema),
  sentences: z.array(SentenceSchema),
  totalWords: z.number(),
  totalSentences: z.number(),
});
export type CataloguePage = z.infer<typeof CataloguePageSchema>;

export const StreamEventSchema = z.object({
  id: z.string(),
  delta: z.string(),
  done: z.boolean(),
  error: z.string().nullable(),
});
export type StreamEvent = z.infer<typeof StreamEventSchema>;

export const VoiceSchema = z.object({
  name: z.string(),
  gender: z.string(),
  description: z.string(),
});
export type Voice = z.infer<typeof VoiceSchema>;

/** A line queued for synthesis, with the voice gender to read it in. */
export const SpeechLineSchema = z.object({
  text: z.string(),
  gender: z.string(),
});
export type SpeechLine = z.infer<typeof SpeechLineSchema>;

export const ExerciseKindSchema = z.enum([
  "dialogueRead",
  "intro",
  "recognise",
  "produce",
  "type",
  "sentenceIntro",
  "wordOrder",
  "dialogueGap",
  "gapFill",
  "memory",
  "pictureIntro",
  "wordPick",
  "picturePick",
  "dictation",
  "soundPair",
  "sceneExplore",
  "sceneFind",
  "sceneName",
  "sceneType",
  "sceneWhere",
  "storyPanel",
  "storyQuestion",
  "storyEnd",
  "sayIt",
  "dialogueSay",
]);
export type ExerciseKind = z.infer<typeof ExerciseKindSchema>;

export const GapSchema = z.object({
  before: z.string(),
  answer: z.string(),
  after: z.string(),
  meaning: z.string(),
});
export type Gap = z.infer<typeof GapSchema>;

export const PairSchema = z.object({
  pl: z.string(),
  en: z.string(),
  /** Picture lessons: the card shows this picture instead of the English. */
  image: FileIdSchema.nullable(),
});
export type Pair = z.infer<typeof PairSchema>;

/** A word-order tile whose ending the learner picks ("kawa" → "kawę"). */
export const TileEndingSchema = z.object({
  /** Which tile: its index in the card's `tokens`. */
  token: z.number(),
  /** What it shows until an ending is picked: the dictionary form. */
  shown: z.string(),
  /** The forms to pick from, the right one among them. */
  choices: z.array(z.string()),
});
export type TileEnding = z.infer<typeof TileEndingSchema>;

/** One thing in a scene lesson's picture, and where it is. */
export const SceneSpotSchema = z.object({
  wordId: WordIdSchema,
  pl: z.string(),
  en: z.string(),
  /** Its box, in fractions of the picture's width and height (0–1, from the top left). */
  x: z.number(),
  y: z.number(),
  w: z.number(),
  h: z.number(),
  /** The card points it out: the thing to name, or the two a sentence is about. */
  lit: z.boolean(),
});
export type SceneSpot = z.infer<typeof SceneSpotSchema>;

/** A scene as a card shows it: the picture, its size in pixels, its things. */
export const SceneCardSchema = z.object({
  image: FileIdSchema,
  width: z.number(),
  height: z.number(),
  spots: z.array(SceneSpotSchema),
});
export type SceneCard = z.infer<typeof SceneCardSchema>;

/** What a sung word means (singalong.rs), looked up by its sung form. */
export const GlossWordSchema = z.object({
  /** As sung: lowercase, no punctuation. */
  form: z.string(),
  /** Its dictionary form. */
  pl: z.string(),
  en: z.string(),
  wordId: WordIdSchema.nullable(),
  /** One of the words of the song's course: Sing along asks these first. */
  lesson: z.boolean(),
});
export type GlossWord = z.infer<typeof GlossWordSchema>;

/** A sentence under a story's panel, with the voice to read it in. */
export const StoryLineSchema = z.object({
  pl: z.string(),
  en: z.string(),
  /** Who says it; empty for narration. */
  speaker: z.string(),
  /** "f" or "m" for a character's line; empty for the narrator. */
  gender: z.string(),
});
export type StoryLine = z.infer<typeof StoryLineSchema>;

/** One panel of a story: its picture (once drawn) and its sentences. */
export const StoryPanelSchema = z.object({
  id: PanelIdSchema,
  /** Its place in the story, from 1. */
  number: z.number(),
  /** What the picture shows, in English: for screen readers, and while it is drawn. */
  picture: z.string(),
  image: FileIdSchema.nullable(),
  width: z.number(),
  height: z.number(),
  status: z.enum(["waiting", "drawing", "ready", "failed"]),
  /** Why its picture failed, when it did. */
  error: z.string(),
  lines: z.array(StoryLineSchema),
});
export type StoryPanel = z.infer<typeof StoryPanelSchema>;

/** A word of a story's lesson, for its last page. */
export const StoryWordSchema = z.object({
  wordId: WordIdSchema,
  pl: z.string(),
  en: z.string(),
  /** Met for the first time in this story. */
  new: z.boolean(),
});
export type StoryWord = z.infer<typeof StoryWordSchema>;

/** What a story card shows: a panel to read, what a question is about, or the words at the end. */
export const StoryCardSchema = z.object({
  title: z.string(),
  panel: StoryPanelSchema.nullable(),
  /** Panels in the story. */
  total: z.number(),
  /** A question's kind; empty on a panel or the end. */
  ask: z.enum(["", "choice", "picture", "order"]),
  /** The meanings of the panel's words. */
  glossary: z.array(GlossWordSchema),
  /** The end: the lesson's words, met in the story. */
  words: z.array(StoryWordSchema),
});
export type StoryCard = z.infer<typeof StoryCardSchema>;

export const ExerciseSchema = z.object({
  key: z.string(),
  kind: ExerciseKindSchema,
  itemKind: z.enum(["word", "sentence", "dialogue", "set"]),
  itemId: z.string(),
  isNew: z.boolean(),
  prompt: z.string(),
  hint: z.string(),
  answer: z.string(),
  options: z.array(z.string()),
  tokens: z.array(z.string()),
  pl: z.string(),
  meaning: z.string(),
  /** Fill-the-gaps: sentences with one word missing; the bank is `options`. */
  gaps: z.array(GapSchema),
  /** Memory cards: the Polish/English pairs to find. */
  pairs: z.array(PairSchema),
  /** The item's picture (a file id), when it has one. */
  image: FileIdSchema.nullable(),
  /** Picture lessons: each option's picture, in the order of `options`. */
  optionImages: z.array(FileIdSchema.nullable()),
  /** Dialogue gaps: the conversation just before the gap, oldest first. */
  context: z.array(DialogueLineSchema),
  /** Word order: the tiles whose ending is to be picked; empty for plain ordering. */
  endings: z.array(TileEndingSchema),
  /** Scene lessons: the picture and where each thing is in it. */
  scene: SceneCardSchema.nullable(),
  /** Story lessons: the panel, the question's kind, or the words at the end. */
  story: StoryCardSchema.nullable(),
  /** A spoken dialogue turn: the other speaker's answer, said once the line is. */
  reply: DialogueLineSchema.nullable(),
  /** Why the answer is what it is (empty when there is nothing to explain). */
  explanation: z.string(),
});
export type Exercise = z.infer<typeof ExerciseSchema>;

/** Quick games on Home: enough words to play, the records, today's XP from them. */
export const QuickGamesSchema = z.object({
  wordsMet: z.number(),
  minWords: z.number(),
  /** Dictation needs a few sentences met. */
  sentencesMet: z.number(),
  minSentences: z.number(),
  matchBestMs: z.number().nullable(),
  flipBestMoves: z.number().nullable(),
  flipBestMs: z.number().nullable(),
  xpToday: z.number(),
  xpPerDay: z.number(),
  xpPerGame: z.number(),
});
export type QuickGames = z.infer<typeof QuickGamesSchema>;

export const QuickGameKindSchema = z.enum(["match", "flip", "dictation", "sounds", "say"]);
export type QuickGameKind = z.infer<typeof QuickGameKindSchema>;

/** A sentence to write down in the Dictation game. */
export const DictationItemSchema = z.object({
  id: SentenceIdSchema,
  pl: z.string(),
  meaning: z.string(),
});
export type DictationItem = z.infer<typeof DictationItemSchema>;

/** A pair of words that differ by one sound English ears mix up. */
export const SoundPairSchema = z.object({
  /** A built-in pair's id; null for a word's own neighbour. */
  id: SoundPairIdSchema.nullable(),
  /** What to listen for: "sz or ś". */
  label: z.string(),
  a: z.string(),
  aEn: z.string(),
  b: z.string(),
  bEn: z.string(),
});
export type SoundPair = z.infer<typeof SoundPairSchema>;

/** A round of the Sound pairs game: the pair, and which of it is said. */
export const SoundRoundSchema = SoundPairSchema.extend({ said: z.string() });
export type SoundRound = z.infer<typeof SoundRoundSchema>;

/** One game's cards, sentences or rounds. */
export const QuickGameSchema = z.object({
  game: QuickGameKindSchema,
  pairs: z.array(PairSchema),
  sentences: z.array(DictationItemSchema),
  sounds: z.array(SoundRoundSchema),
});
export type QuickGame = z.infer<typeof QuickGameSchema>;

/** What finishing a quick game brought. */
export const GameFinishedSchema = z.object({
  xpGained: z.number(),
  /** Beat the best before it (a first game only sets one). */
  record: z.boolean(),
  bestMs: z.number().nullable(),
  bestMoves: z.number().nullable(),
  previousMs: z.number().nullable(),
  previousMoves: z.number().nullable(),
  xpToday: z.number(),
  xpPerDay: z.number(),
  totalXp: z.number(),
  level: z.number(),
});
export type GameFinished = z.infer<typeof GameFinishedSchema>;

export const SessionPlanSchema = z.object({
  day: z.number(),
  course: CourseSchema,
  dialogue: z.array(DialogueLineSchema),
  exercises: z.array(ExerciseSchema),
  newWords: z.number(),
  reviewWords: z.number(),
  newSentences: z.number(),
  reviewSentences: z.number(),
  alreadyCompleted: z.boolean(),
  /** The drill lesson this session practises. */
  lessonId: LessonIdSchema,
  /** Done before and not due: extra practice — half XP, schedule unchanged. */
  extra: z.boolean(),
});
export type SessionPlan = z.infer<typeof SessionPlanSchema>;

// ------------------------------------------------------------------ lessons
export const ComplexitySchema = z.enum(["low", "medium", "high"]);
export type Complexity = z.infer<typeof ComplexitySchema>;

export const LessonResultSchema = z.enum(["pass", "partial", "fail"]);
export type LessonResult = z.infer<typeof LessonResultSchema>;

/** A unit of learning in a course: a drill, or a study, writing or listening lesson. */
export const LessonSchema = z.object({
  id: LessonIdSchema,
  /** Its type: one of the registered lesson types (see lib/lessonTypes.ts). */
  type: z.string(),
  /** How it is practised, from its type. */
  engine: z.enum(["exercises", "conversation"]),
  title: z.string(),
  summary: z.string(),
  complexity: ComplexitySchema,
  /** The first course it was listed in, for display. */
  courseId: CourseIdSchema.nullable(),
  courseTitle: z.string().nullable(),
  /** Every course that lists it. */
  courseIds: z.array(CourseIdSchema),
  /** Who made it; informational — every lesson can be edited and deleted. */
  origin: z.enum(["starter", "tutor", "you"]),
  periodDays: z.number(),
  intervalDays: z.number(),
  /** Day number; null until done once. */
  dueDay: z.number().nullable(),
  lastDay: z.number().nullable(),
  lastResult: LessonResultSchema.nullable(),
  reps: z.number(),
  /** Writing lessons: 0 guided (word bank, hints), 1 supported (hints), 2 free. */
  supportLevel: z.number(),
  /** Mastered, for good (lessons.rs `stamp_mastered`). */
  mastered: z.boolean(),
  /** Beyond the course's level: needed to master it, not to complete it. */
  challenge: z.boolean(),
  /** Passed at least once. */
  passed: z.boolean(),
  /** Mastery points, 0–100: every pass earns some, spaced passes more. */
  mastery: z.number(),
  /** Its own CEFR level, when it differs from its course's. */
  level: z.string().nullable(),
  items: z.number(),
  /** A listening lesson's song. */
  songId: SongIdSchema.nullable(),
});
export type Lesson = z.infer<typeof LessonSchema>;

/** A lesson's word, with its picture if it has one. */
export const WordPictureSchema = z.object({
  wordId: WordIdSchema,
  word: z.string(),
  meaning: z.string(),
  image: FileIdSchema.nullable(),
});
export type WordPicture = z.infer<typeof WordPictureSchema>;

/** A scene lesson's picture as its page shows it, and how far along it is. */
export const SceneInfoSchema = z.object({
  status: z.enum(["drawing", "locating", "ready", "failed"]),
  /** Why it failed, when it did. */
  error: z.string(),
  /** Being made on this device right now. */
  working: z.boolean(),
  description: z.string(),
  card: SceneCardSchema.nullable(),
  /** The "Where is it?" sentences the picture bears out: [Polish, English]. */
  statements: z.array(z.tuple([z.string(), z.string()])),
});
export type SceneInfo = z.infer<typeof SceneInfoSchema>;

/** Someone in a story, and how they look in every picture. */
export const CharacterSchema = z.object({
  name: z.string(),
  look: z.string(),
  gender: z.string(),
});
export type Character = z.infer<typeof CharacterSchema>;

/** A story lesson's story as its page shows it: the picture book. */
export const StoryInfoSchema = z.object({
  theme: z.string(),
  characters: z.array(CharacterSchema),
  panels: z.array(StoryPanelSchema),
  /** The meanings of every word in it. */
  glossary: z.array(GlossWordSchema),
  /** Its pictures are being drawn on this device right now. */
  working: z.boolean(),
});
export type StoryInfo = z.infer<typeof StoryInfoSchema>;

export const LessonDetailSchema = LessonSchema.extend({
  content: z.string(),
  objectives: z.array(z.string()),
  words: z.array(z.tuple([z.string(), z.string()])),
  sentences: z.array(z.tuple([z.string(), z.string()])),
  lastFeedback: z.string().nullable(),
  /** The courses that list it: [id, English title, Polish title]. */
  courses: z.array(z.tuple([CourseIdSchema, z.string(), z.string()])),
  /** A conversation drill's dialogue. */
  lines: z.array(DialogueLineSchema),
  /** What the last check against the lesson's Wikipedia sources changed. */
  factNote: z.string().nullable(),
  /** When the lesson was last checked against its sources (seconds). */
  factCheckedAt: z.number().nullable(),
  /** Its words with their ids and pictures. */
  wordPictures: z.array(WordPictureSchema),
  /** A scene lesson's scene; null for every other lesson. */
  scene: SceneInfoSchema.nullable(),
  /** A story lesson's story; null for every other lesson. */
  story: StoryInfoSchema.nullable(),
});
export type LessonDetail = z.infer<typeof LessonDetailSchema>;

export const XpEntrySchema = z.object({
  at: z.number(),
  label: z.string(),
  xp: z.number(),
  /** Why it earned what it did, when that needs saying. */
  note: z.string(),
});
export type XpEntry = z.infer<typeof XpEntrySchema>;

/** One go at a lesson, or one check-in. */
export const XpRunSchema = z.object({
  lessonId: LessonIdSchema.nullable(),
  lessonTitle: z.string(),
  courseTitle: z.string().nullable(),
  /** Unix seconds. */
  startedAt: z.number(),
  xp: z.number(),
  entries: z.array(XpEntrySchema),
});
export type XpRun = z.infer<typeof XpRunSchema>;

/** What earned a day's XP (xplog.rs). */
export const XpDaySchema = z.object({
  day: z.number(),
  /** The day's total: kept for every day. */
  total: z.number(),
  /** Whether the day has its details (days from before the log do not). */
  detailed: z.boolean(),
  runs: z.array(XpRunSchema),
});
export type XpDay = z.infer<typeof XpDaySchema>;

/** A day's total, for the history chart. */
export const XpHistoryDaySchema = z.object({
  day: z.number(),
  xp: z.number(),
  completed: z.boolean(),
  frozen: z.boolean(),
});
export type XpHistoryDay = z.infer<typeof XpHistoryDaySchema>;

/** A note in Settings → Diagnostics. */
/** What a TypeID names (ids_resolve): its kind and title; no title once deleted. */
export const ResolvedIdSchema = z.object({
  id: z.string(),
  kind: z.string(),
  title: z.string().nullable(),
});
export type ResolvedId = z.infer<typeof ResolvedIdSchema>;

export const DiagnosticEntrySchema = z.object({
  /** Unix seconds. */
  at: z.number(),
  /** "timing" | "model reply" | "background" | "database" */
  kind: z.string(),
  message: z.string(),
});
export type DiagnosticEntry = z.infer<typeof DiagnosticEntrySchema>;

/** A lesson type, as the core registers it. */
export const LessonTypeInfoSchema = z.object({
  id: z.string(),
  name: z.string(),
  /** drill | study | write | song | picture */
  icon: z.string(),
  description: z.string(),
  engine: z.enum(["exercises", "conversation"]),
  /** Mastering it also takes doing it once without help (writing). */
  unaidedToMaster: z.boolean(),
  /** What its page shows, in order. */
  sections: z.array(z.string()),
  /** Its page's button, the first time and after. */
  startLabel: z.string(),
  againLabel: z.string(),
  /** Minutes it takes, by complexity [low, medium, high] (lesson_types::minutes). */
  minutes: z.tuple([z.number(), z.number(), z.number()]),
});
export type LessonTypeInfo = z.infer<typeof LessonTypeInfoSchema>;

export const LessonOverviewSchema = z.object({
  today: z.number(),
  /** Today's lessons still to do, most important first. */
  due: z.array(LessonSchema),
  doneToday: z.number(),
  /** Today's lessons: how many, and how many done. */
  planTotal: z.number(),
  planDone: z.number(),
});
export type LessonOverview = z.infer<typeof LessonOverviewSchema>;

export const AssessmentSchema = z.object({
  result: LessonResultSchema,
  summary: z.string(),
  strengths: z.array(z.string()),
  toReview: z.array(z.string()),
  nextDueDay: z.number(),
  xp: z.number(),
});
export type Assessment = z.infer<typeof AssessmentSchema>;

export const CheckinTurnSchema = z.object({
  checkinId: CheckinIdSchema,
  reply: z.string(),
  assessment: AssessmentSchema.nullable(),
  firstCompletionToday: z.boolean(),
  /** What the examiner saved to or changed in memory this turn. */
  remembered: z.array(z.string()),
});
export type CheckinTurn = z.infer<typeof CheckinTurnSchema>;

/** One word of a dictation, compared with the sentence (dictation.rs). */
export const DiffWordSchema = z.object({
  /** The word as said; empty for one typed that was not said. */
  expected: z.string(),
  /** What was typed in its place; empty for a word left out. */
  given: z.string(),
  /** "accents": right, but its Polish letters were left off — accepted, and shown. */
  status: z.enum(["right", "accents", "wrong", "missing", "extra"]),
});
export type DiffWord = z.infer<typeof DiffWordSchema>;

/** A dictation checked: right when every word was heard. */
export const DictationCheckedSchema = z.object({
  correct: z.boolean(),
  words: z.array(DiffWordSchema),
});
export type DictationChecked = z.infer<typeof DictationCheckedSchema>;

/** A word said that was heard as another, and the sound that likely did it (sounds.rs). */
export const SoundTipSchema = z.object({
  word: z.string(),
  heard: z.string(),
  /** The contrast: "ż or sz". */
  label: z.string(),
  /** What to try. */
  advice: z.string(),
  /** A built-in pair of the contrast to hear side by side, when it has one. */
  pair: z
    .object({
      id: z.string().nullable(),
      label: z.string(),
      a: z.string(),
      aEn: z.string(),
      b: z.string(),
      bEn: z.string(),
    })
    .nullable(),
});
export type SoundTip = z.infer<typeof SoundTipSchema>;

/** A spoken attempt compared with what was to be said (speaking.rs). */
export const SpokenSchema = z.object({
  correct: z.boolean(),
  words: z.array(DiffWordSchema),
  tips: z.array(SoundTipSchema),
});
export type Spoken = z.infer<typeof SpokenSchema>;

/** A recording heard: the words, and for a speaking card how they compare. */
export const HeardSchema = z.object({
  text: z.string(),
  spoken: SpokenSchema.nullable(),
});
export type Heard = z.infer<typeof HeardSchema>;

export const AnswerResultSchema = z.object({
  correct: z.boolean(),
  expected: z.string(),
  xpGained: z.number(),
  totalXp: z.number(),
  dayXp: z.number(),
  level: z.number(),
  /** A dictation (or a speaking card's best attempt), word by word; empty for every other kind. */
  diff: z.array(DiffWordSchema),
  /** A speaking card: the sounds to try; empty for every other kind. */
  tips: z.array(SoundTipSchema),
});
export type AnswerResult = z.infer<typeof AnswerResultSchema>;

export const AchievementSchema = z.object({
  code: z.string(),
  title: z.string(),
  /** What it is about, in a line. */
  description: z.string(),
  /** Exactly what to do to earn it. */
  requirement: z.string(),
  icon: z.string(),
  unlocked: z.boolean(),
  /** `current` of `target` `unit`s, from the numbers the unlock check reads; full once unlocked. */
  current: z.number(),
  target: z.number(),
  /** What is counted, singular: "day", "word". */
  unit: z.string(),
  /** The day number it was unlocked on. */
  unlockedDay: z.number().nullable(),
});
export type Achievement = z.infer<typeof AchievementSchema>;

export const DayStatSchema = z.object({
  day: z.number(),
  xp: z.number(),
  answered: z.number(),
  correct: z.number(),
  completed: z.boolean(),
  /** Missed, but a streak freeze kept the run going. */
  frozen: z.boolean(),
});
export type DayStat = z.infer<typeof DayStatSchema>;

export const CourseProgressSchema = z.object({
  course: CourseSchema,
  totalWords: z.number(),
  seenWords: z.number(),
  masteredWords: z.number(),
  totalSentences: z.number(),
  seenSentences: z.number(),
  totalLessons: z.number(),
  /** Lessons done at least once. */
  startedLessons: z.number(),
  /** The course's picture: a file id, loaded with fileUrl(). */
  imageFileId: FileIdSchema.nullable(),
  /** Completed: every core lesson passed once. Mastered: every lesson mastered, challenges too. */
  status: z.enum(["active", "completed", "mastered"]),
  /** Lessons needed to complete it (not challenges), and how many are passed. */
  coreLessons: z.number(),
  passedCore: z.number(),
  challenges: z.number(),
  /** Lessons, challenges too, already mastered. */
  masteredLessons: z.number(),
  /** Mastery points earned across its lessons, out of masteryMax (100 a lesson). */
  masteryPoints: z.number(),
  masteryMax: z.number(),
  /** About how many minutes of practice are left to master it. */
  minutesToMaster: z.number(),
  /** The first core lesson, in the course's order, not passed yet. */
  nextLesson: z.string().nullable(),
  /** A built-in course that never ends (Grammar & culture, Listening, Writing): always active. */
  openEnded: z.boolean(),
  /** When it was closed as mastered (Unix seconds), if it was. */
  closedAt: z.number().nullable(),
});
export type CourseProgress = z.infer<typeof CourseProgressSchema>;

/** What the courses closed as mastered add up to, each counted once. */
export const MasteredSummarySchema = z.object({
  courses: z.number(),
  lessons: z.number(),
  words: z.number(),
});
export type MasteredSummary = z.infer<typeof MasteredSummarySchema>;

/** A course just completed or mastered. */
/** A course just closed as mastered, for its celebration (courses.rs CourseClosed). */
export const CourseClosedSchema = z.object({
  title: z.string(),
  lessons: z.number(),
  words: z.number(),
  days: z.number(),
  xp: z.number(),
});
export type CourseClosed = z.infer<typeof CourseClosedSchema>;

/** Whether a course's picture is being drawn, or why it could not be (pictures.rs). */
export const CoursePictureStatusSchema = z.object({
  courseId: CourseIdSchema,
  drawing: z.boolean(),
  error: z.string().nullable(),
});
export type CoursePictureStatus = z.infer<typeof CoursePictureStatusSchema>;

export const MilestoneSchema = z.object({
  courseId: CourseIdSchema,
  title: z.string(),
  mastered: z.boolean(),
});
export type Milestone = z.infer<typeof MilestoneSchema>;

export const StatsSchema = z.object({
  /** The courses closed as mastered, together. */
  mastered: MasteredSummarySchema,
  day: z.number(),
  totalXp: z.number(),
  dayXp: z.number(),
  /** Lessons a day: how many today's list holds. */
  dailyLessons: z.number(),
  level: z.number(),
  xpIntoLevel: z.number(),
  xpForNextLevel: z.number(),
  currentStreak: z.number(),
  /** Freezes kept: each covers one missed day (at most two). */
  streakFreezes: z.number(),
  longestStreak: z.number(),
  wordsSeen: z.number(),
  wordsMastered: z.number(),
  wordsTotal: z.number(),
  sentencesSeen: z.number(),
  sentencesTotal: z.number(),
  dialoguesSeen: z.number(),
  dialoguesTotal: z.number(),
  dueToday: z.number(),
  accuracy: z.number(),
  completedToday: z.boolean(),
  history: z.array(DayStatSchema),
  achievements: z.array(AchievementSchema),
});
export type Stats = z.infer<typeof StatsSchema>;

export const SongLengthSchema = z.enum(["short", "medium", "long"]);
export type SongLength = z.infer<typeof SongLengthSchema>;

export const SettingsSchema = z.object({
  /** Lessons a day: how many today's list holds. */
  dailyLessons: z.number(),
  reminderHour: z.number(),
  reminderMinute: z.number(),
  remindersEnabled: z.boolean(),
  hasApiKey: z.boolean(),
  /** Empty until the learner loads the catalogue and picks one. */
  aiModel: z.string(),
  ttsVoiceFemale: z.string(),
  ttsVoiceMale: z.string(),
  ttsSpeed: z.number(),
  ttsPrefetch: z.boolean(),
  tzOffsetMinutes: z.number(),
  /** Empty means the songwriter uses the tutor model. */
  songwriterModel: z.string(),
  /** Plays the other side in writing lessons; empty means the tutor model. */
  partnerModel: z.string(),
  /** "Talk about it" chats; empty means the tutor model. */
  wordChatModel: z.string(),
  /** Models starred in the model picker, shown first. */
  favoriteModels: z.array(z.string()),
  /** Draws course pictures; empty means the default image model. */
  imageModel: z.string(),
  /** Finds the things in a scene lesson's picture; empty means the default. */
  visionModel: z.string(),
  /** Hears the learner speak; empty means the default. */
  hearingModel: z.string(),
  /** Speaking cards in lessons (kept on this device). */
  speaking: z.boolean(),
  hasKieKey: z.boolean(),
  sunoModel: z.string(),
  sunoModels: z.array(z.string()),
  /** What the songwriter writes for: about 2, 3 or 4–5 minutes. */
  songLength: SongLengthSchema,
  /** Empty until first stored. */
  theme: z.enum(["", "system", "light", "dark"]),
  playerRepeat: z.enum(["", "off", "all", "one"]),
  /** Taps and buzzes with the answers, on a phone (kept on this device). */
  haptics: z.boolean(),
});
export type Settings = z.infer<typeof SettingsSchema>;

export const SettingsPatchSchema = z.object({
  dailyLessons: z.number().optional(),
  reminderHour: z.number().optional(),
  reminderMinute: z.number().optional(),
  remindersEnabled: z.boolean().optional(),
  aiModel: z.string().optional(),
  ttsVoiceFemale: z.string().optional(),
  ttsVoiceMale: z.string().optional(),
  ttsSpeed: z.number().optional(),
  ttsPrefetch: z.boolean().optional(),
  songwriterModel: z.string().optional(),
  partnerModel: z.string().optional(),
  wordChatModel: z.string().optional(),
  favoriteModels: z.array(z.string()).optional(),
  imageModel: z.string().optional(),
  visionModel: z.string().optional(),
  hearingModel: z.string().optional(),
  speaking: z.boolean().optional(),
  sunoModel: z.string().optional(),
  songLength: SongLengthSchema.optional(),
  theme: z.enum(["system", "light", "dark"]).optional(),
  playerRepeat: z.enum(["off", "all", "one"]).optional(),
  haptics: z.boolean().optional(),
});
export type SettingsPatch = z.infer<typeof SettingsPatchSchema>;

export const SessionResultSchema = z.object({
  firstCompletionToday: z.boolean(),
  newlyUnlocked: z.array(AchievementSchema),
  stats: StatsSchema,
  /** Courses this completed or mastered. */
  milestones: z.array(MilestoneSchema),
});
export type SessionResult = z.infer<typeof SessionResultSchema>;

export const LessonFinishedSchema = SessionResultSchema.extend({
  lesson: LessonSchema,
  result: LessonResultSchema,
  /** Extra practice: the schedule did not move. */
  extra: z.boolean(),
  /** This drill's score: right answers out of the scored cards. */
  correct: z.number(),
  total: z.number(),
});
export type LessonFinished = z.infer<typeof LessonFinishedSchema>;

export const TutorActionSchema = z.object({
  kind: z.enum([
    "courseCreated",
    "courseUpdated",
    "courseDeleted",
    "programmeCreated",
    "programmeUpdated",
    "programmeDeleted",
    "settingsChanged",
    "songStarted",
    "songDraft",
    "songsDeleted",
    "lessonCreated",
    "lessonUpdated",
    "lessonDeleted",
    "lessonScheduled",
    "lessonSuggested",
    "memory",
    "playlistChanged",
  ]),
  label: z.string(),
  courseId: CourseIdSchema.nullable(),
  songId: SongIdSchema.nullable(),
  lessonId: LessonIdSchema.nullable(),
});
export type TutorAction = z.infer<typeof TutorActionSchema>;

export const ConversationSchema = z.object({
  id: ConversationIdSchema,
  title: z.string(),
  /** The last thing said, shortened. */
  preview: z.string(),
  messages: z.number(),
  /** Seconds since the epoch. */
  updatedAt: z.number(),
  /** The tutor is working on a reply in the background. */
  working: z.boolean(),
  /** Why the last reply failed, if it did. */
  error: z.string().nullable(),
  /** "word" for a chat about a word or sentence ("Talk about it"). */
  tag: z.string().nullable(),
});
export type Conversation = z.infer<typeof ConversationSchema>;

/** `tutor_talk_about`: the conversation about a word, new or carried on. */
export const TalkAboutSchema = z.object({
  conversationId: ConversationIdSchema,
  existing: z.boolean(),
});
export type TalkAbout = z.infer<typeof TalkAboutSchema>;

export const ChatMessageSchema = z.object({
  id: MessageIdSchema,
  conversationId: ConversationIdSchema,
  role: z.enum(["user", "assistant"]),
  content: z.string(),
  actions: z.array(TutorActionSchema),
});
export type ChatMessage = z.infer<typeof ChatMessageSchema>;

/** `tutor_send` returns at once; the reply follows in the background. */
export const TutorStartedSchema = z.object({
  conversationId: ConversationIdSchema,
  message: ChatMessageSchema,
});
export type TutorStarted = z.infer<typeof TutorStartedSchema>;

/** `tutor://done`: a background reply finished, or failed. */
export const TutorDoneSchema = z.object({
  conversationId: ConversationIdSchema,
  message: ChatMessageSchema.nullable(),
  error: z.string().nullable(),
});
export type TutorDone = z.infer<typeof TutorDoneSchema>;

/** What a tool is doing while an agent runs (`agent://status`). */
export const AgentStatusSchema = z.object({
  id: z.string(),
  label: z.string(),
});
export type AgentStatus = z.infer<typeof AgentStatusSchema>;

export const AiModelSchema = z.object({
  id: z.string(),
  name: z.string(),
  contextLength: z.number(),
  /** Dollars per input token, e.g. "0.0000003". */
  promptPrice: z.string(),
  /** Dollars per output token. */
  completionPrice: z.string(),
  /** Dollars per input token read from the provider's cache; "" when not reported. */
  cacheReadPrice: z.string(),
  /** When it was added to OpenRouter (seconds since the epoch). */
  created: z.number(),
  /** Non-empty for speech models: the voice names the model accepts. */
  voices: z.array(z.string()),
  /** Can call tools — the tutor needs this to act in the app. */
  tools: z.boolean(),
  /** Can make images — for course pictures. */
  images: z.boolean(),
  /** Can read images — for finding the things in a scene. */
  vision: z.boolean(),
  /** Takes sound in: can hear the learner speak. */
  hears: z.boolean(),
});
export type AiModel = z.infer<typeof AiModelSchema>;

export const AudioClipSchema = z.object({
  mime: z.string(),
  base64: z.string(),
  cached: z.boolean(),
});
export type AudioClip = z.infer<typeof AudioClipSchema>;

export const AudioCacheStatsSchema = z.object({
  clips: z.number(),
  bytes: z.number(),
  sentencesCached: z.number(),
  sentencesTotal: z.number(),
});
export type AudioCacheStats = z.infer<typeof AudioCacheStatsSchema>;

/** How much of one course can be played without the network. */
export const CourseAudioSchema = z.object({
  courseId: CourseIdSchema,
  title: z.string(),
  titlePl: z.string(),
  total: z.number(),
  stored: z.number(),
  bytes: z.number(),
});
export type CourseAudio = z.infer<typeof CourseAudioSchema>;

export const AudioOverviewSchema = z.object({
  clips: z.number(),
  bytes: z.number(),
  courses: z.array(CourseAudioSchema),
  /** Clips no course uses with the current voices. */
  unusedClips: z.number(),
  unusedBytes: z.number(),
});
export type AudioOverview = z.infer<typeof AudioOverviewSchema>;

export const AudioItemSchema = z.object({
  kind: z.enum(["word", "sentence", "line"]),
  text: z.string(),
  gender: z.string(),
  stored: z.boolean(),
  bytes: z.number(),
});
export type AudioItem = z.infer<typeof AudioItemSchema>;

/** A Wikipedia article a study lesson or a podcast is built on. */
export const WikiSourceSchema = z.object({
  id: SourceIdSchema,
  lessonId: LessonIdSchema.nullable(),
  songId: SongIdSchema.nullable(),
  lang: z.string(),
  title: z.string(),
  url: z.string(),
  summary: z.string(),
  imageFileId: FileIdSchema.nullable(),
  /** Polish Wikipedia has it too. */
  polishTitle: z.string().nullable(),
  checkedAt: z.number(),
});
export type WikiSource = z.infer<typeof WikiSourceSchema>;

/** A source's article, for the reader. */
export const ArticleViewSchema = z.object({
  lang: z.string(),
  title: z.string(),
  url: z.string(),
  /** Markdown: shown when the layout is not known. */
  content: z.string(),
  /** The article as Wikipedia lays it out (HTML), drawn by WikiArticle. */
  html: z.string().nullable(),
});
export type ArticleView = z.infer<typeof ArticleViewSchema>;

/** An article found on Wikipedia. */
export const WikiHitSchema = z.object({
  title: z.string(),
  snippet: z.string(),
});
export type WikiHit = z.infer<typeof WikiHitSchema>;

/** The outcome of checking a lesson against its sources. */
export const FactCheckSchema = z.object({
  lessonId: LessonIdSchema,
  title: z.string(),
  updated: z.boolean(),
  note: z.string().nullable(),
});
export type FactCheck = z.infer<typeof FactCheckSchema>;

export const CacheProgressSchema = z.object({
  done: z.number(),
  total: z.number(),
  failed: z.number(),
  finished: z.boolean(),
  error: z.string().nullable(),
});
export type CacheProgress = z.infer<typeof CacheProgressSchema>;

// ------------------------------------------------------------------ songs

/** One phrase: sung in English, then in Polish (choruses: Polish only). */
export const SongLineSchema = z.object({
  en: z.string(),
  pl: z.string(),
  /** In a podcast with a cast: who says it (absent or "": the narrator). */
  speaker: z.string().optional(),
  /** In a podcast: how it is said ("surprised"). */
  how: z.string().optional(),
});
export type SongLine = z.infer<typeof SongLineSchema>;

export const SongSectionKindSchema = z.enum([
  "verse",
  "chorus",
  "bridge",
  "intro",
  "outro",
]);
export type SongSectionKind = z.infer<typeof SongSectionKindSchema>;

export const SongSectionSchema = z.object({
  kind: SongSectionKindSchema,
  lines: z.array(SongLineSchema),
});
export type SongSection = z.infer<typeof SongSectionSchema>;

/** A host or a character in a podcast. */
export const CastMemberSchema = z.object({
  name: z.string(),
  /** "f" or "m". */
  gender: z.string(),
  /** How they come across, in a word ("curious"). */
  character: z.string(),
  /** The voice they speak in; chosen when the episode is recorded. */
  voice: z.string(),
});
export type CastMember = z.infer<typeof CastMemberSchema>;

/** How a podcast episode's audio is put together. */
export const PodcastAudioSchema = z.object({
  /** The English is said before each Polish sentence; off: only Polish is heard. */
  english: z.boolean(),
  /** Seconds of silence after the English, before the Polish. */
  pause: z.number(),
  /** How many times the Polish is said. */
  repeats: z.number(),
  /** The Polish read slowly. */
  slow: z.boolean(),
  /** "two" voices (English and Polish apart) or "one". */
  voices: z.enum(["one", "two"]),
  /** Who speaks besides the narrator, each in a voice of their own. */
  cast: z.array(CastMemberSchema).optional(),
});
export type PodcastAudio = z.infer<typeof PodcastAudioSchema>;

/** A news story a podcast told, as the news index had it. */
export const NewsItemSchema = z.object({
  title: z.string(),
  url: z.string(),
  /** The outlet's address, e.g. "pap.pl". */
  outlet: z.string(),
  /** "2026-11-16". */
  date: z.string(),
  image: z.string(),
  summary: z.string(),
});
export type NewsItem = z.infer<typeof NewsItemSchema>;

/** An article a podcast was written from. */
export const PodcastSourceSchema = z.object({
  title: z.string(),
  url: z.string(),
  /** "en" or "pl". */
  lang: z.string(),
});
export type PodcastSource = z.infer<typeof PodcastSourceSchema>;

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
  basedOn?: SongId | null;
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
  steps?: {
    kind: string;
    by: string;
    text: string;
    model: string;
    before: SongDraft | null;
  }[];
}
export const SongOriginSchema: z.ZodType<SongOrigin, z.ZodTypeDef, unknown> =
  z.object({
    brief: z.string(),
    requestedStyle: z.string(),
    requestedVocalGender: z.string(),
    requestedChorus: z.string(),
    length: z.string(),
    songwriterModel: z.string(),
    revisions: z.array(z.string()),
    /** What a podcast was written from: its Wikipedia article (and the Polish one). */
    sources: z.array(PodcastSourceSchema).optional(),
    sourceImage: z.string().optional(),
    requestedArticles: z.array(z.string()).optional(),
    story: z.boolean().optional(),
    requestedCast: z.string().optional(),
    /** Made from today's news, and the stories it told. */
    news: z.boolean().optional(),
    newsItems: z.array(NewsItemSchema).optional(),
    /** "composer" or "tutor". */
    madeBy: z.string().optional(),
    /** What the lyrics or script had to meet (the tutor's). */
    requirements: z.string().optional(),
    /** The song or podcast this one varies. */
    basedOn: SongIdSchema.nullable().optional(),
    /** Everything the writer was given besides the request. */
    given: z
      .object({
        words: z.array(z.string()),
        sentences: z.array(z.string()),
        weakWords: z.array(z.string()),
        existingChoruses: z.array(z.string()),
        memories: z.array(z.string()),
        reading: z.string(),
      })
      .optional(),
    instructionsVersion: z.string().optional(),
    /** Revisions and edits after the first draft, each with the draft before it. */
    steps: z
      .array(
        z.object({
          kind: z.string(),
          by: z.string(),
          text: z.string(),
          model: z.string(),
          before: z.lazy(() => SongDraftSchema).nullable(),
        }),
      )
      .optional(),
  });

export const SongDraftSchema = z.object({
  title: z.string(),
  chorusPl: z.string(),
  chorusEn: z.string(),
  style: z.string(),
  /** "f", "m" or "" for no preference. */
  vocalGender: z.string(),
  sections: z.array(SongSectionSchema),
  /** The course it is filed in (through its listening lesson). */
  courseId: CourseIdSchema.nullable().optional(),
  /** How it came about — kept with the song; pass it back unchanged. */
  origin: SongOriginSchema.optional(),
  /** "podcast" for a spoken episode; "song" (or absent) for a song. */
  kind: z.enum(["song", "podcast", ""]).optional(),
  /** For a podcast: how its audio is put together. */
  podcast: PodcastAudioSchema.nullable().optional(),
});
export type SongDraft = z.infer<typeof SongDraftSchema>;

/** Who talks in a podcast. */
export const PodcastCastSchema = z.enum(["narrator", "hosts", "characters"]);
export type PodcastCast = z.infer<typeof PodcastCastSchema>;

/** "pairs": English then Polish; "immersion": only Polish, in simpler sentences. */
export const PodcastFormatSchema = z.enum(["pairs", "immersion"]);
export type PodcastFormat = z.infer<typeof PodcastFormatSchema>;

export const PodcastRequestSchema = z.object({
  topic: z.string().optional(),
  /** English Wikipedia article titles to write from (0–5). */
  articles: z.array(z.string()).optional(),
  /** A short made-up story, from no articles. */
  story: z.boolean().optional(),
  /** Who talks; absent: hosts for articles, characters for a story. */
  cast: PodcastCastSchema.optional(),
  /** Today's news from Polish outlets (about `topic`, if given). */
  news: z.boolean().optional(),
  format: PodcastFormatSchema.optional(),
  length: SongLengthSchema.optional(),
  vocalGender: z.string().optional(),
  previous: SongDraftSchema.optional(),
  feedback: z.string().optional(),
});
export type PodcastRequest = z.infer<typeof PodcastRequestSchema>;

/** A subject the learner might like an episode about. */
export const TopicIdeaSchema = z.object({
  topic: z.string(),
  /** None: a story. */
  articles: z.array(z.string()),
  why: z.string(),
});
export type TopicIdea = z.infer<typeof TopicIdeaSchema>;

/** One sung line and when it is sung. */
export const TimedLineSchema = z.object({
  start: z.number(),
  end: z.number(),
  text: z.string(),
  /** "pl", "en", or "" for ad-libs that are not in the lyrics. */
  lang: z.string(),
  /** For a Polish line, its English meaning. */
  gloss: z.string().nullable(),
  /** Set on the first line of a section, e.g. "Chorus". */
  section: z.string().nullable(),
  /** In a podcast with a cast: who says it. */
  speaker: z.string().nullable().optional(),
});
export type TimedLine = z.infer<typeof TimedLineSchema>;

export const SongTrackSchema = z.object({
  id: TrackIdSchema,
  songId: SongIdSchema,
  ord: z.number(),
  /** The take's audio and cover art: file ids, loaded with fileUrl(). */
  audioFileId: FileIdSchema,
  imageFileId: FileIdSchema.nullable(),
  /** Copies on disk, only for Android's native player. */
  audioPath: z.string(),
  imagePath: z.string().nullable(),
  duration: z.number(),
  playCount: z.number(),
  /** Lyrics timed to this take, once Suno's word timings are known. */
  lines: z.array(TimedLineSchema).nullable(),
});
export type SongTrack = z.infer<typeof SongTrackSchema>;

export const SongSchema = SongDraftSchema.extend({
  id: SongIdSchema,
  status: z.enum(["generating", "ready", "failed"]),
  error: z.string().nullable(),
  createdAt: z.number(),
  tracks: z.array(SongTrackSchema),
});
export type Song = z.infer<typeof SongSchema>;

export const PlaylistEntrySchema = z.object({
  /** The entry itself; the same take may be in a playlist twice. */
  id: EntryIdSchema,
  trackId: TrackIdSchema,
});
export type PlaylistEntry = z.infer<typeof PlaylistEntrySchema>;

export const PlaylistSchema = z.object({
  id: PlaylistIdSchema,
  name: z.string(),
  entries: z.array(PlaylistEntrySchema),
});
export type Playlist = z.infer<typeof PlaylistSchema>;

export const DraftRequestSchema = z.object({
  length: SongLengthSchema.optional(),
  brief: z.string().optional(),
  chorusPl: z.string().optional(),
  style: z.string().optional(),
  vocalGender: z.string().optional(),
  previous: SongDraftSchema.optional(),
  feedback: z.string().optional(),
});
export type DraftRequest = z.infer<typeof DraftRequestSchema>;

// ------------------------------------------------------------------ casting

/** A Sonos room or group. */
export const CastDeviceSchema = z.object({
  id: z.string(),
  name: z.string(),
  baseUrl: z.string(),
});
export type CastDevice = z.infer<typeof CastDeviceSchema>;

export const CastItemSchema = z.object({
  trackId: TrackIdSchema,
  title: z.string(),
  artist: z.string(),
});
export type CastItem = z.infer<typeof CastItemSchema>;

export const CastStateSchema = z.object({
  device: CastDeviceSchema,
  items: z.array(CastItemSchema),
  index: z.number(),
  trackId: TrackIdSchema.nullable(),
  playing: z.boolean(),
  transport: z.string(),
  position: z.number(),
  duration: z.number(),
  /** 0–100, or -1 when the speaker did not say. */
  volume: z.number(),
  repeat: z.string(),
  /** The speaker has moved on to something that is not ours. */
  detached: z.boolean(),
});
export type CastState = z.infer<typeof CastStateSchema>;

/** What one AI feature cost, over a day or a window. */
export const FeatureCostSchema = z.object({
  feature: z.string(),
  costUsd: z.number(),
  calls: z.number(),
  /** Requests whose price the provider did not report (speech). */
  unpriced: z.number(),
});
export type FeatureCost = z.infer<typeof FeatureCostSchema>;

export const DayCostSchema = z.object({
  day: z.number(),
  costUsd: z.number(),
  calls: z.number(),
  features: z.array(FeatureCostSchema),
});
export type DayCost = z.infer<typeof DayCostSchema>;

export const CostSummarySchema = z.object({
  today: z.number(),
  days: z.number(),
  todayUsd: z.number(),
  totalUsd: z.number(),
  allTimeUsd: z.number(),
  features: z.array(FeatureCostSchema),
  /** Days with any AI use, newest first. */
  perDay: z.array(DayCostSchema),
});
export type CostSummary = z.infer<typeof CostSummarySchema>;

export const MemoryCategorySchema = z.enum([
  "goal",
  "preference",
  "context",
  "strength",
  "struggle",
]);
export type MemoryCategory = z.infer<typeof MemoryCategorySchema>;

/** One fact the agents remember about the learner. */
export const MemorySchema = z.object({
  id: MemoryIdSchema,
  content: z.string(),
  category: MemoryCategorySchema,
  /** "tutor", "examiner" or "you". */
  source: z.string(),
  createdAt: z.number(),
  updatedAt: z.number(),
});
export type Memory = z.infer<typeof MemorySchema>;

export const MemoryOverviewSchema = z.object({
  enabled: z.boolean(),
  memories: z.array(MemorySchema),
});
export type MemoryOverview = z.infer<typeof MemoryOverviewSchema>;

/** A phrase with its meaning. */
export const PhraseSchema = z.object({
  pl: z.string(),
  en: z.string(),
});
export type Phrase = z.infer<typeof PhraseSchema>;

/** A writing goal's help: a way to start, and words to use. */
export const GoalHelpSchema = z.object({
  goal: z.string(),
  starter: PhraseSchema,
  words: z.array(PhraseSchema),
});
export type GoalHelp = z.infer<typeof GoalHelpSchema>;

/** Tokens one feature used lately: what a month with another model is estimated from. */
export const FeatureUsageSchema = z.object({
  feature: z.string(),
  requests: z.number(),
  tokensIn: z.number(),
  tokensCached: z.number(),
  tokensOut: z.number(),
  /** Days the counts cover. */
  days: z.number(),
});
export type FeatureUsage = z.infer<typeof FeatureUsageSchema>;

/** One benchmark item's outcome. */
export const BenchItemSchema = z.object({
  part: z.enum(["polish", "tools", "rules"]),
  name: z.string(),
  /** 0–1. */
  score: z.number(),
  note: z.string(),
  answer: z.string(),
});
export type BenchItem = z.infer<typeof BenchItemSchema>;

/** A benchmark run on one model. */
export const BenchRunSchema = z.object({
  id: BenchRunIdSchema,
  model: z.string(),
  createdAt: z.number(),
  score: z.number(),
  polish: z.number(),
  tools: z.number(),
  rules: z.number(),
  /** Average seconds per reply. */
  seconds: z.number(),
  costUsd: z.number().nullable(),
  items: z.array(BenchItemSchema),
});
export type BenchRun = z.infer<typeof BenchRunSchema>;

/** A sentence that uses a word, shown when the word is answered wrong. */
export const WordExampleSchema = z.object({
  pl: z.string(),
  en: z.string(),
});
export type WordExample = z.infer<typeof WordExampleSchema>;

// Sync between the learner's devices (src-tauri/src/sync/service.rs,
// session.rs; docs/SYNC.md).

/** What sync is doing: the payload of the `sync-status` event. */
export const SyncActivitySchema = z.object({
  syncing: z.boolean(),
  lastError: z.string().nullable(),
  /** When the last round ended (ISO 8601, UTC). */
  lastRoundAt: z.string().nullable(),
});
export type SyncActivity = z.infer<typeof SyncActivitySchema>;

/** One of the vault's devices, as this device knows it. */
export const SyncDeviceSchema = z.object({
  id: DeviceIdSchema,
  name: z.string(),
  /** "android" | "ios" | "macos" | "windows" | "linux" */
  platform: z.string(),
  isSelf: z.boolean(),
  /** When the two last synced, and last ended holding the same data (ISO 8601). */
  lastSyncAt: z.string().nullable(),
  lastCompleteAt: z.string().nullable(),
  complete: z.boolean(),
});
export type SyncDevice = z.infer<typeof SyncDeviceSchema>;

export const SyncStatusSchema = SyncActivitySchema.extend({
  deviceId: DeviceIdSchema,
  name: z.string(),
  platform: z.string(),
  inVault: z.boolean(),
  devices: z.array(SyncDeviceSchema),
  /** A pairing code is showing and still good. */
  offerActive: z.boolean(),
});
export type SyncStatus = z.infer<typeof SyncStatusSchema>;

/** What a sync changed here: the payload of the `sync-changed` event. */
export const SyncReceivedSchema = z.object({
  applied: z.number(),
  conflicts: z.number(),
  uniqueMerges: z.number(),
  removed: z.number(),
});
export type SyncReceived = z.infer<typeof SyncReceivedSchema>;

export const SyncOutcomeSchema = z.object({
  peerId: z.string(),
  peerName: z.string(),
  received: SyncReceivedSchema,
  /** Both devices ended holding exactly the same data. */
  complete: z.boolean(),
  /** The tables that differed, when not. */
  differences: z.array(z.string()),
  digestSkipped: z.boolean(),
});
export type SyncOutcome = z.infer<typeof SyncOutcomeSchema>;

export const JoinResultSchema = z.object({
  outcome: SyncOutcomeSchema,
  /** Where the copy of this device's own data went, when it held any. */
  backup: z.string().nullable(),
});
export type JoinResult = z.infer<typeof JoinResultSchema>;

/** A pairing code for another device to scan, good for ten minutes, once. */
export const SyncOfferSchema = z.object({
  code: z.string(),
  /** The code as a QR code (SVG markup). */
  svg: z.string(),
  /** ISO 8601, UTC. */
  expiresAt: z.string(),
});
export type SyncOffer = z.infer<typeof SyncOfferSchema>;

/** One table's worth of the learner's data: what joining would replace. */
export const SyncHeldSchema = z.object({
  table: z.string(),
  rows: z.number(),
});
export type SyncHeld = z.infer<typeof SyncHeldSchema>;

// ---------------------------------------------------------------------------
// Learning analytics (analytics.rs): the same queries the tutor's tools use.
// ---------------------------------------------------------------------------

/** Which answers: a span of days (inclusive) and one course. */
export const FilterSchema = z.object({
  fromDay: z.number().nullable(),
  toDay: z.number().nullable(),
  courseId: CourseIdSchema.nullable(),
});
export type Filter = z.infer<typeof FilterSchema>;

export const ClassCountSchema = z.object({ class: z.string(), label: z.string(), count: z.number() });
export type ClassCount = z.infer<typeof ClassCountSchema>;

export const DayPointSchema = z.object({ day: z.number(), attempts: z.number(), correct: z.number() });
export type DayPoint = z.infer<typeof DayPointSchema>;

/** One word of a sentence, compared: r right, a letters, w wrong, e ending, m missing, x extra, o out of order. */
export const WordResultSchema = z.object({ e: z.string(), g: z.string(), s: z.string() });
export type WordResult = z.infer<typeof WordResultSchema>;

export const AttemptViewSchema = z.object({
  id: AttemptIdSchema,
  at: z.number(),
  day: z.number(),
  exercise: z.string(),
  exerciseLabel: z.string(),
  itemKind: z.string(),
  itemId: z.string(),
  item: z.string(),
  lessonId: LessonIdSchema.nullable(),
  correct: z.boolean(),
  given: z.string(),
  expected: z.string(),
  mistake: z.string(),
  mistakeLabel: z.string(),
  detail: z.string(),
  contrast: z.string(),
  msAnswer: z.number().nullable(),
  words: z.array(WordResultSchema).optional(),
});
export type AttemptView = z.infer<typeof AttemptViewSchema>;

export const AreaSchema = z.object({
  kind: z.enum(["structure", "sound", "exercise"]),
  key: z.string(),
  name: z.string(),
  attempts: z.number(),
  accuracy: z.number(),
});
export type Area = z.infer<typeof AreaSchema>;

export const OverviewSchema = z.object({
  filter: FilterSchema,
  attempts: z.number(),
  correct: z.number(),
  accuracy: z.number(),
  items: z.number(),
  daysActive: z.number(),
  minutes: z.number(),
  mistakes: z.array(ClassCountSchema),
  lettersForgiven: z.number(),
  daily: z.array(DayPointSchema),
  accuracyBefore: z.number().nullable(),
  trend: z.enum(["improving", "slipping", "steady", "new"]),
  strongest: z.array(AreaSchema),
  weakest: z.array(AreaSchema),
  wordsMet: z.number(),
  structuresMet: z.number(),
  soundsMet: z.number(),
  untagged: z.number(),
});
export type Overview = z.infer<typeof OverviewSchema>;

export const HardKindSchema = z.enum(["word", "sentence", "structure", "sound"]);
export type HardKind = z.infer<typeof HardKindSchema>;

export const HardItemSchema = z.object({
  kind: HardKindSchema,
  key: z.string(),
  title: z.string(),
  subtitle: z.string(),
  attempts: z.number(),
  wrong: z.number(),
  accuracy: z.number(),
  score: z.number(),
  mistakes: z.array(ClassCountSchema),
  last: z.array(AttemptViewSchema),
});
export type HardItem = z.infer<typeof HardItemSchema>;

export const StrengthSchema = z.enum(["new", "learning", "solid", "tricky"]);
export type Strength = z.infer<typeof StrengthSchema>;

export const TagInfoSchema = z.object({
  id: GrammarTagIdSchema,
  key: z.string(),
  name: z.string(),
  group: z.string(),
  explain: z.string(),
});
export type TagInfo = z.infer<typeof TagInfoSchema>;

/** Optional on the wire: a few fields, only when set. */
export const EventDetailsSchema = z.object({
  v: z.number(),
  atCard: z.number().optional(),
  ofCards: z.number().optional(),
  answered: z.number().optional(),
  correct: z.number().optional(),
  reason: z.string().optional(),
  word: z.string().optional(),
  slow: z.boolean().optional(),
  game: z.string().optional(),
  ms: z.number().optional(),
  source: z.string().optional(),
  exercise: z.string().optional(),
});
export type EventDetails = z.infer<typeof EventDetailsSchema>;

export const EventViewSchema = z.object({
  at: z.number(),
  kind: z.string(),
  label: z.string(),
  details: EventDetailsSchema,
});
export type EventView = z.infer<typeof EventViewSchema>;

export const ItemHistorySchema = z.object({
  itemKind: z.string(),
  itemId: z.string(),
  pl: z.string(),
  en: z.string(),
  lessonId: LessonIdSchema.nullable(),
  strength: StrengthSchema,
  attempts: z.number(),
  wrong: z.number(),
  mistakes: z.array(ClassCountSchema),
  history: z.array(AttemptViewSchema),
  events: z.array(EventViewSchema),
  grammar: z.array(TagInfoSchema),
});
export type ItemHistory = z.infer<typeof ItemHistorySchema>;

export const MetWordSchema = z.object({
  id: WordIdSchema,
  pl: z.string(),
  en: z.string(),
  pos: z.string(),
  courses: z.array(z.object({ id: CourseIdSchema, title: z.string() })),
  strength: StrengthSchema,
  attempts: z.number(),
  wrong: z.number(),
  score: z.number(),
  lastAt: z.number().nullable(),
  image: FileIdSchema.nullable(),
});
export type MetWord = z.infer<typeof MetWordSchema>;

export const MetWordDetailSchema = z.object({
  word: MetWordSchema,
  lessons: z.array(z.object({ id: LessonIdSchema, title: z.string() })),
  examples: z.array(z.object({ pl: z.string(), en: z.string() })),
  near: z.object({ pl: z.string(), en: z.string() }).nullable(),
  history: ItemHistorySchema,
});
export type MetWordDetail = z.infer<typeof MetWordDetailSchema>;

export const StructureRowSchema = z.object({
  tag: TagInfoSchema,
  items: z.number(),
  attempts: z.number(),
  correct: z.number(),
  accuracy: z.number().nullable(),
  score: z.number(),
  weeks: z.array(DayPointSchema),
});
export type StructureRow = z.infer<typeof StructureRowSchema>;

export const StructuresSchema = z.object({
  tags: z.array(StructureRowSchema),
  untagged: z.number(),
});
export type Structures = z.infer<typeof StructuresSchema>;

export const StructureDetailSchema = z.object({
  row: StructureRowSchema,
  examples: z.array(
    z.object({
      itemKind: z.string(),
      itemId: z.string(),
      pl: z.string(),
      en: z.string(),
      attempts: z.number(),
      wrong: z.number(),
    }),
  ),
  classes: z.array(ClassCountSchema),
  mistakes: z.array(AttemptViewSchema),
});
export type StructureDetail = z.infer<typeof StructureDetailSchema>;

export const SoundRowSchema = z.object({
  label: z.string(),
  attempts: z.number(),
  hits: z.number(),
  accuracy: z.number(),
  sides: z.array(z.object({ said: z.string(), attempts: z.number(), missed: z.number(), heardAs: z.string() })),
  examples: z.array(AttemptViewSchema),
});
export type SoundRow = z.infer<typeof SoundRowSchema>;

export const MistakePageSchema = z.object({
  class: z.string(),
  label: z.string(),
  count: z.number(),
  share: z.number(),
  items: z.array(z.object({ itemKind: z.string(), itemId: z.string(), title: z.string(), count: z.number() })),
  examples: z.array(AttemptViewSchema),
});
export type MistakePage = z.infer<typeof MistakePageSchema>;

const SummarySchema = z.object({
  filter: FilterSchema,
  attempts: z.number(),
  correct: z.number(),
  accuracy: z.number(),
  daysActive: z.number(),
});

export const ChangeSchema = z.object({
  kind: HardKindSchema,
  key: z.string(),
  title: z.string(),
  attemptsA: z.number(),
  accuracyA: z.number(),
  attemptsB: z.number(),
  accuracyB: z.number(),
});
export type Change = z.infer<typeof ChangeSchema>;

export const ComparisonSchema = z.object({
  a: SummarySchema,
  b: SummarySchema,
  improved: z.array(ChangeSchema),
  slipped: z.array(ChangeSchema),
  mistakes: z.array(z.object({ class: z.string(), label: z.string(), a: z.number(), b: z.number() })),
});
export type Comparison = z.infer<typeof ComparisonSchema>;

export const ActivitySchema = z.object({
  filter: FilterSchema,
  daysActive: z.number(),
  minutes: z.number(),
  attempts: z.number(),
  sessions: z.number(),
  finished: z.number(),
  leftEarly: z.number(),
  games: z.number(),
  checkins: z.number(),
  byHour: z.array(z.number()),
  byWeekday: z.array(z.number()),
  medianMs: z.number().nullable(),
  audioPlays: z.number(),
  reveals: z.number(),
  exits: z.array(
    z.object({
      lessonId: LessonIdSchema.nullable(),
      title: z.string(),
      at: z.number(),
      atCard: z.number().nullable(),
      ofCards: z.number().nullable(),
    }),
  ),
  lessons: z.array(z.object({ lessonId: LessonIdSchema, title: z.string(), attempts: z.number(), accuracy: z.number() })),
  events: z.array(ClassCountSchema),
});
export type Activity = z.infer<typeof ActivitySchema>;

/** A quick game's answer, graded and logged (games.rs). */
export const GameAnsweredSchema = z.object({
  correct: z.boolean(),
  words: z.array(DiffWordSchema),
  tips: z.array(SoundTipSchema),
});
export type GameAnswered = z.infer<typeof GameAnsweredSchema>;
