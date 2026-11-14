import { invoke } from "@tauri-apps/api/core";

import type {
  MemoryCategory,
  MemoryOverview,
  CostSummary,
  AiModel,
  Block,
  BlockDetail,
  BlockFinished,
  BlockOverview,
  CheckinTurn,
  AnswerResult,
  AudioCacheStats,
  AudioItem,
  TutorStarted,
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
  TopicProgress,
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
  }) => invoke<AnswerResult>("submit_answer", { answer }),
  blocksOverview: () => invoke<BlockOverview>("blocks_overview"),
  blocksForTopic: (topicId: number) => invoke<Block[]>("blocks_for_topic", { topicId }),
  blockDetail: (blockId: number) => invoke<BlockDetail>("block_detail", { blockId }),
  blockSession: (blockId: number) => invoke<SessionPlan>("block_session", { blockId }),
  /** A drill is done: its score schedules the next check-in. */
  blockFinish: (blockId: number, correct: number, total: number) =>
    invoke<BlockFinished>("block_finish", { blockId, correct, total }),
  blockSetPeriod: (blockId: number, periodDays: number) =>
    invoke<Block>("block_set_period", { blockId, periodDays }),
  blockDelete: (blockId: number) => invoke<void>("block_delete", { blockId }),
  blockAddToCourse: (blockId: number, topicId: number) =>
    invoke<void>("block_add_to_course", { blockId, topicId }),
  /** True when it was the block's last course, so the block was deleted. */
  blockRemoveFromCourse: (blockId: number, topicId: number) =>
    invoke<boolean>("block_remove_from_course", { blockId, topicId }),
  /** The listening block for a song, made on first ask. */
  blockForSong: (songId: number) => invoke<Block>("block_for_song", { songId }),
  checkinStart: (blockId: number, streamId: string) =>
    invoke<CheckinTurn>("checkin_start", { blockId, streamId }),
  checkinSend: (checkinId: number, message: string, streamId: string) =>
    invoke<CheckinTurn>("checkin_send", { checkinId, message, streamId }),
  /** Ends a writing conversation; the partner gives its verdict. */
  checkinEnd: (checkinId: number, streamId: string) =>
    invoke<CheckinTurn>("checkin_end", { checkinId, streamId }),
  checkinSummary: (firstCompletionToday: boolean) =>
    invoke<SessionResult>("checkin_summary", { firstCompletionToday }),

  getStats: () => invoke<Stats>("get_stats"),
  getTopicProgress: () => invoke<TopicProgress[]>("get_topic_progress"),
  browseWords: (topicId: number | null, query: string, limit = 300) =>
    invoke<Word[]>("browse_words", { topicId, query, limit }),
  browseCatalogue: (topicId: number | null, query: string, limit = 200) =>
    invoke<CataloguePage>("browse_catalogue", { topicId, query, limit }),
  browseSentences: (topicId: number) => invoke<Sentence[]>("browse_sentences", { topicId }),
  resetProgress: () => invoke<void>("reset_progress"),

  aiSetKey: (key: string) => invoke<Settings>("ai_set_key", { key }),
  aiClearKey: () => invoke<Settings>("ai_clear_key"),
  aiTestKey: () => invoke<string>("ai_test_key"),
  aiListModels: () => invoke<AiModel[]>("ai_list_models"),
  aiExplainStream: (itemKind: string, itemId: number, streamId: string) =>
    invoke<string>("ai_explain_stream", { itemKind, itemId, streamId }),

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
  /** Draws a new picture for a course; returns its path on disk. */
  coursePicture: (topicId: number) => invoke<string>("course_picture", { topicId }),
  coursePictureDelete: (topicId: number) => invoke<void>("course_picture_delete", { topicId }),
  tutorConversations: () => invoke<Conversation[]>("tutor_conversations"),
  tutorHistory: (conversationId: number) =>
    invoke<ChatMessage[]>("tutor_history", { conversationId }),
  tutorRename: (conversationId: number, title: string) =>
    invoke<void>("tutor_rename", { conversationId, title }),
  tutorDelete: (conversationId: number) => invoke<void>("tutor_delete", { conversationId }),
  courseDelete: (topicId: number) => invoke<void>("course_delete", { topicId }),
  courseRename: (topicId: number, title: string, titlePl: string, summary: string) =>
    invoke<void>("course_rename", { topicId, title, titlePl, summary }),
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
  speak: (text: string, gender?: string) => invoke<AudioClip>("speak", { text, gender }),
  /** `force` makes stored lines again too, replacing their clips. */
  cacheAudio: (lines: SpeechLine[], force = false) => invoke<CacheProgress>("cache_audio", { lines, force }),
  /** Makes one line's audio again, replacing the stored clip. */
  respeak: (text: string, gender: string) => invoke<AudioClip>("respeak", { text, gender }),
  audioOverview: () => invoke<AudioOverview>("audio_overview"),
  courseAudio: (topicId: number) => invoke<AudioItem[]>("course_audio", { topicId }),
  /** Deletes clips no course uses; returns how many went. */
  deleteUnusedAudio: () => invoke<number>("delete_unused_audio"),
  topicAudioLines: (topicId: number) => invoke<SpeechLine[]>("topic_audio_lines", { topicId }),
  audioCacheStats: () => invoke<AudioCacheStats>("audio_cache_stats"),
  clearAudioCache: () => invoke<void>("clear_audio_cache"),

  kieSetKey: (key: string) => invoke<Settings>("kie_set_key", { key }),
  kieClearKey: () => invoke<Settings>("kie_clear_key"),
  kieCredits: () => invoke<number>("kie_credits"),
  songsList: () => invoke<Song[]>("songs_list"),
  songDraft: (request: DraftRequest) => invoke<SongDraft>("song_draft", { request }),
  songCreate: (draft: SongDraft) => invoke<Song>("song_create", { draft }),
  songRetry: (songId: number) => invoke<Song>("song_retry", { songId }),
  songDelete: (songId: number) => invoke<void>("song_delete", { songId }),
  songTrackDelete: (trackId: number) => invoke<void>("song_track_delete", { trackId }),
  songTrackPlayed: (trackId: number) => invoke<void>("song_track_played", { trackId }),
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
