import { useState } from "react";

import { Banner, Button, Card, Field, Inline, List, Screen, Section, Segmented, SettingRow, Stack } from "../../design";
import { api } from "../../lib/api";
import { toast } from "../../lib/toast";
import { sunoLabel, useSettingsActions, type SettingsPageProps } from "./actions";

/** Settings → Songs: the kie.ai key that Suno sings through, and how songs are made. */
export function SongSettings({ settings, onSettings, onBack, onOpenPage }: SettingsPageProps) {
  const { busy, patch, run } = useSettingsActions(onSettings);
  const [kieDraft, setKieDraft] = useState("");

  return (
    <Screen
      title="Songs"
      onBack={onBack}
      nav={false}
      info={
        <>
          <p>Suno sings the learning songs, through your kie.ai account.</p>
          <p>Requests go from the app's core straight to kie.ai. Finished songs are kept on this device.</p>
        </>
      }
    >
      {!settings.hasApiKey ? (
        <Banner tone="info">
          The songwriter also needs your OpenRouter key.{" "}
          <Button variant="plain" onClick={() => onOpenPage("ai")}>
            Open AI settings
          </Button>
        </Banner>
      ) : null}
      <Section>
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
                        toast(`kie.ai works — ${credits} credits left.`);
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
                wide
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
    </Screen>
  );
}
