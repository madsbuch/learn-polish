import {
  Schedule,
  cancel,
  isPermissionGranted,
  pending,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

/** Fixed id so re-scheduling replaces the previous reminder instead of stacking. */
const REMINDER_ID = 42;

const NUDGES = [
  { title: "Czas na polski!", body: "One conversation, its words and its sentences. Ten minutes." },
  { title: "Twoja seria czeka", body: "Keep the streak alive — today's check-in is ready." },
  { title: "Dzień dobry!", body: "A new conversation is waiting for you in Polski." },
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
    const scheduled = await pending();
    const ids = scheduled.map((n) => n.id).filter((id) => id === REMINDER_ID);
    if (ids.length > 0) await cancel(ids);
  } catch {
    // Nothing scheduled, or the platform has no pending-notification query.
  }
}

/**
 * Schedules (or removes) the daily practice reminder. Returns whether a
 * reminder is now active, so Settings can report the truth rather than the
 * intent.
 */
export async function syncReminder(options: {
  enabled: boolean;
  hour: number;
  minute: number;
}): Promise<boolean> {
  await clearReminder();
  if (!options.enabled) return false;
  if (!(await ensurePermission())) return false;

  const nudge = NUDGES[Math.floor(Math.random() * NUDGES.length)];
  try {
    await sendNotification({
      id: REMINDER_ID,
      title: nudge.title,
      body: nudge.body,
      // An interval with only hour+minute set fires once every day at that time.
      schedule: Schedule.interval({ hour: options.hour, minute: options.minute }, true),
    });
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
