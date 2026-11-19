import { Button, List, Screen, Section, SettingRow, Switch } from "../../design";
import { sendTestNotification } from "../../lib/notifications";
import { toast } from "../../lib/toast";
import { useSettingsActions, type SettingsPageProps } from "./actions";

/** Settings → Practice: how much a day, and the daily reminder. */
export function PracticeSettings({ settings, onSettings, onBack }: SettingsPageProps) {
  const { busy, patch, run } = useSettingsActions(onSettings);
  return (
    <Screen title="Practice" onBack={onBack} nav={false}>
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
