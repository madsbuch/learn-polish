import { invoke } from "@tauri-apps/api/core";

import type {
  WordExample,
  MemoryCategory,
  MemoryOverview,
  CostSummary,
  AiModel,
  Lesson,
  LessonDetail,
  LessonFinished,
  LessonOverview,
  CheckinTurn,
  AnswerResult,
  AudioCacheStats,
  AudioItem,
  ArticleView,
  FactCheck,
  WikiSource,
  TutorStarted,
  TalkAbout,
  FeatureUsage,
  BenchRun,
  GoalHelp,
  AudioOverview,
  AudioClip,
  CacheProgress,
  CataloguePage,
  CastDevice,
  CastState,
  ChatMessage,
  Conversation,
  DraftRequest,
  Playlist,
  PodcastAudio,
  PodcastRequest,
  TopicIdea,
  Song,
  SongDraft,
  TimedLine,
  SpeechLine,
  Voice,
  Sentence,
  SessionPlan,
  SessionResult,
  Settings,
  SettingsPatch,
  Stats,
  CourseProgress,
  Word,
} from "./types";

/** Minutes to add to UTC to get local time — the backend needs it to know when
 *  "today" starts, and it can't determine that reliably by itself. */
function tzOffsetMinutes(): number {
  return -new Date().getTimezoneOffset();
}

export const api = {
  init: () => invoke<Settings>("init_app", { tzOffsetMinutes: tzOffsetMinutes() }),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (patch: SettingsPatch) => invoke<Settings>("update_settings", { patch }),


  submitAnswer: (answer: {
    kind: string;
    itemKind: string;
    itemId: number;
    isNew: boolean;
    expected: string;
    given: string;
    /** Extra practice (the lesson is not due): half XP. */
    extra?: boolean;
  }) => invoke<AnswerResult>("submit_answer", { answer }),
  lessonsOverview: () => invoke<LessonOverview>("lessons_overview"),
  lessonsForCourse: (courseId: number) => invoke<Lesson[]>("lessons_for_course", { courseId }),
  lessonDetail: (lessonId: number) => invoke<LessonDetail>("lesson_detail", { lessonId }),
  lessonSession: (lessonId: number) => invoke<SessionPlan>("lesson_session", { lessonId }),
  /** A drill is done: its score schedules the next check-in. */
  lessonFinish: (lessonId: number, correct: number, total: number) =>
    invoke<LessonFinished>("lesson_finish", { lessonId, correct, total }),
  lessonSetPeriod: (lessonId: number, periodDays: number) =>
    invoke<Lesson>("lesson_set_period", { lessonId, periodDays }),
  lessonDelete: (lessonId: number) => invoke<void>("lesson_delete", { lessonId }),
  /** A challenge: needed to master its courses, not to complete them. */
  lessonSetChallenge: (lessonId: number, challenge: boolean) =>
    invoke<Lesson>("lesson_set_challenge", { lessonId, challenge }),
  lessonAddToCourse: (lessonId: number, courseId: number) =>
    invoke<void>("lesson_add_to_course", { lessonId, courseId }),
  /** True when it was the lesson's last course, so the lesson was deleted. */
  lessonRemoveFromCourse: (lessonId: number, courseId: number) =>
    invoke<boolean>("lesson_remove_from_course", { lessonId, courseId }),
  /** The listening lesson for a song, made on first ask. */
  lessonForSong: (songId: number) => invoke<Lesson>("lesson_for_song", { songId }),
  checkinStart: (lessonId: number, streamId: string) =>
    invoke<CheckinTurn>("checkin_start", { lessonId, streamId }),
  checkinSend: (checkinId: number, message: string, streamId: string) =>
    invoke<CheckinTurn>("checkin_send", { checkinId, message, streamId }),
  /** Ends a writing conversation; the partner gives its verdict. */
  checkinEnd: (checkinId: number, streamId: string) =>
    invoke<CheckinTurn>("checkin_end", { checkinId, streamId }),
  /** A writing lesson's word bank (made once, then kept). */
  writingScaffold: (lessonId: number) => invoke<GoalHelp[]>("writing_scaffold", { lessonId }),
  /** A hint for the next message in a writing conversation. */
  writingHint: (checkinId: number) => invoke<string>("writing_hint", { checkinId }),
  /** Two sentences that use a word, shown after it is answered wrong. */
  wordExamples: (wordId: number) => invoke<WordExample[]>("word_examples", { wordId }),
  /** Adds audio players to a lesson's Polish examples (its text otherwise unchanged). */
  lessonAddPlayers: (lessonId: number) => invoke<LessonDetail>("lesson_add_players", { lessonId }),
  /** The summary after a check-in; new achievements are those since it began. */
  checkinSummary: (checkinId: number, firstCompletionToday: boolean) =>
    invoke<SessionResult>("checkin_summary", { checkinId, firstCompletionToday }),

  getStats: () => invoke<Stats>("get_stats"),
  getCourseProgress: () => invoke<CourseProgress[]>("get_course_progress"),
  browseWords: (courseId: number | null, query: string, limit = 300) =>
    invoke<Word[]>("browse_words", { courseId, query, limit }),
  browseCatalogue: (courseId: number | null, query: string, limit = 200) =>
    invoke<CataloguePage>("browse_catalogue", { courseId, query, limit }),
  browseSentences: (courseId: number) => invoke<Sentence[]>("browse_sentences", { courseId }),
  resetProgress: () => invoke<void>("reset_progress"),

  aiSetKey: (key: string) => invoke<Settings>("ai_set_key", { key }),
  aiClearKey: () => invoke<Settings>("ai_clear_key"),
  aiTestKey: () => invoke<string>("ai_test_key"),
  aiListModels: () => invoke<AiModel[]>("ai_list_models"),
  /** Tokens each feature used over the last 30 days. */
  aiUsage: () => invoke<FeatureUsage[]>("ai_usage"),
  /** Runs the benchmark on a model (progress: `bench://progress`). */
  benchRun: (model: string) => invoke<BenchRun>("bench_run", { model }),
  benchRuns: () => invoke<BenchRun[]>("bench_runs"),
  benchDelete: (id: number) => invoke<void>("bench_delete", { id }),
  /**
   * Opens the tutor conversation about a word or sentence — or carries on the
   * one already about it. A new one starts with the tutor talking.
   */
  tutorTalkAbout: (itemKind: string, itemId: number, polish?: string, meaning?: string) =>
    invoke<TalkAbout>("tutor_talk_about", { itemKind, itemId, polish: polish ?? null, meaning: meaning ?? null }),

  /**
   * Stores the message and starts the reply in the background; `conversationId`
   * null starts a new conversation. The reply streams under
   * `tutor-<conversation id>` and arrives as `tutor://done`.
   */
  tutorSend: (message: string, conversationId: number | null) =>
    invoke<TutorStarted>("tutor_send", { message, conversationId }),
  /** Asks again after a reply failed. */
  tutorRetry: (conversationId: number) => invoke<void>("tutor_retry", { conversationId }),
  /** Whether the app is on screen: off screen, finished replies notify. */
  appVisibility: (visible: boolean) => invoke<void>("app_visibility", { visible }),
  /** A study lesson's Wikipedia sources. */
  lessonSources: (lessonId: number) => invoke<WikiSource[]>("lesson_sources", { lessonId }),
  /** A source's article, in its own language or in Polish. */
  sourceArticle: (sourceId: number, polish: boolean) => invoke<ArticleView>("source_article", { sourceId, polish }),
  sourceRemove: (sourceId: number) => invoke<void>("source_remove", { sourceId }),
  /** Checks a lesson's lesson against its sources now. */
  factCheckLesson: (lessonId: number) => invoke<FactCheck>("fact_check_lesson", { lessonId }),
  /** Draws a new picture for a course; returns its file id. */
  coursePicture: (courseId: number) => invoke<number>("course_picture", { courseId }),
  coursePictureDelete: (courseId: number) => invoke<void>("course_picture_delete", { courseId }),
  tutorConversations: () => invoke<Conversation[]>("tutor_conversations"),
  tutorHistory: (conversationId: number) =>
    invoke<ChatMessage[]>("tutor_history", { conversationId }),
  tutorRename: (conversationId: number, title: string) =>
    invoke<void>("tutor_rename", { conversationId, title }),
  tutorDelete: (conversationId: number) => invoke<void>("tutor_delete", { conversationId }),
  courseDelete: (courseId: number) => invoke<void>("course_delete", { courseId }),
  /** Closes a mastered course: a trophy, out of the check-ins. Final after a few minutes. */
  courseClose: (courseId: number) => invoke<void>("course_close", { courseId }),
  courseUndoClose: (courseId: number) => invoke<void>("course_undo_close", { courseId }),
  courseRename: (courseId: number, title: string, titlePl: string, summary: string) =>
    invoke<void>("course_rename", { courseId, title, titlePl, summary }),
  /** Resolves to where the file went, or null if the dialog was cancelled. */
  exportDatabase: (includeKeys: boolean) =>
    invoke<string | null>("export_database", { includeKeys }),

  memoriesOverview: () => invoke<MemoryOverview>("memories_overview"),
  memoriesSetEnabled: (enabled: boolean) =>
    invoke<MemoryOverview>("memories_set_enabled", { enabled }),
  memoryAdd: (content: string, category: MemoryCategory) =>
    invoke<MemoryOverview>("memory_add", { content, category }),
  memoryUpdate: (id: number, content: string, category: MemoryCategory) =>
    invoke<MemoryOverview>("memory_update", { id, content, category }),
  memoryDelete: (id: number) => invoke<MemoryOverview>("memory_delete", { id }),
  memoriesClear: () => invoke<MemoryOverview>("memories_clear"),

  costsSummary: (days: number) => invoke<CostSummary>("costs_summary", { days }),

  listVoices: () => invoke<Voice[]>("list_voices"),
  previewVoice: (voice: string, text: string) =>
    invoke<AudioClip>("preview_voice", { voice, text }),
  /** A line's clip; `slow` is its slow, clearly spoken version (stored apart). */
  speak: (text: string, gender?: string, slow = false) => invoke<AudioClip>("speak", { text, gender, slow }),
  /** `force` makes stored lines again too, replacing their clips. */
  cacheAudio: (lines: SpeechLine[], force = false) => invoke<CacheProgress>("cache_audio", { lines, force }),
  /** Downloads a session's audio in the background; returns at once. */
  prefetchAudio: (lines: SpeechLine[]) => invoke<void>("prefetch_audio", { lines }),
  /** Makes one line's audio again, replacing the stored clip. */
  respeak: (text: string, gender: string) => invoke<AudioClip>("respeak", { text, gender }),
  audioOverview: () => invoke<AudioOverview>("audio_overview"),
  courseAudio: (courseId: number) => invoke<AudioItem[]>("course_audio", { courseId }),
  /** Deletes clips no course uses; returns how many went. */
  deleteUnusedAudio: () => invoke<number>("delete_unused_audio"),
  courseAudioLines: (courseId: number) => invoke<SpeechLine[]>("course_audio_lines", { courseId }),
  audioCacheStats: () => invoke<AudioCacheStats>("audio_cache_stats"),
  clearAudioCache: () => invoke<void>("clear_audio_cache"),

  kieSetKey: (key: string) => invoke<Settings>("kie_set_key", { key }),
  kieClearKey: () => invoke<Settings>("kie_clear_key"),
  kieCredits: () => invoke<number>("kie_credits"),
  /** The saved API keys as a QR code (SVG markup) for another device to scan. */
  keysShareQr: () => invoke<string>("keys_share_qr"),
  /** Saves the API keys from a QR code scanned off another device. */
  keysImport: (payload: string) => invoke<Settings>("keys_import", { payload }),
  songsList: () => invoke<Song[]>("songs_list"),
  songDraft: (request: DraftRequest) => invoke<SongDraft>("song_draft", { request }),
  songCreate: (draft: SongDraft) => invoke<Song>("song_create", { draft }),
  songRetry: (songId: number) => invoke<Song>("song_retry", { songId }),
  songDelete: (songId: number) => invoke<void>("song_delete", { songId }),
  songTrackDelete: (trackId: number) => invoke<void>("song_track_delete", { trackId }),
  songTrackPlayed: (trackId: number) => invoke<void>("song_track_played", { trackId }),
  podcastTopics: () => invoke<TopicIdea[]>("podcast_topics"),
  podcastDraft: (request: PodcastRequest) => invoke<SongDraft>("podcast_draft", { request }),
  podcastCreate: (draft: SongDraft) => invoke<Song>("podcast_create", { draft }),
  /** Puts an episode's audio together again, with new settings if given. */
  podcastRerender: (songId: number, audio?: PodcastAudio) =>
    invoke<Song>("podcast_rerender", { songId, audio: audio ?? null }),
  songTrackLyrics: (trackId: number) =>
    invoke<TimedLine[] | null>("song_track_lyrics", { trackId }),

  playlistsList: () => invoke<Playlist[]>("playlists_list"),
  playlistCreate: (name: string, trackIds: number[] = []) =>
    invoke<Playlist>("playlist_create", { name, trackIds }),
  playlistRename: (playlistId: number, name: string) =>
    invoke<Playlist>("playlist_rename", { playlistId, name }),
  playlistDelete: (playlistId: number) => invoke<void>("playlist_delete", { playlistId }),
  playlistAdd: (playlistId: number, trackIds: number[]) =>
    invoke<Playlist>("playlist_add", { playlistId, trackIds }),
  playlistRemove: (playlistId: number, entryId: number) =>
    invoke<Playlist>("playlist_remove", { playlistId, entryId }),
  playlistMove: (playlistId: number, entryId: number, delta: number) =>
    invoke<Playlist>("playlist_move", { playlistId, entryId, delta }),
  /** Resolves to where the file went, or null if the dialog was cancelled. */
  songTrackSave: (trackId: number) => invoke<string | null>("song_track_save", { trackId }),
  songTrackShareFile: (trackId: number) =>
    invoke<string>("song_track_share_file", { trackId }),

  castDiscover: (address?: string) => invoke<CastDevice[]>("cast_discover", { address }),
  castStart: (device: CastDevice, trackIds: number[], index: number, repeat: string) =>
    invoke<CastState>("cast_start", { device, trackIds, index, repeat }),
  castCommand: (action: string, value?: number) => invoke<void>("cast_command", { action, value }),
  castSetRepeat: (repeat: string) => invoke<void>("cast_set_repeat", { repeat }),
  castState: () => invoke<CastState | null>("cast_state"),
  castControlInfo: () => invoke<{ port: number; token: string }>("cast_control_info"),
};

/** Tauri rejects with a plain string built from our Error::Display impl. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}
