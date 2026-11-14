import { useCallback, useEffect, useState } from "react";

import { CostCenter } from "../components/CostCenter";
import { megabytes } from "./AudioScreen";
import { MemoryIcon, PlayerIcon } from "../components/icons";
import {
  Banner,
  Button,
  Card,
  ConfirmButton,
  Field,
  IconButton,
  Inline,
  List,
  ListItem,
  Screen,
  Section,
  Segmented,
  SettingRow,
  Stack,
  Switch,
  Text,
} from "../design";
import { api, errorMessage } from "../lib/api";
import { useThemePreference, type ThemePreference } from "../lib/theme";
import { sendTestNotification, syncReminder } from "../lib/notifications";
import { playClip } from "../lib/speech";
import { playCorrect, setSoundsEnabled, soundsEnabled } from "../lib/sounds";
import type {
  AiModel,
  AudioCacheStats,
  Settings,
  SettingsPatch,
  Voice,
} from "../lib/types";

/** Spoken when testing a voice, so you can hear whether it handles Polish. */
const VOICE_TEST_LINE = "Dzień dobry! Uczę się języka polskiego.";


export function SettingsScreen({
  settings,
  onSettings,
  onReset,
  onOpenMemories,
  onOpenAudio,
}: {
  settings: Settings;
  onSettings: (settings: Settings) => void;
  onReset: () => void;
  onOpenMemories: () => void;
  /** The downloaded-audio page. */
  onOpenAudio: () => void;
}) {
  const [keyDraft, setKeyDraft] = useState("");
  const [sounds, setSounds] = useState(soundsEnabled);
  const [kieDraft, setKieDraft] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [chatModels, setChatModels] = useState<AiModel[] | null>(null);
  const [voices, setVoices] = useState<Voice[] | null>(null);
  const [cache, setCache] = useState<AudioCacheStats | null>(null);
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

  const voiceName = (gender: "f" | "m") =>
    gender === "f" ? settings.ttsVoiceFemale : settings.ttsVoiceMale;
  const sunoLabel = (model: string) =>
    model === "V6_MINI" ? "V6 Mini" : model === "V6_WILD" ? "V6 Wild" : model;

  return (
    <Screen title="Settings">
      {error ? <Banner tone="error">{error}</Banner> : null}
      {notice ? <Banner tone="ok">{notice}</Banner> : null}

      <Section title="Practice">
        <List>
          <SettingRow
            label="Daily goal"
            value={`${settings.dailyGoal} XP`}
            stacked
            control={
              <input
                type="range"
                min={20}
                max={300}
                step={10}
                value={settings.dailyGoal}
                aria-label="Daily goal"
                onChange={(event) => patch({ dailyGoal: Number(event.target.value) }, "goal")}
              />
            }
          />
          <SettingRow
            label="Daily reminder"
            description={settings.remindersEnabled ? "A nudge if you have not practised yet" : "Off"}
            control={
              <Switch
                label="Daily reminder"
                checked={settings.remindersEnabled}
                onChange={(enabled) => patch({ remindersEnabled: enabled }, "remind")}
              />
            }
          />
          {settings.remindersEnabled ? (
            <SettingRow
              label="Remind me at"
              control={
                <div className="no-shrink">
                  <input
                    type="time"
                    aria-label="Reminder time"
                    value={`${String(settings.reminderHour).padStart(2, "0")}:${String(settings.reminderMinute).padStart(2, "0")}`}
                    onChange={(event) => {
                      const [hour, minute] = event.target.value.split(":").map(Number);
                      if (Number.isFinite(hour) && Number.isFinite(minute)) {
                        patch({ reminderHour: hour, reminderMinute: minute }, "remind-time");
                      }
                    }}
                  />
                </div>
              }
            />
          ) : null}
        </List>
        {settings.remindersEnabled ? (
          <Button
            variant="plain"
            size="sm"
            loading={busy === "test-note"}
            onClick={() =>
              run("test-note", async () => {
                const sent = await sendTestNotification();
                setNotice(sent ? "Sent — check your notifications." : "This device did not allow the notification.");
              })
            }
          >
            Send a test notification
          </Button>
        ) : null}
      </Section>

      <Section title="Appearance">
        <Segmented
          value={theme}
          onChange={(value) => setTheme(value as ThemePreference)}
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
        />
      </Section>

      <Section
        title="OpenRouter"
        info={
          <>
            <p>Powers the tutor, the songwriter and the Polish voice.</p>
            <p>
              Get a key at openrouter.ai/keys. It is stored in the app's private database on this
              device, and requests are made from the app's core — the key never reaches the web
              layer.
            </p>
          </>
        }
      >
        {settings.hasApiKey ? (
          <List>
            <SettingRow
              label="API key"
              description="Saved on this device"
              control={
                <Inline gap={1}>
                  <Button
                    variant="plain"
                    size="sm"
                    loading={busy === "test-key"}
                    onClick={() =>
                      run("test-key", async () => {
                        const reply = await api.aiTestKey();
                        setNotice(`The model replied “${reply}” — everything works.`);
                      })
                    }
                  >
                    Test
                  </Button>
                  <Button
                    variant="plain"
                    size="sm"
                    onClick={() => run("clear-key", async () => onSettings(await api.aiClearKey()))}
                  >
                    Remove
                  </Button>
                </Inline>
              }
            />
          </List>
        ) : (
          <Card>
            <Stack gap={3}>
              <Field label="API key">
                <input
                  value={keyDraft}
                  type="password"
                  placeholder="sk-or-v1-…"
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                  onChange={(event) => setKeyDraft(event.target.value)}
                />
              </Field>
              <Button
                variant="primary"
                block
                loading={busy === "save-key"}
                disabled={keyDraft.trim().length === 0}
                onClick={() =>
                  run("save-key", async () => {
                    onSettings(await api.aiSetKey(keyDraft));
                    setKeyDraft("");
                  })
                }
              >
                Save key
              </Button>
            </Stack>
          </Card>
        )}
      </Section>

      <Section
        title="AI models"
        info={
          <>
            <p>
              <b>Tutor</b> answers in the Tutor tab and explains words and sentences. To build
              courses, lessons and songs it needs a model that can use tools — those are marked 🛠.
            </p>
            <p>
              <b>Songwriter</b> writes song lyrics. "Same as the tutor" reuses the tutor's model.
            </p>
          </>
        }
      >
        <List>
          <SettingRow
            label="Tutor"
            description={settings.aiModel || "None chosen yet"}
            stacked={chatModels !== null}
            control={
              chatModels ? (
                <select
                  value={settings.aiModel}
                  aria-label="Tutor model"
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
              ) : undefined
            }
          />
          <SettingRow
            label="Songwriter"
            description={settings.songwriterModel || "Same as the tutor"}
            stacked={chatModels !== null}
            control={
              chatModels ? (
                <select
                  value={settings.songwriterModel}
                  aria-label="Songwriter model"
                  onChange={(event) => patch({ songwriterModel: event.target.value }, "songwriter-model")}
                >
                  <option value="">Same as the tutor</option>
                  {chatModels.map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.name}
                    </option>
                  ))}
                </select>
              ) : undefined
            }
          />
          <SettingRow
            label="Writing partner"
            description={
              settings.partnerModel || "Same as the tutor. A small, fast model marked 🛠 is enough here"
            }
            stacked={chatModels !== null}
            control={
              chatModels ? (
                <select
                  value={settings.partnerModel}
                  aria-label="Writing partner model"
                  onChange={(event) => patch({ partnerModel: event.target.value }, "partner-model")}
                >
                  <option value="">Same as the tutor</option>
                  {[...chatModels]
                    .sort((a, b) => Number(b.tools) - Number(a.tools))
                    .map((model) => (
                      <option key={model.id} value={model.id}>
                        {model.tools ? "🛠 " : ""}
                        {model.name}
                      </option>
                    ))}
                </select>
              ) : undefined
            }
          />
        </List>
        {settings.aiModel && chatModels?.some((model) => model.id === settings.aiModel && !model.tools) ? (
          <Banner tone="error">
            This model can't use tools, so the tutor won't be able to build courses or songs
            with it. Pick one marked 🛠.
          </Banner>
        ) : null}
        {chatModels === null ? (
          <Button
            block
            loading={busy === "models"}
            onClick={() => run("models", async () => setChatModels(await api.aiListModels()))}
          >
            Choose models
          </Button>
        ) : null}
      </Section>

      <Section
        title="Songs"
        info={
          <>
            <p>Suno sings the learning songs, through your kie.ai account.</p>
            <p>Requests go from the app's core straight to kie.ai. Finished songs are kept on this device.</p>
          </>
        }
      >
        {settings.hasKieKey ? (
          <List>
            <SettingRow
              label="kie.ai key"
              description="Saved on this device"
              control={
                <Inline gap={1}>
                  <Button
                    variant="plain"
                    size="sm"
                    loading={busy === "kie-credits"}
                    onClick={() =>
                      run("kie-credits", async () => {
                        const credits = await api.kieCredits();
                        setNotice(`kie.ai works — ${credits} credits left.`);
                      })
                    }
                  >
                    Credits
                  </Button>
                  <Button
                    variant="plain"
                    size="sm"
                    onClick={() => run("kie-clear", async () => onSettings(await api.kieClearKey()))}
                  >
                    Remove
                  </Button>
                </Inline>
              }
            />
            <SettingRow
              label="Song length"
              description="Suno sings as long as the lyrics are; this sets how much the songwriter writes."
              stacked
              control={
                <Segmented
                  value={settings.songLength}
                  onChange={(length) => patch({ songLength: length }, "song-length")}
                  options={[
                    { value: "short", label: "~2 min" },
                    { value: "medium", label: "~3 min" },
                    { value: "long", label: "4–5 min" },
                  ]}
                />
              }
            />
            <SettingRow
              label="Suno model"
              stacked
              control={
                <Segmented
                  value={settings.sunoModel}
                  onChange={(model) => patch({ sunoModel: model }, "suno-model")}
                  options={settings.sunoModels.map((model) => ({ value: model, label: sunoLabel(model) }))}
                />
              }
            />
          </List>
        ) : (
          <Card>
            <Stack gap={3}>
              <Field label="kie.ai API key">
                <input
                  value={kieDraft}
                  type="password"
                  placeholder="Paste your kie.ai key"
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                  onChange={(event) => setKieDraft(event.target.value)}
                />
              </Field>
              <Button
                variant="primary"
                block
                loading={busy === "kie-save"}
                disabled={kieDraft.trim().length === 0}
                onClick={() =>
                  run("kie-save", async () => {
                    onSettings(await api.kieSetKey(kieDraft));
                    setKieDraft("");
                  })
                }
              >
                Save key
              </Button>
            </Stack>
          </Card>
        )}
      </Section>

      <Section
        title="Polish voices"
        info={
          <p>
            Android has no built-in Polish speech, so the speaker buttons use Gemini on
            OpenRouter. Dialogue lines use the voice matching the speaker. Each line is made once,
            then kept on the device.
          </p>
        }
      >
        <List>
          {(["f", "m"] as const).map((gender) => {
            const current = voiceName(gender);
            const options = (voices ?? []).filter((voice) => voice.gender === gender);
            return (
              <SettingRow
                key={gender}
                label={gender === "f" ? "Female voice" : "Male voice"}
                stacked
                control={
                  <Inline gap={2}>
                    <select
                      value={current}
                      aria-label={gender === "f" ? "Female voice" : "Male voice"}
                      disabled={options.length === 0}
                      onChange={(event) =>
                        patch(
                          gender === "f" ? { ttsVoiceFemale: event.target.value } : { ttsVoiceMale: event.target.value },
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
                    <IconButton
                      variant="tinted"
                      icon={<PlayerIcon.play size={16} />}
                      label={`Hear ${current}`}
                      loading={busy === `preview-${gender}`}
                      onClick={() =>
                        run(`preview-${gender}`, async () => {
                          await playClip(await api.previewVoice(current, VOICE_TEST_LINE));
                        })
                      }
                    />
                  </Inline>
                }
              />
            );
          })}
          <SettingRow
            label="Speaking speed"
            value={`${settings.ttsSpeed.toFixed(2)}×`}
            stacked
            control={
              <input
                type="range"
                min={0.5}
                max={1.5}
                step={0.05}
                value={settings.ttsSpeed}
                aria-label="Speaking speed"
                onChange={(event) => patch({ ttsSpeed: Number(event.target.value) }, "tts-speed")}
              />
            }
          />
        </List>
      </Section>

      <Section title="Sound">
        <List>
          <ListItem
            leading={<PlayerIcon.download />}
            title="Downloaded audio"
            subtitle={
              cache
                ? `${cache.clips} clips · ${megabytes(cache.bytes)} — see what's stored, per course`
                : "See what's stored, per course"
            }
            onClick={onOpenAudio}
          />
          <SettingRow
            label="Answer sounds"
            description="A chime when you're right, a bump when you're not"
            control={
              <Switch
                label="Answer sounds"
                checked={sounds}
                onChange={(on) => {
                  setSoundsEnabled(on);
                  setSounds(on);
                  if (on) playCorrect();
                }}
              />
            }
          />
          <SettingRow
            label="Fetch a session's audio up front"
            description="So no line waits on the network"
            control={
              <Switch
                label="Fetch a session's audio up front"
                checked={settings.ttsPrefetch}
                onChange={(prefetch) => patch({ ttsPrefetch: prefetch }, "prefetch")}
              />
            }
          />
        </List>
      </Section>

      <Section title="Memories">
        <List>
          <ListItem
            leading={<MemoryIcon />}
            title="What the agents remember"
            subtitle="Goals, preferences and struggles the tutor and examiner noted"
            onClick={onOpenMemories}
          />
        </List>
      </Section>

      <CostCenter />

      <Section
        title="Your data"
        info={
          <p>
            Everything the app keeps — progress, courses, the tutor conversations, settings,
            downloaded audio and your songs — lives in one SQLite database on this device. Export
            saves a copy wherever you choose; it opens in any SQLite tool.
          </p>
        }
      >
        <List>
          <SettingRow
            label="Include API keys"
            description="Off keeps your keys out of the copy"
            control={<Switch label="Include API keys" checked={includeKeys} onChange={setIncludeKeys} />}
          />
        </List>
        <Button
          block
          icon={<PlayerIcon.download />}
          loading={busy === "export"}
          onClick={() =>
            run("export", async () => {
              const where = await api.exportDatabase(includeKeys);
              if (where) setNotice(`Exported to ${decodeURIComponent(where)}`);
            })
          }
        >
          Export database
        </Button>
      </Section>

      <Section title="Reset">
        <ConfirmButton
          label="Reset progress"
          confirmLabel="Erase progress"
          question="Erase every review, streak and achievement, the daily focus and the tutor conversation? Courses, memories and songs stay."
          block
          onConfirm={() =>
            run("reset", async () => {
              await api.resetProgress();
              onReset();
            })
          }
        />
      </Section>

      <Text variant="caption" tone="faint" align="center">
        Polski · learn Polish from English
      </Text>
    </Screen>
  );
}
