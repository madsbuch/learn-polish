import { useState } from "react";

import { Benchmark } from "../../components/Benchmark";
import { CostCenter } from "../../components/CostCenter";
import { ModelSetting } from "../../components/ModelPicker";
import { Button, Card, Field, Inline, List, ListItem, Screen, Section, SettingRow, Stack } from "../../design";
import { api } from "../../lib/api";
import { toast } from "../../lib/toast";
import { useSettingsActions, type SettingsPageProps } from "./actions";

/** Settings → AI: the OpenRouter key, the model for each job, the benchmark and what it all cost. */
export function AiSettings({ settings, onSettings, onBack }: SettingsPageProps) {
  const { busy, patch, run } = useSettingsActions(onSettings);
  const [keyDraft, setKeyDraft] = useState("");
  const [benchmark, setBenchmark] = useState(false);
  // Every model row shares the starred models.
  const models = {
    favorites: settings.favoriteModels ?? [],
    onFavorites: (favorites: string[]) => void patch({ favoriteModels: favorites }, "favorites"),
  };

  return (
    <Screen title="AI" onBack={onBack} nav={false}>
      <Section
        title="OpenRouter"
        info={
          <>
            <p>Powers the tutor, the songwriter and the Polish voice.</p>
            <p>
              Get a key at openrouter.ai/keys. It is stored in the app's private database on this device, and requests
              are made from the app's core — the key never reaches the web layer.
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
                        toast(`The model replied “${reply}” — everything works.`);
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
                wide
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
              <b>Tutor</b> answers in the Tutor tab. To build courses, lessons and songs it needs a model that can use
              tools — those are marked 🛠.
            </p>
            <p>
              <b>Songwriter</b> writes song lyrics. "Same as the tutor" reuses the tutor's model.
            </p>
            <p>
              <b>Writing partner</b> plays the other person in writing lessons. <b>Talk about it</b> chats with you
              about a word or sentence. Both are short replies, so a small, fast model is enough; pick one marked 🛠
              so it can still make a drill when you ask.
            </p>
          </>
        }
      >
        <List>
          <ModelSetting
            label="Tutor"
            job="tutor"
            value={settings.aiModel}
            need="tools"
            {...models}
            onChange={(model) => patch({ aiModel: model }, "model")}
          />
          <ModelSetting
            label="Songwriter"
            job="songwriter"
            value={settings.songwriterModel}
            fallback="Same as the tutor"
            {...models}
            onChange={(model) => patch({ songwriterModel: model }, "songwriter-model")}
          />
          <ModelSetting
            label="Writing partner"
            job="partner"
            value={settings.partnerModel}
            fallback="Same as the tutor"
            need="tools"
            {...models}
            onChange={(model) => patch({ partnerModel: model }, "partner-model")}
          />
          <ModelSetting
            label="Talk about it"
            job="word chat"
            value={settings.wordChatModel}
            fallback="Same as the tutor"
            need="tools"
            {...models}
            onChange={(model) => patch({ wordChatModel: model }, "word-chat-model")}
          />
          <ModelSetting
            label="Course pictures"
            job="pictures"
            value={settings.imageModel}
            fallback="Default (Gemini 2.5 Flash Image)"
            need="images"
            {...models}
            onChange={(model) => patch({ imageModel: model }, "image-model")}
          />
          <ListItem
            title="Benchmark"
            subtitle="Test a model on this app's own jobs, and compare scores, speed and price"
            onClick={() => setBenchmark(true)}
          />
        </List>
        {benchmark ? <Benchmark onClose={() => setBenchmark(false)} /> : null}
      </Section>

      <CostCenter />
    </Screen>
  );
}
