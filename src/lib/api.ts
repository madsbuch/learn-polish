import { invoke, type InvokeArgs } from "@tauri-apps/api/core";
import { z } from "zod";

import { describe, logCause, logError, markLogged } from "./log";

import type {
  BenchRunId,
  CheckinId,
  ConversationId,
  CourseId,
  DeviceId,
  EntryId,
  FileId,
  LessonId,
  PanelId,
  MemoryId,
  PlaylistId,
  SongId,
  SourceId,
  TrackId,
  WordId,
  MemoryCategory,
  CastDevice,
  DraftRequest,
  PodcastAudio,
  PodcastRequest,
  SongDraft,
  SpeechLine,
  SettingsPatch,
  QuickGameKind,
  Filter,
  HardKind,
  PracticeContext,
  PracticeSource,
  RawCursor,
  RawFilter,
} from "./types";
import {
  ActivitySchema,
  ContextStatsSchema,
  RawPageSchema,
  TopicDetailSchema,
  TopicsPageSchema,
  ComparisonSchema,
  GameAnsweredSchema,
  HardItemSchema,
  ItemHistorySchema,
  MetWordDetailSchema,
  MetWordSchema,
  MistakePageSchema,
  OverviewSchema,
  SoundRowSchema,
  StructureDetailSchema,
  StructuresSchema,
  GameFinishedSchema,
  DictationCheckedSchema,
  HeardSchema,
  SpokenSchema,
  GlossWordSchema,
  QuickGameSchema,
  QuickGamesSchema,
  FileIdSchema,
  CoursePictureStatusSchema,
  JoinResultSchema,
  ResolvedIdSchema,
  SyncHeldSchema,
  SyncOfferSchema,
  SyncStatusSchema,
  AiModelSchema,
  AnswerResultSchema,
  ArticleViewSchema,
  AudioCacheStatsSchema,
  AudioClipSchema,
  AudioItemSchema,
  AudioOverviewSchema,
  BenchRunSchema,
  CacheProgressSchema,
  CastDeviceSchema,
  CastStateSchema,
  CataloguePageSchema,
  ChatMessageSchema,
  CheckinTurnSchema,
  ConversationSchema,
  CostSummarySchema,
  CourseClosedSchema,
  CourseProgressSchema,
  DiagnosticEntrySchema,
  FactCheckSchema,
  FeatureUsageSchema,
  GoalHelpSchema,
  LessonSchema,
  LessonDetailSchema,
  LessonFinishedSchema,
  LessonOverviewSchema,
  LessonTypeInfoSchema,
  MemoryOverviewSchema,
  PlaylistSchema,
  SentenceSchema,
  SessionPlanSchema,
  SessionResultSchema,
  SettingsSchema,
  SongSchema,
  SongDraftSchema,
  SpeechLineSchema,
  StatsSchema,
  TalkAboutSchema,
  TimedLineSchema,
  TopicIdeaSchema,
  TutorStartedSchema,
  VoiceSchema,
  WikiHitSchema,
  WikiSourceSchema,
  WordSchema,
  WordExampleSchema,
  XpDaySchema,
  XpHistoryDaySchema,
} from "./types";

/** A command's result that did not have the shape its schema says. */
export class ShapeError extends Error {
  constructor(command: string, issues: z.ZodIssue[]) {
    const where = issues
      .slice(0, 3)
      .map((issue) => `${issue.path.join(".") || "(result)"}: ${issue.message}`)
      .join("; ");
    super(`The app's core sent an unexpected answer to ${command} (${where}).`);
    this.name = "ShapeError";
  }
}

/**
 * Runs a command in the Rust core and checks its result against `schema`:
 * nothing crosses into the screens unchecked.
 */
async function call<S extends z.ZodTypeAny>(
  schema: S,
  command: string,
  args?: InvokeArgs,
): Promise<z.infer<S>> {
  const raw = await invokeLogged(command, args);
  const parsed = schema.safeParse(raw);
  if (!parsed.success) {
    const error = new ShapeError(command, parsed.error.issues);
    logCause("shape", error);
    throw error;
  }
  return parsed.data;
}

/** Runs a command; a failure goes in the Diagnostics log (never its arguments, which can hold keys). */
async function invokeLogged(
  command: string,
  args?: InvokeArgs,
): Promise<unknown> {
  try {
    return await invoke<unknown>(command, args);
  } catch (cause) {
    logError("command", `${command}: ${describe(cause)}`);
    markLogged(cause);
    throw cause;
  }
}

/** How an answer was given (learnlog::Signals). */
export interface AnswerSignals {
  msFirst?: number;
  msAnswer?: number;
  tries?: number;
  hints?: number;
  plays?: number;
  slowPlays?: number;
  reveals?: number;
  changed?: boolean;
  firstPick?: string;
}

/** What a card showed beyond its prompt and options (learnlog::CardShown). */
export interface CardShownIn {
  key: string;
  position?: number;
  ofCards?: number;
  hint?: string;
  tokens?: string[];
  image?: FileId | null;
  meaning?: string;
}

/** An event for the learning log (learnlog::EventIn). */
export interface LearningEventIn {
  kind: LearningEventKind;
  itemKind?: string;
  itemId?: string;
  lessonId?: LessonId;
  run?: string;
  at?: number;
  /** Where it happened: the course and how it was reached (learnLog.ts fills these in). */
  courseId?: CourseId | null;
  source?: PracticeSource;
  details?: {
    atCard?: number;
    ofCards?: number;
    answered?: number;
    correct?: number;
    reason?: string;
    word?: string;
    slow?: boolean;
    game?: string;
    ms?: number;
    source?: string;
    exercise?: string;
  };
}

/** Every kind of event (learnlog::EVENT_KINDS). */
export type LearningEventKind =
  | "lesson_opened"
  | "lesson_started"
  | "lesson_left"
  | "lesson_finished"
  | "item_viewed"
  | "spot_explored"
  | "meaning_tapped"
  | "talk_opened"
  | "audio_played"
  | "card_skipped"
  | "game_started"
  | "game_finished"
  | "checkin_started"
  | "report_asked";

/** Runs a command whose result is nothing (Rust's `()`); nothing is read back. */
async function run(command: string, args?: InvokeArgs): Promise<void> {
  await invokeLogged(command, args);
}

/** Minutes to add to UTC to get local time — the backend needs it to know when
 *  "today" starts, and it can't determine that reliably by itself. */
function tzOffsetMinutes(): number {
  return -new Date().getTimezoneOffset();
}

export const api = {
  init: () =>
    call(SettingsSchema, "init_app", { tzOffsetMinutes: tzOffsetMinutes() }),
  getSettings: () => call(SettingsSchema, "get_settings"),
  updateSettings: (patch: SettingsPatch) =>
    call(SettingsSchema, "update_settings", { patch }),

  submitAnswer: (answer: {
    kind: string;
    itemKind: string;
    itemId: string;
    isNew: boolean;
    expected: string;
    given: string;
    /** Extra practice (the lesson is not due): half XP. */
    extra?: boolean;
    /** The lesson, and this go at it: for the XP log. */
    lessonId?: LessonId;
    run?: string;
    /** For the learning log: the options shown, what the card showed, its Polish. */
    options?: string[];
    prompt?: string;
    pl?: string;
    signals?: AnswerSignals;
    /** Where it is practised from (the plan's context, handed back). */
    context?: PracticeContext;
    /** What else the card showed: its key, place in the run, hint, tiles. */
    card?: CardShownIn;
  }) => call(AnswerResultSchema, "submit_answer", { answer }),
  xpDay: (day: number) => call(XpDaySchema, "xp_day", { day }),
  /** Events for the learning log, in one batch (learnLog.ts sends them). */
  learningLog: (events: LearningEventIn[]) => run("learning_log", { events }),
  /** A graded quick-game answer (Dictation, Sound pairs, Say it), logged. */
  quickGameAnswer: (answer: {
    game: "dictation" | "sounds" | "say";
    itemId?: string;
    expected: string;
    given: string;
    options?: string[];
    run?: string;
    signals?: AnswerSignals;
    card?: CardShownIn;
  }) => call(GameAnsweredSchema, "quick_game_answer", { answer }),
  /** A page of the raw learning log, newest first, after `before`. */
  rawLog: (filter: RawFilter, before: RawCursor | null, limit = 50) =>
    call(RawPageSchema, "learning_raw_log", { filter, before, limit }),
  /** Saves the filtered log as CSV or JSON where the learner picks; null if cancelled. */
  rawLogExport: (filter: RawFilter, format: "csv" | "json") =>
    call(z.string().nullable(), "learning_log_export", { filter, format }),
  contextStats: (subject: { itemId?: string; tag?: string; topic?: string }, groupBy: string, filter: Filter) =>
    call(ContextStatsSchema, "context_stats", { subject, groupBy, filter }),
  metTopics: (filter: Filter) => call(TopicsPageSchema, "met_topics", { filter }),
  metTopic: (key: string, filter: Filter) => call(TopicDetailSchema, "met_topic", { key, filter }),
  analyticsOverview: (filter: Filter) => call(OverviewSchema, "analytics_overview", { filter }),
  analyticsHard: (kind: HardKind, filter: Filter, limit = 50) =>
    call(z.array(HardItemSchema), "analytics_hard", { kind, filter, limit }),
  analyticsMistakes: (cls: string, filter: Filter) =>
    call(MistakePageSchema, "analytics_mistakes", { class: cls, filter }),
  analyticsCompare: (a: Filter, b: Filter) => call(ComparisonSchema, "analytics_compare", { a, b }),
  analyticsActivity: (filter: Filter) => call(ActivitySchema, "analytics_activity", { filter }),
  analyticsItem: (itemId: string) => call(ItemHistorySchema, "analytics_item", { itemId }),
  metWords: () => call(z.array(MetWordSchema), "met_words_list"),
  metWord: (wordId: WordId) => call(MetWordDetailSchema, "met_word_detail", { wordId }),
  metStructures: (filter: Filter) => call(StructuresSchema, "met_structures", { filter }),
  metStructure: (key: string, filter: Filter) => call(StructureDetailSchema, "met_structure", { key, filter }),
  metSounds: (filter: Filter) => call(z.array(SoundRowSchema), "met_sounds", { filter }),
  quickGames: () => call(QuickGamesSchema, "quick_games"),
  quickGameDeal: (game: QuickGameKind) => call(QuickGameSchema, "quick_game_deal", { game }),
  quickGameFinish: (game: QuickGameKind, pairs: number, ms: number, moves: number) =>
    call(GameFinishedSchema, "quick_game_finish", { game, pairs, ms, moves }),
  dictationCheck: (expected: string, given: string) =>
    call(DictationCheckedSchema, "dictation_check", { expected, given }),
  /** Hears a recording (base64, in `format`): for a speaking card, compared
   *  with `expected`; for a chat box (null), just the words. */
  speechHear: (audio: string, format: string, expected: string | null) =>
    call(HeardSchema, "speech_hear", { audio, format, expected }),
  speechCheck: (expected: string, heard: string) => call(SpokenSchema, "speech_check", { expected, heard }),
  xpHistory: (days: number) =>
    call(z.array(XpHistoryDaySchema), "xp_history", { days }),
  lessonsOverview: () => call(LessonOverviewSchema, "lessons_overview"),
  lessonsForCourse: (courseId: CourseId) =>
    call(z.array(LessonSchema), "lessons_for_course", { courseId }),
  /** A course's lessons, most important first, the mastered ones last. */
  lessonsByImportance: (courseId: CourseId) =>
    call(z.array(LessonSchema), "lessons_by_importance", { courseId }),
  lessonDetail: (lessonId: LessonId) =>
    call(LessonDetailSchema, "lesson_detail", { lessonId }),
  lessonTypes: () => call(z.array(LessonTypeInfoSchema), "lesson_types"),
  /** What each TypeID names, for showing titles in place of ids. */
  /** Whether the database is being upgraded (asked while starting up). */
  dbUpgrading: () => call(z.boolean(), "db_upgrading"),
  idsResolve: (ids: string[]) =>
    call(z.array(ResolvedIdSchema), "ids_resolve", { ids }),
  diagnostics: () => call(z.array(DiagnosticEntrySchema), "diagnostics_list"),
  diagnosticsClear: () => run("diagnostics_clear"),
  /** A drill's plan; `context` says where it was opened from (the log keeps it). */
  lessonSession: (lessonId: LessonId, context?: PracticeContext) =>
    call(SessionPlanSchema, "lesson_session", { lessonId, context: context ?? null }),
  /** A drill is done: its score schedules the next check-in. */
  lessonFinish: (lessonId: LessonId, correct: number, total: number, context?: PracticeContext) =>
    call(LessonFinishedSchema, "lesson_finish", { lessonId, correct, total, context: context ?? null }),
  lessonSetPeriod: (lessonId: LessonId, periodDays: number) =>
    call(LessonSchema, "lesson_set_period", { lessonId, periodDays }),
  lessonDelete: (lessonId: LessonId) => run("lesson_delete", { lessonId }),
  /** A challenge: needed to master its courses, not to complete them. */
  lessonSetChallenge: (lessonId: LessonId, challenge: boolean) =>
    call(LessonSchema, "lesson_set_challenge", { lessonId, challenge }),
  lessonAddToCourse: (lessonId: LessonId, courseId: CourseId) =>
    run("lesson_add_to_course", { lessonId, courseId }),
  /** True when it was the lesson's last course, so the lesson was deleted. */
  lessonRemoveFromCourse: (lessonId: LessonId, courseId: CourseId) =>
    call(z.boolean(), "lesson_remove_from_course", { lessonId, courseId }),
  /** The listening lesson for a song, made on first ask. */
  lessonForSong: (songId: SongId) =>
    call(LessonSchema, "lesson_for_song", { songId }),
  checkinStart: (lessonId: LessonId, streamId: string, context?: PracticeContext) =>
    call(CheckinTurnSchema, "checkin_start", { lessonId, streamId, context: context ?? null }),
  checkinSend: (checkinId: CheckinId, message: string, streamId: string) =>
    call(CheckinTurnSchema, "checkin_send", { checkinId, message, streamId }),
  /** Ends a writing conversation; the partner gives its verdict. */
  checkinEnd: (checkinId: CheckinId, streamId: string) =>
    call(CheckinTurnSchema, "checkin_end", { checkinId, streamId }),
  /** A writing lesson's word bank (made once, then kept). */
  writingScaffold: (lessonId: LessonId) =>
    call(z.array(GoalHelpSchema), "writing_scaffold", { lessonId }),
  /** A hint for the next message in a writing conversation. */
  writingHint: (checkinId: CheckinId) =>
    call(z.string(), "writing_hint", { checkinId }),
  /** Two sentences that use a word, shown after it is answered wrong. */
  wordExamples: (wordId: WordId) =>
    call(z.array(WordExampleSchema), "word_examples", { wordId }),
  /** Adds audio players to a lesson's Polish examples (its text otherwise unchanged). */
  lessonAddPlayers: (lessonId: LessonId) =>
    call(LessonDetailSchema, "lesson_add_players", { lessonId }),
  /** The summary after a conversation lesson; new achievements are those since it began. */
  checkinSummary: (checkinId: CheckinId, firstCompletionToday: boolean) =>
    call(SessionResultSchema, "checkin_summary", {
      checkinId,
      firstCompletionToday,
    }),

  getStats: () => call(StatsSchema, "get_stats"),
  getCourseProgress: () =>
    call(z.array(CourseProgressSchema), "get_course_progress"),
  browseWords: (courseId: CourseId | null, query: string, limit = 300) =>
    call(z.array(WordSchema), "browse_words", { courseId, query, limit }),
  browseCatalogue: (courseId: CourseId | null, query: string, limit = 200) =>
    call(CataloguePageSchema, "browse_catalogue", { courseId, query, limit }),
  browseSentences: (courseId: CourseId) =>
    call(z.array(SentenceSchema), "browse_sentences", { courseId }),
  resetProgress: () => run("reset_progress"),

  aiSetKey: (key: string) => call(SettingsSchema, "ai_set_key", { key }),
  aiClearKey: () => call(SettingsSchema, "ai_clear_key"),
  aiTestKey: () => call(z.string(), "ai_test_key"),
  aiListModels: () => call(z.array(AiModelSchema), "ai_list_models"),
  /** Tokens each feature used over the last 30 days. */
  aiUsage: () => call(z.array(FeatureUsageSchema), "ai_usage"),
  /** Runs the benchmark on a model (progress: `bench://progress`). */
  benchRun: (model: string) => call(BenchRunSchema, "bench_run", { model }),
  benchRuns: () => call(z.array(BenchRunSchema), "bench_runs"),
  benchDelete: (id: BenchRunId) => run("bench_delete", { id }),
  /**
   * Opens the tutor conversation about a word or sentence — or carries on the
   * one already about it. A new one starts with the tutor talking.
   */
  tutorTalkAbout: (
    itemKind: string,
    itemId: string,
    polish?: string,
    meaning?: string,
  ) =>
    call(TalkAboutSchema, "tutor_talk_about", {
      itemKind,
      itemId,
      polish: polish ?? null,
      meaning: meaning ?? null,
    }),

  /**
   * Stores the message and starts the reply in the background; `conversationId`
   * null starts a new conversation. The reply streams under
   * `tutor-<conversation id>` and arrives as `tutor://done`.
   */
  tutorSend: (message: string, conversationId: ConversationId | null) =>
    call(TutorStartedSchema, "tutor_send", { message, conversationId }),
  /** Asks again after a reply failed. */
  tutorRetry: (conversationId: ConversationId) =>
    run("tutor_retry", { conversationId }),
  /** Whether the app is on screen: off screen, finished replies notify. */
  appVisibility: (visible: boolean) => run("app_visibility", { visible }),
  /** A study lesson's Wikipedia sources. */
  lessonSources: (lessonId: LessonId) =>
    call(z.array(WikiSourceSchema), "lesson_sources", { lessonId }),
  /** A source's article, in its own language or in Polish. */
  sourceArticle: (sourceId: SourceId, polish: boolean) =>
    call(ArticleViewSchema, "source_article", { sourceId, polish }),
  sourceRemove: (sourceId: SourceId) => run("source_remove", { sourceId }),
  /** Checks a lesson's lesson against its sources now. */
  factCheckLesson: (lessonId: LessonId) =>
    call(FactCheckSchema, "fact_check_lesson", { lessonId }),
  /** Draws a new picture for a course; returns its file id. */
  coursePicture: (courseId: CourseId) =>
    call(FileIdSchema, "course_picture", { courseId }),
  /** Draws the pictures a lesson lacks, in the background; resolves to how many. */
  lessonAddPictures: (lessonId: LessonId) =>
    call(z.number(), "lesson_add_pictures", { lessonId }),
  wordPictureDraw: (wordId: WordId) =>
    call(FileIdSchema, "word_picture_draw", { wordId }),
  wordPictureDelete: (wordId: WordId) => run("word_picture_delete", { wordId }),
  /** A scene lesson's picture made again: its things found again, or (`redraw`) a new picture. */
  sceneMake: (lessonId: LessonId, redraw: boolean) => run("scene_make", { lessonId, redraw }),
  /** A story's panel drawn again, or (`panelId` null) every panel that failed. */
  storyDraw: (lessonId: LessonId, panelId: PanelId | null) => run("story_draw", { lessonId, panelId }),
  /** Whether its picture is being drawn (in the background too), or why it could not be. */
  coursePictureStatus: (courseId: CourseId) =>
    call(CoursePictureStatusSchema, "course_picture_status", { courseId }),
  coursePictureDelete: (courseId: CourseId) =>
    run("course_picture_delete", { courseId }),
  tutorConversations: () =>
    call(z.array(ConversationSchema), "tutor_conversations"),
  tutorHistory: (conversationId: ConversationId) =>
    call(z.array(ChatMessageSchema), "tutor_history", { conversationId }),
  tutorRename: (conversationId: ConversationId, title: string) =>
    run("tutor_rename", { conversationId, title }),
  tutorDelete: (conversationId: ConversationId) =>
    run("tutor_delete", { conversationId }),
  courseDelete: (courseId: CourseId) => run("course_delete", { courseId }),
  /** Closes a mastered course: a trophy, out of play, for good. */
  courseClose: (courseId: CourseId) =>
    call(CourseClosedSchema, "course_close", { courseId }),
  courseRename: (
    courseId: CourseId,
    title: string,
    titlePl: string,
    summary: string,
  ) => run("course_rename", { courseId, title, titlePl, summary }),
  /** Resolves to where the file went, or null if the dialog was cancelled. */
  exportDatabase: (includeKeys: boolean) =>
    call(z.string().nullable(), "export_database", { includeKeys }),

  memoriesOverview: () => call(MemoryOverviewSchema, "memories_overview"),
  memoriesSetEnabled: (enabled: boolean) =>
    call(MemoryOverviewSchema, "memories_set_enabled", { enabled }),
  memoryAdd: (content: string, category: MemoryCategory) =>
    call(MemoryOverviewSchema, "memory_add", { content, category }),
  memoryUpdate: (id: MemoryId, content: string, category: MemoryCategory) =>
    call(MemoryOverviewSchema, "memory_update", { id, content, category }),
  memoryDelete: (id: MemoryId) =>
    call(MemoryOverviewSchema, "memory_delete", { id }),
  memoriesClear: () => call(MemoryOverviewSchema, "memories_clear"),

  costsSummary: (days: number) =>
    call(CostSummarySchema, "costs_summary", { days }),

  listVoices: () => call(z.array(VoiceSchema), "list_voices"),
  previewVoice: (voice: string, text: string) =>
    call(AudioClipSchema, "preview_voice", { voice, text }),
  /** A line's clip; `slow` is its slow, clearly spoken version (stored apart). */
  speak: (text: string, gender?: string, slow = false) =>
    call(AudioClipSchema, "speak", { text, gender, slow }),
  /** `force` makes stored lines again too, replacing their clips. */
  cacheAudio: (lines: SpeechLine[], force = false) =>
    call(CacheProgressSchema, "cache_audio", { lines, force }),
  /** Downloads a session's audio in the background; returns at once. */
  prefetchAudio: (lines: SpeechLine[]) => run("prefetch_audio", { lines }),
  /** Makes one line's audio again, replacing the stored clip. */
  respeak: (text: string, gender: string) =>
    call(AudioClipSchema, "respeak", { text, gender }),
  audioOverview: () => call(AudioOverviewSchema, "audio_overview"),
  courseAudio: (courseId: CourseId) =>
    call(z.array(AudioItemSchema), "course_audio", { courseId }),
  /** Deletes clips no course uses; returns how many went. */
  deleteUnusedAudio: () => call(z.number(), "delete_unused_audio"),
  courseAudioLines: (courseId: CourseId) =>
    call(z.array(SpeechLineSchema), "course_audio_lines", { courseId }),
  audioCacheStats: () => call(AudioCacheStatsSchema, "audio_cache_stats"),
  clearAudioCache: () => run("clear_audio_cache"),

  kieSetKey: (key: string) => call(SettingsSchema, "kie_set_key", { key }),
  kieClearKey: () => call(SettingsSchema, "kie_clear_key"),
  kieCredits: () => call(z.number(), "kie_credits"),
  /** The saved API keys as a QR code (SVG markup) for another device to scan. */
  keysShareQr: () => call(z.string(), "keys_share_qr"),
  /** Saves the API keys from a QR code scanned off another device. */
  keysImport: (payload: string) =>
    call(SettingsSchema, "keys_import", { payload }),
  songsList: () => call(z.array(SongSchema), "songs_list"),
  songDraft: (request: DraftRequest) =>
    call(SongDraftSchema, "song_draft", { request }),
  songCreate: (draft: SongDraft) => call(SongSchema, "song_create", { draft }),
  songRetry: (songId: SongId) => call(SongSchema, "song_retry", { songId }),
  songDelete: (songId: SongId) => run("song_delete", { songId }),
  songTrackDelete: (trackId: TrackId) => run("song_track_delete", { trackId }),
  songTrackPlayed: (trackId: TrackId) => run("song_track_played", { trackId }),
  podcastTopics: () => call(z.array(TopicIdeaSchema), "podcast_topics"),
  podcastDraft: (request: PodcastRequest) =>
    call(SongDraftSchema, "podcast_draft", { request }),
  podcastCreate: (draft: SongDraft) =>
    call(SongSchema, "podcast_create", { draft }),
  /** Makes a whole episode in the background: written, then recorded. */
  podcastMake: (request: PodcastRequest) =>
    call(SongSchema, "podcast_make", { request }),
  songSources: (songId: SongId) =>
    call(z.array(WikiSourceSchema), "song_sources", { songId }),
  /** One of an episode's articles, kept now if it is not yet, for the reader. */
  songSourceOpen: (songId: SongId, lang: string, title: string) =>
    call(WikiSourceSchema, "song_source_open", { songId, lang, title }),
  wikiFind: (query: string) =>
    call(z.array(WikiHitSchema), "wiki_find", { query }),
  /** Puts an episode's audio together again, with new settings if given. */
  podcastRerender: (songId: SongId, audio?: PodcastAudio) =>
    call(SongSchema, "podcast_rerender", { songId, audio: audio ?? null }),
  songTrackLyrics: (trackId: TrackId) =>
    call(z.array(TimedLineSchema).nullable(), "song_track_lyrics", { trackId }),
  songGlossary: (songId: SongId) => call(z.array(GlossWordSchema), "song_glossary", { songId }),

  playlistsList: () => call(z.array(PlaylistSchema), "playlists_list"),
  playlistCreate: (name: string, trackIds: TrackId[] = []) =>
    call(PlaylistSchema, "playlist_create", { name, trackIds }),
  playlistRename: (playlistId: PlaylistId, name: string) =>
    call(PlaylistSchema, "playlist_rename", { playlistId, name }),
  playlistDelete: (playlistId: PlaylistId) =>
    run("playlist_delete", { playlistId }),
  playlistAdd: (playlistId: PlaylistId, trackIds: TrackId[]) =>
    call(PlaylistSchema, "playlist_add", { playlistId, trackIds }),
  playlistRemove: (playlistId: PlaylistId, entryId: EntryId) =>
    call(PlaylistSchema, "playlist_remove", { playlistId, entryId }),
  playlistMove: (playlistId: PlaylistId, entryId: EntryId, delta: number) =>
    call(PlaylistSchema, "playlist_move", { playlistId, entryId, delta }),
  /** Resolves to where the file went, or null if the dialog was cancelled. */
  songTrackSave: (trackId: TrackId) =>
    call(z.string().nullable(), "song_track_save", { trackId }),
  songTrackShareFile: (trackId: TrackId) =>
    call(z.string(), "song_track_share_file", { trackId }),

  castDiscover: (address?: string) =>
    call(z.array(CastDeviceSchema), "cast_discover", { address }),
  castStart: (
    device: CastDevice,
    trackIds: TrackId[],
    index: number,
    repeat: string,
  ) => call(CastStateSchema, "cast_start", { device, trackIds, index, repeat }),
  castCommand: (action: string, value?: number) =>
    run("cast_command", { action, value }),
  castSetRepeat: (repeat: string) => run("cast_set_repeat", { repeat }),
  castState: () => call(CastStateSchema.nullable(), "cast_state"),
  castControlInfo: () =>
    call(
      z.object({ port: z.number(), token: z.string() }),
      "cast_control_info",
    ),

  // Sync between the learner's devices (docs/SYNC.md).
  syncStatus: () => call(SyncStatusSchema, "sync_status"),
  syncSetName: (name: string) => run("sync_set_name", { name }),
  /** A pairing code and its QR, good for one device for ten minutes. */
  syncOffer: () => call(SyncOfferSchema, "sync_offer"),
  syncCancelOffer: () => run("sync_cancel_offer"),
  /**
   * Joins the vault behind a scanned code. A device holding data of its
   * own joins only with `replace` (a copy is saved first).
   */
  syncJoin: (code: string, replace: boolean) =>
    call(JoinResultSchema, "sync_join", { code, replace }),
  /** Syncs with the vault's devices now rather than in a few seconds. */
  syncNow: () => run("sync_now"),
  /** Removes a device from the vault: its key is never accepted again. */
  syncRemoveDevice: (id: DeviceId) => run("sync_remove_device", { id }),
  /** What of the learner's this device holds, table by table. */
  syncHeldData: () => call(z.array(SyncHeldSchema), "sync_held_data"),
};

/** Tauri rejects with a plain string built from our Error::Display impl. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}
