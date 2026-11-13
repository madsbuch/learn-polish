import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { Bar, Banner, Spinner } from "../components/ui";
import { api, errorMessage } from "../lib/api";
import { useThemePreference, type ThemePreference } from "../lib/theme";
import { sendTestNotification, syncReminder } from "../lib/notifications";
import { playClip } from "../lib/speech";
import type {
  AiModel,
  AudioCacheStats,
  CacheProgress,
  Settings,
  SettingsPatch,
  Voice,
} from "../lib/types";

/** Spoken when testing a voice, so you can hear whether it handles Polish. */
const VOICE_TEST_LINE = "Dzień dobry! Uczę się języka polskiego.";

function megabytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} kB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function SettingsScreen({
  settings,
  onSettings,
  onReset,
}: {
  settings: Settings;
  onSettings: (settings: Settings) => void;
  onReset: () => void;
}) {
  const [keyDraft, setKeyDraft] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [chatModels, setChatModels] = useState<AiModel[] | null>(null);
  const [voices, setVoices] = useState<Voice[] | null>(null);
  const [cache, setCache] = useState<AudioCacheStats | null>(null);
  const [progress, setProgress] = useState<CacheProgress | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);
  const [theme, setTheme] = useThemePreference();

  const refreshCache = useCallback(() => {
    api.audioCacheStats().then(setCache).catch(() => setCache(null));
  }, []);

  useEffect(refreshCache, [refreshCache, settings.ttsVoiceFemale, settings.ttsVoiceMale]);

  // The voice list ships with the app, so this never blocks on the network.
  useEffect(() => {
    api.listVoices().then(setVoices).catch(() => setVoices(null));
  }, []);

  // Bulk caching reports line-by-line so a 200-sentence run is not a black box.
  useEffect(() => {
    const unlisten = listen<CacheProgress>("tts://progress", (event) => {
      setProgress(event.payload);
      if (event.payload.finished) refreshCache();
    });
    return () => {
      void unlisten.then((off) => off());
    };
  }, [refreshCache]);

  async function patch(update: SettingsPatch, label: string) {
    setBusy(label);
    setError(null);
    try {
      const next = await api.updateSettings(update);
      onSettings(next);
      if (
        update.remindersEnabled !== undefined ||
        update.reminderHour !== undefined ||
        update.reminderMinute !== undefined
      ) {
        const active = await syncReminder({
          enabled: next.remindersEnabled,
          hour: next.reminderHour,
          minute: next.reminderMinute,
        });
        setNotice(
          next.remindersEnabled && !active
            ? "Notifications are switched off for this app in your system settings."
            : null,
        );
      }
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }

  async function run(label: string, action: () => Promise<void>) {
    setBusy(label);
    setError(null);
    setNotice(null);
    try {
      await action();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="scroll">
      <h1>Settings</h1>

      {error ? <Banner kind="error">{error}</Banner> : null}
      {notice ? <Banner kind="info">{notice}</Banner> : null}

      <div className="card">
        <h3>Appearance</h3>
        <div className="segmented">
          {(
            [
              ["system", "System"],
              ["light", "Light"],
              ["dark", "Dark"],
            ] as [ThemePreference, string][]
          ).map(([value, label]) => (
            <button key={value} data-active={theme === value} onClick={() => setTheme(value)}>
              {label}
            </button>
          ))}
        </div>
      </div>

      <div className="card">
        <h3>I am learning Polish from</h3>
        <div className="segmented">
          <button
            data-active={settings.sourceLanguage === "en"}
            onClick={() => patch({ sourceLanguage: "en" }, "lang")}
          >
            🇬🇧 English
          </button>
          <button
            data-active={settings.sourceLanguage === "da"}
            onClick={() => patch({ sourceLanguage: "da" }, "lang")}
          >
            🇩🇰 Dansk
          </button>
        </div>
        <p className="tiny faint" style={{ marginTop: 10, marginBottom: 0 }}>
          Every word, sentence and conversation is translated into both.
        </p>
      </div>

      <div className="card">
        <h3>Daily practice</h3>
        <label className="field">
          <span>Daily XP goal — {settings.dailyGoal} XP</span>
          <input
            type="range"
            min={20}
            max={300}
            step={10}
            value={settings.dailyGoal}
            onChange={(event) => patch({ dailyGoal: Number(event.target.value) }, "goal")}
          />
        </label>
        <label className="field" style={{ marginBottom: 0 }}>
          <span>New words per day — {settings.newWordsPerDay}</span>
          <input
            type="range"
            min={3}
            max={40}
            value={settings.newWordsPerDay}
            onChange={(event) =>
              patch({ newWordsPerDay: Number(event.target.value) }, "new-words")
            }
          />
        </label>
      </div>

      <div className="card">
        <div className="row between" style={{ marginBottom: 12 }}>
          <h3 style={{ margin: 0 }}>Daily reminder</h3>
          <button
            className={`btn small ${settings.remindersEnabled ? "success" : "ghost"}`}
            onClick={() => patch({ remindersEnabled: !settings.remindersEnabled }, "remind")}
          >
            {settings.remindersEnabled ? "On" : "Off"}
          </button>
        </div>
        <label className="field">
          <span>Remind me at</span>
          <input
            type="time"
            value={`${String(settings.reminderHour).padStart(2, "0")}:${String(
              settings.reminderMinute,
            ).padStart(2, "0")}`}
            onChange={(event) => {
              const [hour, minute] = event.target.value.split(":").map(Number);
              if (Number.isFinite(hour) && Number.isFinite(minute)) {
                patch({ reminderHour: hour, reminderMinute: minute }, "remind-time");
              }
            }}
          />
        </label>
        <button
          className="btn ghost block"
          disabled={busy === "test-note"}
          onClick={() =>
            run("test-note", async () => {
              const sent = await sendTestNotification();
              setNotice(
                sent
                  ? "Sent — check your notification shade."
                  : "This device did not allow the notification.",
              );
            })
          }
        >
          {busy === "test-note" ? <Spinner /> : "Send a test notification"}
        </button>
      </div>

      <div className="card">
        <h3>OpenRouter</h3>
        <p className="small">
          Powers both the AI tutor and the Polish voice. Requests are made from the app's
          Rust core, never from the web layer — so there is no CORS involved and your key
          stays out of JavaScript.
        </p>

        {settings.hasApiKey ? (
          <div className="row" style={{ marginBottom: 4 }}>
            <span className="chip mint">✓ Key saved</span>
            <span className="grow" />
            <button
              className="btn ghost small"
              disabled={busy === "test-key"}
              onClick={() =>
                run("test-key", async () => {
                  const reply = await api.aiTestKey();
                  setNotice(`Model replied: "${reply}" — everything works.`);
                })
              }
            >
              {busy === "test-key" ? <Spinner /> : "Test"}
            </button>
            <button
              className="btn danger small"
              onClick={() => run("clear-key", async () => onSettings(await api.aiClearKey()))}
            >
              Remove
            </button>
          </div>
        ) : (
          <>
            <label className="field">
              <span>API key</span>
              <input
                value={keyDraft}
                type="password"
                placeholder="sk-or-v1-…"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
                onChange={(event) => setKeyDraft(event.target.value)}
              />
            </label>
            <button
              className="btn primary block"
              disabled={keyDraft.trim().length === 0 || busy === "save-key"}
              onClick={() =>
                run("save-key", async () => {
                  onSettings(await api.aiSetKey(keyDraft));
                  setKeyDraft("");
                })
              }
            >
              {busy === "save-key" ? <Spinner /> : "Save key"}
            </button>
            <p className="tiny faint" style={{ marginTop: 10, marginBottom: 0 }}>
              Get one at openrouter.ai/keys. It is stored in the app's private database on
              this device.
            </p>
          </>
        )}
      </div>

      <div className="card">
        <h3>Tutor model</h3>
        {settings.aiModel ? (
          <p className="tiny faint" style={{ marginBottom: 10 }}>
            Using <b className="muted">{settings.aiModel}</b>
          </p>
        ) : (
          <Banner kind="info">
            No model chosen yet. Load the catalogue and pick one — the tutor stays off
            until you do.
          </Banner>
        )}

        {chatModels ? (
          <label className="field">
            <span>{chatModels.length} models available</span>
            <select
              value={settings.aiModel}
              onChange={(event) => patch({ aiModel: event.target.value }, "model")}
            >
              <option value="">— choose a model —</option>
              {chatModels.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.name}
                </option>
              ))}
            </select>
          </label>
        ) : null}

        <button
          className="btn ghost block"
          disabled={busy === "models"}
          onClick={() => run("models", async () => setChatModels(await api.aiListModels()))}
        >
          {busy === "models" ? <Spinner /> : "Load models from OpenRouter"}
        </button>
      </div>

      <div className="card">
        <h3>Polish voices</h3>
        <p className="small">
          Android's webview has no built-in speech engine, so the 🔊 buttons speak through
          Gemini. Dialogue lines are read in a voice matching the speaker. Each line is
          generated once and then stored on the device.
        </p>

        {(["f", "m"] as const).map((gender) => {
          const label = gender === "f" ? "Female voice" : "Male voice";
          const current = gender === "f" ? settings.ttsVoiceFemale : settings.ttsVoiceMale;
          const options = (voices ?? []).filter((voice) => voice.gender === gender);
          return (
            <label className="field" key={gender}>
              <span>{label}</span>
              <div className="row">
                <select
                  className="grow"
                  value={current}
                  disabled={options.length === 0}
                  onChange={(event) =>
                    patch(
                      gender === "f"
                        ? { ttsVoiceFemale: event.target.value }
                        : { ttsVoiceMale: event.target.value },
                      `voice-${gender}`,
                    )
                  }
                >
                  {options.length === 0 ? <option value={current}>{current}</option> : null}
                  {options.map((voice) => (
                    <option key={voice.name} value={voice.name}>
                      {voice.name} — {voice.description}
                    </option>
                  ))}
                </select>
                <button
                  className="btn ghost small"
                  disabled={busy === `preview-${gender}`}
                  onClick={() =>
                    run(`preview-${gender}`, async () => {
                      const clip = await api.previewVoice(current, VOICE_TEST_LINE);
                      await playClip(clip);
                    })
                  }
                >
                  {busy === `preview-${gender}` ? <Spinner /> : "▶︎"}
                </button>
              </div>
            </label>
          );
        })}

        <label className="field" style={{ marginBottom: 0 }}>
          <span>Speaking speed — {settings.ttsSpeed.toFixed(2)}×</span>
          <input
            type="range"
            min={0.5}
            max={1.5}
            step={0.05}
            value={settings.ttsSpeed}
            onChange={(event) => patch({ ttsSpeed: Number(event.target.value) }, "tts-speed")}
          />
        </label>
      </div>

      <div className="card">
          <h3>Downloaded audio</h3>
          {cache ? (
            <>
              <div className="row between small muted" style={{ marginBottom: 6 }}>
                <span>
                  {cache.sentencesCached} of {cache.sentencesTotal} sentences
                </span>
                <span>{megabytes(cache.bytes)}</span>
              </div>
              <Bar value={cache.sentencesCached} max={cache.sentencesTotal} tone="mint" />
              <p className="tiny faint" style={{ marginTop: 8 }}>
                {cache.clips} clips stored. Cached lines replay instantly and for free,
                even offline.
              </p>
            </>
          ) : null}

          {progress && !progress.finished ? (
            <>
              <Bar value={progress.done} max={Math.max(1, progress.total)} />
              <p className="tiny faint" style={{ marginTop: 8 }}>
                Generating {progress.done} of {progress.total}…
                {progress.failed > 0 ? ` ${progress.failed} failed.` : ""}
              </p>
            </>
          ) : null}

          <label className="field" style={{ marginTop: 12 }}>
            <span>
              Prefetch each session's audio in the background —{" "}
              {settings.ttsPrefetch ? "on" : "off"}
            </span>
            <button
              className={`btn small ${settings.ttsPrefetch ? "success" : "ghost"}`}
              onClick={() => patch({ ttsPrefetch: !settings.ttsPrefetch }, "prefetch")}
            >
              {settings.ttsPrefetch ? "On" : "Off"}
            </button>
          </label>

          <button
            className="btn ghost block"
            disabled={busy === "cache-all"}
            onClick={() =>
              run("cache-all", async () => {
                const lines = [];
                for (const topic of await api.getTopicProgress()) {
                  lines.push(...(await api.topicAudioLines(topic.topic.id)));
                }
                const result = await api.cacheAudio(lines);
                setNotice(
                  result.error
                    ? `Stored ${result.done}. Stopped after an error: ${result.error}`
                    : `Stored ${result.done} new clips.`,
                );
              })
            }
          >
            {busy === "cache-all" ? <Spinner /> : "⬇ Download every sentence and dialogue"}
          </button>

          <button
            className="btn danger block"
            style={{ marginTop: 8 }}
            disabled={busy === "cache-clear"}
            onClick={() =>
              run("cache-clear", async () => {
                await api.clearAudioCache();
                refreshCache();
                setNotice("Audio cache emptied.");
              })
            }
          >
            {busy === "cache-clear" ? <Spinner /> : "Delete downloaded audio"}
          </button>
      </div>

      <div className="card">
        <h3>Danger zone</h3>
        <p className="small">
          Clears every review, streak and achievement. The 2000 words, 200 sentences and 10
          conversations stay.
        </p>
        {confirmReset ? (
          <div className="row">
            <button
              className="btn danger grow"
              onClick={() =>
                run("reset", async () => {
                  await api.resetProgress();
                  setConfirmReset(false);
                  onReset();
                })
              }
            >
              {busy === "reset" ? <Spinner /> : "Yes, erase my progress"}
            </button>
            <button className="btn ghost" onClick={() => setConfirmReset(false)}>
              Cancel
            </button>
          </div>
        ) : (
          <button className="btn danger block" onClick={() => setConfirmReset(true)}>
            Reset progress
          </button>
        )}
      </div>

      <p className="center tiny faint">Polski · 2000 words · 200 sentences · 10 conversations</p>
    </div>
  );
}
