import { useCallback, useEffect, useState } from "react";

import { PlayerIcon } from "../../components/icons";
import { IconButton, Inline, List, ListItem, Screen, Section, SettingRow, Switch } from "../../design";
import { api } from "../../lib/api";
import { playClip } from "../../lib/speech";
import { playCorrect, setSoundsEnabled, soundsEnabled } from "../../lib/sounds";
import { canBuzz, hapticRight, setHapticsEnabled } from "../../lib/haptics";
import type { AudioCacheStats, Voice } from "../../lib/schemas";
import { megabytes } from "../AudioScreen";
import { useSettingsActions, type SettingsPageProps } from "./actions";

/** Spoken when testing a voice, so you can hear whether it handles Polish. */
const VOICE_TEST_LINE = "Dzień dobry! Uczę się języka polskiego.";

/** Settings → Voices & sound: the Polish voices and their speed, the audio kept, answer sounds. */
export function SoundSettings({
  settings,
  onSettings,
  onBack,
  onOpenAudio,
}: SettingsPageProps & {
  /** The downloaded-audio page. */
  onOpenAudio: () => void;
}) {
  const { busy, patch, run } = useSettingsActions(onSettings);
  const [sounds, setSounds] = useState(soundsEnabled);
  const [voices, setVoices] = useState<Voice[] | null>(null);
  const [cache, setCache] = useState<AudioCacheStats | null>(null);

  const refreshCache = useCallback(() => {
    api
      .audioCacheStats()
      .then(setCache)
      .catch(() => setCache(null));
  }, []);

  useEffect(refreshCache, [refreshCache, settings.ttsVoiceFemale, settings.ttsVoiceMale]);

  // The voice list ships with the app, so this never blocks on the network.
  useEffect(() => {
    api
      .listVoices()
      .then(setVoices)
      .catch(() => setVoices(null));
  }, []);

  const voiceName = (gender: "f" | "m") => (gender === "f" ? settings.ttsVoiceFemale : settings.ttsVoiceMale);

  return (
    <Screen title="Voices & sound" onBack={onBack} nav={false}>
      <Section
        title="Polish voices"
        info={
          <p>
            Android has no built-in Polish speech, so the speaker buttons use Gemini on OpenRouter. Dialogue lines use
            the voice matching the speaker. Each line is made once, then kept on the device.
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
            description="Every line plays at this speed, stored ones too. For one slow line, use 🐢 next to it."
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
          {canBuzz ? (
            <SettingRow
              label="Vibration"
              description="A tap when you're right, a buzz when you're not"
              control={
                <Switch
                  label="Vibration"
                  checked={settings.haptics}
                  onChange={(haptics) => {
                    patch({ haptics }, "haptics");
                    // Felt at once: what it will be like.
                    setHapticsEnabled(haptics);
                    if (haptics) hapticRight();
                  }}
                />
              }
            />
          ) : null}
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
    </Screen>
  );
}
