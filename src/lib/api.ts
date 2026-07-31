import { invoke } from "@tauri-apps/api/core";

import type {
  AiModel,
  AnswerResult,
  AudioCacheStats,
  AudioClip,
  CacheProgress,
  ChatMessage,
  Dialogue,
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

  getSession: () => invoke<SessionPlan>("get_session"),
  submitAnswer: (answer: {
    kind: string;
    itemKind: string;
    itemId: number;
    isNew: boolean;
    expected: string;
    given: string;
  }) => invoke<AnswerResult>("submit_answer", { answer }),
  completeSession: () => invoke<SessionResult>("complete_session"),

  getStats: () => invoke<Stats>("get_stats"),
  getTopicProgress: () => invoke<TopicProgress[]>("get_topic_progress"),
  getDialogue: (topicId: number) => invoke<Dialogue>("get_dialogue", { topicId }),
  browseWords: (topicId: number | null, query: string, limit = 300) =>
    invoke<Word[]>("browse_words", { topicId, query, limit }),
  browseSentences: (topicId: number) => invoke<Sentence[]>("browse_sentences", { topicId }),
  resetProgress: () => invoke<void>("reset_progress"),

  aiSetKey: (key: string) => invoke<Settings>("ai_set_key", { key }),
  aiClearKey: () => invoke<Settings>("ai_clear_key"),
  aiTestKey: () => invoke<string>("ai_test_key"),
  aiListModels: () => invoke<AiModel[]>("ai_list_models"),
  aiListSpeechModels: () => invoke<AiModel[]>("ai_list_speech_models"),

  speak: (text: string) => invoke<AudioClip>("speak", { text }),
  audioIsCached: (text: string) => invoke<boolean>("audio_is_cached", { text }),
  cacheAudio: (lines: string[]) => invoke<CacheProgress>("cache_audio", { lines }),
  topicAudioLines: (topicId: number) => invoke<string[]>("topic_audio_lines", { topicId }),
  audioCacheStats: () => invoke<AudioCacheStats>("audio_cache_stats"),
  clearAudioCache: () => invoke<void>("clear_audio_cache"),
  aiHistory: (topicId: number) => invoke<ChatMessage[]>("ai_history", { topicId }),
  aiClearChat: (topicId: number) => invoke<void>("ai_clear_chat", { topicId }),
  aiChat: (topicId: number, message: string) =>
    invoke<string>("ai_chat", { topicId, message }),
  aiExplain: (itemKind: string, itemId: number) =>
    invoke<string>("ai_explain", { itemKind, itemId }),
  aiFeedback: (target: string, attempt: string) =>
    invoke<string>("ai_feedback", { target, attempt }),
  aiDrill: () => invoke<string>("ai_drill"),
};

/** Tauri rejects with a plain string built from our Error::Display impl. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}
