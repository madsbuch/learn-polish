import { useState } from "react";

import { api, errorMessage } from "../../lib/api";
import { syncReminder } from "../../lib/notifications";
import { toast } from "../../lib/toast";
import type { Settings, SettingsPatch } from "../../lib/schemas";

/** The Settings sub-screens, each opened from a row of the Settings menu. */
export type { SettingsPage } from "../../lib/navigation";
import type { SettingsPage } from "../../lib/navigation";

/** What every Settings sub-screen is given. */
export interface SettingsPageProps {
  settings: Settings;
  onSettings: (settings: Settings) => void;
  /** Back to the Settings menu; absent beside it on a wide window. */
  onBack?: () => void;
  /** Opens another Settings sub-screen (a hint that points elsewhere). */
  onOpenPage: (page: SettingsPage) => void;
}

/**
 * Saving a setting, or running a button's action, with the label of what is
 * busy (for its spinner) and failures as a toast.
 */
export function useSettingsActions(onSettings: (settings: Settings) => void) {
  const [busy, setBusy] = useState<string | null>(null);

  async function patch(update: SettingsPatch, label: string) {
    setBusy(label);
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
        if (next.remindersEnabled && !active) {
          toast("Notifications are switched off for this app in your system settings.", "error");
        }
      }
    } catch (cause) {
      toast(errorMessage(cause), "error");
    } finally {
      setBusy(null);
    }
  }

  async function run(label: string, action: () => Promise<void>) {
    setBusy(label);
    try {
      await action();
    } catch (cause) {
      toast(errorMessage(cause), "error");
    } finally {
      setBusy(null);
    }
  }

  return { busy, patch, run };
}

/** "V6_MINI" → "V6 Mini". */
export const sunoLabel = (model: string) => (model === "V6_MINI" ? "V6 Mini" : model === "V6_WILD" ? "V6 Wild" : model);

/** A model's id as a short name: "anthropic/claude-sonnet-4.5" → "Claude Sonnet 4.5". */
export function modelName(id: string): string {
  const bare = id.slice(id.indexOf("/") + 1).split(":")[0];
  return bare
    .split("-")
    .map((word) => (word === "gpt" ? "GPT" : word.charAt(0).toUpperCase() + word.slice(1)))
    .join(" ");
}
