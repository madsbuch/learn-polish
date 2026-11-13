import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { TitleWithInfo } from "../components/Info";
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
  const [kieDraft, setKieDraft] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [chatModels, setChatModels] = useState<AiModel[] | null>(null);
  const [voices, setVoices] = useState<Voice[] | null>(null);
  const [cache, setCache] = useState<AudioCacheStats | null>(null);
  const [progress, setProgress] = useState<CacheProgress | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);
  const [includeKeys, setIncludeKeys] = useState(false);
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
        <TitleWithInfo
          title="OpenRouter"
          info={
            <>
              <p>Powers the AI tutor, the songwriter and the Polish voice.</p>
              <p>Get a key at openrouter.ai/keys. It is stored in the app's private database on this device, and requests are made from the app's core — the key never reaches the web layer.</p>
            </>
          }
        />

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
          </>
        )}
      </div>

      <div className="card">
        <TitleWithInfo
          title="AI models"
          info={
            <>
              <p><b>Tutor</b> answers in the Tutor tab and explains words and sentences. To build programmes and change things in the app it needs a model that can use tools — those are marked 🛠.</p>
              <p><b>Songwriter</b> writes song lyrics. "Same as the tutor" reuses the tutor's model.</p>
              <p>Models come from OpenRouter's live catalogue, so nothing goes stale.</p>
            </>
          }
        />
        {settings.aiModel ? (
          <p className="tiny faint" style={{ marginBottom: 10 }}>
            Using <b className="muted">{settings.aiModel}</b>
          </p>
        ) : (
          <Banner kind="info">
            Load the models and pick one.
          </Banner>
        )}

        {settings.songwriterModel ? (
          <p className="tiny faint" style={{ marginBottom: 10 }}>
            Songwriter: <b className="muted">{settings.songwriterModel}</b>
          </p>
        ) : null}

        {chatModels ? (
          <>
            <label className="field">
              <span>
                Tutor — {chatModels.filter((model) => model.tools).length} of {chatModels.length}{" "}
                models can use tools 🛠
              </span>
              <select
                value={settings.aiModel}
                onChange={(event) => patch({ aiModel: event.target.value }, "model")}
              >
                <option value="">— choose a model —</option>
                {[...chatModels]
                  .sort((a, b) => Number(b.tools) - Number(a.tools))
                  .map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.tools ? "🛠 " : ""}
                      {model.name}
                    </option>
                  ))}
              </select>
            </label>
            {settings.aiModel &&
            chatModels.some((model) => model.id === settings.aiModel && !model.tools) ? (
              <Banner kind="error">
                This model can't use tools, so the tutor won't be able to build programmes
                with it. Pick one marked 🛠.
              </Banner>
            ) : null}
            <label className="field">
              <span>Songwriter — writes the song lyrics</span>
              <select
                value={settings.songwriterModel}
                onChange={(event) =>
                  patch({ songwriterModel: event.target.value }, "songwriter-model")
                }
              >
                <option value="">Same as the tutor</option>
                {chatModels.map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.name}
                  </option>
                ))}
              </select>
            </label>
          </>
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
        <TitleWithInfo
          title="Songs — Suno via kie.ai"
          infoTitle="Suno"
          info={
            <>
              <p>Suno sings the learning songs, through your kie.ai account.</p>
              <p>Requests go from the app's core straight to kie.ai. Finished songs are downloaded and kept on this device.</p>
            </>
          }
        />

        {settings.hasKieKey ? (
          <div className="row" style={{ marginBottom: 12 }}>
            <span className="chip mint">✓ Key saved</span>
            <span className="grow" />
            <button
              className="btn ghost small"
              disabled={busy === "kie-credits"}
              onClick={() =>
                run("kie-credits", async () => {
                  const credits = await api.kieCredits();
                  setNotice(`kie.ai works — ${credits} credits left.`);
                })
              }
            >
              {busy === "kie-credits" ? <Spinner /> : "Check credits"}
            </button>
            <button
              className="btn danger small"
              onClick={() => run("kie-clear", async () => onSettings(await api.kieClearKey()))}
            >
              Remove
            </button>
          </div>
        ) : (
          <>
            <label className="field">
              <span>kie.ai API key</span>
              <input
                value={kieDraft}
                type="password"
                placeholder="Paste your kie.ai key"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
                onChange={(event) => setKieDraft(event.target.value)}
              />
            </label>
            <button
              className="btn primary block"
              style={{ marginBottom: 12 }}
              disabled={kieDraft.trim().length === 0 || busy === "kie-save"}
              onClick={() =>
                run("kie-save", async () => {
                  onSettings(await api.kieSetKey(kieDraft));
                  setKieDraft("");
                })
              }
            >
              {busy === "kie-save" ? <Spinner /> : "Save key"}
            </button>
          </>
        )}

        <label className="field" style={{ marginBottom: 0 }}>
          <span>Suno model</span>
          <div className="segmented">
            {settings.sunoModels.map((model) => (
              <button
                key={model}
                data-active={settings.sunoModel === model}
                onClick={() => patch({ sunoModel: model }, "suno-model")}
              >
                {model === "V6" ? "V6" : model === "V6_MINI" ? "V6 Mini" : model === "V6_WILD" ? "V6 Wild" : model}
              </button>
            ))}
          </div>
        </label>
      </div>

      <div className="card">
        <TitleWithInfo
          title="Polish voices"
          info={
            <>
              <p>Android has no built-in Polish speech, so the 🔊 buttons speak through Gemini on OpenRouter.</p>
              <p>Dialogue lines use the voice matching the speaker. Each line is generated once, then stored on the device.</p>
            </>
          }
        />

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
          <TitleWithInfo
            title="Downloaded audio"
            info={<p>Stored lines replay instantly and for free, even offline. Downloading everything ahead of time means practice never waits on the network.</p>}
          />
          {cache ? (
            <>
              <div className="row between small muted" style={{ marginBottom: 6 }}>
                <span>
                  {cache.sentencesCached} of {cache.sentencesTotal} sentences
                </span>
                <span>{megabytes(cache.bytes)}</span>
              </div>
              <Bar value={cache.sentencesCached} max={cache.sentencesTotal} tone="mint" />
              <p className="tiny faint" style={{ marginTop: 8 }}>{cache.clips} clips</p>
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
            <span>Prefetch session audio</span>
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
        <TitleWithInfo
          title="Your data"
          info={
            <>
              <p>Everything the app keeps — progress, programmes, the tutor conversation, settings, downloaded audio and your songs — lives in one SQLite database on this device.</p>
              <p>Export saves a copy of it wherever you choose. It opens in any SQLite tool. API keys are left out unless you include them.</p>
            </>
          }
        />
        <label className="field">
          <span>Include API keys</span>
          <button
            className={`btn small ${includeKeys ? "success" : "ghost"}`}
            onClick={() => setIncludeKeys((value) => !value)}
          >
            {includeKeys ? "Yes" : "No"}
          </button>
        </label>
        <button
          className="btn ghost block"
          disabled={busy === "export"}
          onClick={() =>
            run("export", async () => {
              const where = await api.exportDatabase(includeKeys);
              if (where) setNotice(`Exported to ${decodeURIComponent(where)}`);
            })
          }
        >
          {busy === "export" ? <Spinner /> : "⬇ Export database (.sqlite)"}
        </button>
      </div>

      <div className="card">
        <TitleWithInfo
          title="Danger zone"
          info={<p>Reset clears every review, streak, achievement, the daily focus and the tutor conversation. The words, sentences, conversations, your programmes and your songs stay.</p>}
        />
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

      <p className="center tiny faint">Polski · learn Polish from English</p>
    </div>
  );
}
