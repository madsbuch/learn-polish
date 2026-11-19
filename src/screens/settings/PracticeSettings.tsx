import { Button, List, Screen, Section, SettingRow, Switch } from "../../design";
import { canRecord } from "../../lib/mic";
import { sendTestNotification } from "../../lib/notifications";
import { toast } from "../../lib/toast";
import { useSettingsActions, type SettingsPageProps } from "./actions";

/** Settings → Practice: how much a day, speaking, and the daily reminder. */
export function PracticeSettings({ settings, onSettings, onBack }: SettingsPageProps) {
  const { busy, patch, run } = useSettingsActions(onSettings);
  return (
    <Screen
      title="Practice"
      onBack={onBack}
      nav={false}
      info={
        <>
          <p>
            <b>Speaking exercises</b>: now and then a lesson asks you to say a sentence, a word or your line in a
            dialogue out loud, and the AI writes down what it heard, word by word — with a tip when a sound came out
            as another. In a noisy place, tap “Skip speaking for now” in the lesson; this switch turns them off on
            this device altogether.
          </p>
          <p>
            Your recordings are sent to the model that hears you (Settings → AI) for that one request and are never
            kept: they stay in memory while the card is open, so you can play them back, and are not saved or synced.
          </p>
        </>
      }
    >
      <Section>
        <List>
          <SettingRow
            label="Lessons a day"
            description="The most important ones are chosen each morning. Do them and the day is done — for more, open a course"
            value={`${settings.dailyLessons}`}
            stacked
            control={
              <input
                type="range"
                min={1}
                max={10}
                step={1}
                value={settings.dailyLessons}
                aria-label="Lessons a day"
                onChange={(event) => patch({ dailyLessons: Number(event.target.value) }, "lessons")}
              />
            }
          />
          <SettingRow
            label="Speaking exercises"
            description={
              !canRecord()
                ? "This device can't record"
                : !settings.hasApiKey
                  ? "Needs an OpenRouter key (Settings → AI) to hear you"
                  : settings.speaking
                    ? "Now and then, say a sentence or a dialogue line out loud. On this device"
                    : "Off on this device"
            }
            control={
              <Switch
                label="Speaking exercises"
                checked={settings.speaking}
                onChange={(on) => patch({ speaking: on }, "speaking")}
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
                    value={reminderTime(settings.reminderHour, settings.reminderMinute)}
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
                toast(sent ? "Sent — check your notifications." : "This device did not allow the notification.");
              })
            }
          >
            Send a test notification
          </Button>
        ) : null}
      </Section>
    </Screen>
  );
}

/** "19:05". */
export function reminderTime(hour: number, minute: number): string {
  return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
}
