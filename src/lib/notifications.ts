import {
  Schedule,
  cancel,
  isPermissionGranted,
  pending,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { platform } from "@tauri-apps/plugin-os";
import { z } from "zod";

import { onEvent } from "./events";
import { toast } from "./toast";

/**
 * One reminder per day for the next week, each with its own id, so today's
 * can be left out once the day is done. Re-synced whenever the app opens or
 * the day's state changes, which also keeps the week topped up.
 */
const FIRST_ID = 42;
const DAYS_AHEAD = 7;

const GENERIC = [
  { title: "Czas na polski!", body: "A drill and a conversation — about ten minutes." },
  { title: "Dzień dobry!", body: "Your lessons are waiting in Polski." },
  { title: "Pięć minut?", body: "One short drill keeps the Polish warm." },
];

export async function ensurePermission(): Promise<boolean> {
  try {
    if (await isPermissionGranted()) return true;
    return (await requestPermission()) === "granted";
  } catch {
    // Desktop builds without a notification backend simply opt out.
    return false;
  }
}

async function clearReminder(): Promise<void> {
  try {
    const ours = new Set(Array.from({ length: DAYS_AHEAD + 1 }, (_, i) => FIRST_ID + i));
    const scheduled = await pending();
    const ids = scheduled.map((n) => n.id).filter((id) => ours.has(id));
    if (ids.length > 0) await cancel(ids);
  } catch {
    // Nothing scheduled, or the platform has no pending-notification query.
  }
}

/**
 * Schedules (or removes) the practice reminders. Today's is skipped once the
 * day is done (or its time has passed), and names what is due; the days
 * after get a general nudge. Returns whether reminders are now active, so
 * Settings can report the truth rather than the intent.
 */
export async function syncReminder(options: {
  enabled: boolean;
  hour: number;
  minute: number;
  /** Today's goal reached, or nothing left due. */
  doneToday?: boolean;
  /** Lessons due today. */
  due?: number;
  streak?: number;
}): Promise<boolean> {
  await clearReminder();
  if (!options.enabled) return false;
  if (!(await ensurePermission())) return false;

  const now = new Date();
  try {
    for (let offset = 0; offset < DAYS_AHEAD; offset++) {
      const at = new Date(now);
      at.setDate(now.getDate() + offset);
      at.setHours(options.hour, options.minute, 0, 0);
      if (offset === 0 && (options.doneToday || at <= now)) continue;
      const nudge =
        offset === 0 && (options.due ?? 0) > 0
          ? {
              title: options.streak ? `Your ${options.streak}-day streak is waiting` : "Czas na polski!",
              body: `${options.due} lesson${options.due === 1 ? " is" : "s are"} due today.`,
            }
          : GENERIC[(at.getDate() + offset) % GENERIC.length];
      await sendNotification({
        id: FIRST_ID + offset,
        title: nudge.title,
        body: nudge.body,
        schedule: Schedule.at(at, false, true),
      });
    }
    return true;
  } catch {
    return false;
  }
}

/** Immediate notification, used by the "test it" button in Settings. */
export async function sendTestNotification(): Promise<boolean> {
  if (!(await ensurePermission())) return false;
  try {
    await sendNotification({
      title: "Cześć! 👋",
      body: "Reminders are working. See you at practice time.",
    });
    return true;
  } catch {
    return false;
  }
}

const ASKED_FOR_WORK = "polski.notifications.askedForWork";

/**
 * The first time the app works in the background (the tutor writing, a
 * picture being drawn: `work://began`), notifications are asked for, once,
 * with why: on Android 13 and later the notification that keeps that work
 * alive with the screen off is hidden without them.
 */
export function askForNotificationsWhenWorking(): Promise<UnlistenFn> {
  return onEvent(z.null(), "work://began", () => {
    void (async () => {
      try {
        if (localStorage.getItem(ASKED_FOR_WORK)) return;
        localStorage.setItem(ASKED_FOR_WORK, "1");
      } catch {
        // No storage: ask anyway; the system asks only once by itself.
      }
      try {
        if (await isPermissionGranted()) return;
      } catch {
        return;
      }
      toast("Polski shows a quiet notification while it works in the background, so the work carries on with the screen off.", "info");
      await ensurePermission();
    })();
  });
}

const ASKED_FOR_DOWNLOADS = "polski.notifications.askedForDownloads";

/**
 * When the learner starts a course downloading (switching it to offline,
 * Download now), notifications are asked for on Android, once: the
 * download's progress shows in one, hidden on Android 13 and later without
 * them. The download runs either way. Elsewhere nothing: a computer
 * downloads while the app is open, with no notification.
 */
export async function askForNotificationsToFollowDownloads(): Promise<void> {
  try {
    if (platform() !== "android") return;
  } catch {
    return;
  }
  try {
    if (localStorage.getItem(ASKED_FOR_DOWNLOADS)) return;
    localStorage.setItem(ASKED_FOR_DOWNLOADS, "1");
    // Asked now: not again when the download's work begins.
    localStorage.setItem(ASKED_FOR_WORK, "1");
  } catch {
    // No storage: ask anyway; the system asks only once by itself.
  }
  try {
    if (await isPermissionGranted()) return;
  } catch {
    return;
  }
  toast("Allow notifications to follow the download, even with Polski closed.", "info");
  await ensurePermission();
}
