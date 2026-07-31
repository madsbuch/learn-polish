import { useEffect, useState } from "react";

import { Banner, Spinner } from "../components/ui";
import { api, errorMessage } from "../lib/api";
import { sendTestNotification, syncReminder } from "../lib/notifications";
import type { AiModel, Settings, SettingsPatch } from "../lib/types";

const POPULAR_MODELS = [
  "anthropic/claude-3.5-haiku",
  "anthropic/claude-sonnet-4.5",
  "openai/gpt-4.1-mini",
  "google/gemini-2.5-flash",
  "meta-llama/llama-3.3-70b-instruct",
];

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
  const [models, setModels] = useState<AiModel[] | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);

  useEffect(() => {
    setNotice(null);
  }, [settings]);

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
        <h3>AI tutor (OpenRouter)</h3>
        <p className="small">
          Requests are made from the app's Rust core, never from the web layer — so there
          is no CORS involved and your key stays out of JavaScript.
        </p>

        {settings.hasApiKey ? (
          <>
            <div className="row" style={{ marginBottom: 12 }}>
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

            <label className="field">
              <span>Model</span>
              <select
                value={settings.aiModel}
                onChange={(event) => patch({ aiModel: event.target.value }, "model")}
              >
                {(models ?? []).length === 0
                  ? [...new Set([settings.aiModel, ...POPULAR_MODELS])].map((id) => (
                      <option key={id} value={id}>
                        {id}
                      </option>
                    ))
                  : models!.map((model) => (
                      <option key={model.id} value={model.id}>
                        {model.name}
                      </option>
                    ))}
              </select>
            </label>
            <button
              className="btn ghost block"
              disabled={busy === "models"}
              onClick={() => run("models", async () => setModels(await api.aiListModels()))}
            >
              {busy === "models" ? <Spinner /> : "Load all OpenRouter models"}
            </button>
          </>
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
