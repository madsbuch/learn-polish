import { invoke } from "@tauri-apps/api/core";

import type {
  AiModel,
  AnswerResult,
  AudioCacheStats,
  AudioClip,
  CacheProgress,
  CataloguePage,
  CastDevice,
  CastState,
  ChatMessage,
  Dialogue,
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

  /** Today's check-in, or a session for one topic or programme. */
  getSession: (topicId?: number) => invoke<SessionPlan>("get_session", { topicId }),
  submitAnswer: (answer: {
    kind: string;
    itemKind: string;
    itemId: number;
    isNew: boolean;
    expected: string;
    given: string;
  }) => invoke<AnswerResult>("submit_answer", { answer }),
  completeSession: (topicId: number) => invoke<SessionResult>("complete_session", { topicId }),

  getStats: () => invoke<Stats>("get_stats"),
  getTopicProgress: () => invoke<TopicProgress[]>("get_topic_progress"),
  getDialogue: (topicId: number) => invoke<Dialogue>("get_dialogue", { topicId }),
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

  tutorSend: (message: string, streamId: string) =>
    invoke<ChatMessage>("tutor_send", { message, streamId }),
  tutorHistory: () => invoke<ChatMessage[]>("tutor_history"),
  tutorClear: () => invoke<void>("tutor_clear"),
  programmeDelete: (topicId: number) => invoke<void>("programme_delete", { topicId }),
  setFocus: (topicId: number | null) => invoke<Settings>("set_focus", { topicId }),
  /** Resolves to where the file went, or null if the dialog was cancelled. */
  exportDatabase: (includeKeys: boolean) =>
    invoke<string | null>("export_database", { includeKeys }),

  listVoices: () => invoke<Voice[]>("list_voices"),
  previewVoice: (voice: string, text: string) =>
    invoke<AudioClip>("preview_voice", { voice, text }),
  speak: (text: string, gender?: string) => invoke<AudioClip>("speak", { text, gender }),
  audioIsCached: (text: string, gender?: string) =>
    invoke<boolean>("audio_is_cached", { text, gender }),
  cacheAudio: (lines: SpeechLine[]) => invoke<CacheProgress>("cache_audio", { lines }),
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
