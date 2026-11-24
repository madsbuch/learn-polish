/**
 * Haptics: answers felt as well as heard. A soft tap for a right answer, a
 * firmer double buzz for a wrong one, a light tick under a pressed tile, and
 * a little drum roll when a lesson is aced or mastered.
 *
 * Phones only. The Tauri haptics plugin speaks to Android's vibrator and
 * iOS's feedback generators; where it isn't there (a computer, the browser
 * preview) nothing happens — except on Android, where `navigator.vibrate`
 * stands in should the plugin fail. Whether it buzzes at all is a setting of
 * this device (Settings → Voices & sound), set from App when the settings
 * load. Reduced motion doesn't turn it off: it is not motion, and has its own
 * switch.
 */

import { impactFeedback, notificationFeedback, selectionFeedback } from "@tauri-apps/plugin-haptics";
import { platform } from "@tauri-apps/plugin-os";

import { PlatformSchema } from "./schemas";

const os = (() => {
  try {
    return PlatformSchema.parse(platform());
  } catch {
    return "";
  }
})();

/** Only phones have something to buzz. */
export const canBuzz = os === "android" || os === "ios";

let enabled = true;

/** Follows the device setting (`haptics`). */
export function setHapticsEnabled(on: boolean) {
  enabled = on;
}

/** The web's vibration, in milliseconds: on, off, on… (Android's fallback). */
function webVibrate(pattern: number[]) {
  try {
    if (os === "android" && typeof navigator.vibrate === "function") navigator.vibrate(pattern);
  } catch {
    // Nothing to feel; nothing to say.
  }
}

/** Runs one plugin call; if it fails, the web pattern instead. A refused or
 *  missing vibrator is never an error worth showing: the answer still shows. */
function buzz(native: () => Promise<unknown>, fallback: number[]) {
  if (!enabled || !canBuzz) return;
  native().catch(() => webVibrate(fallback));
}

/** Waits a little, then buzzes: the beats of a pattern. */
function later(ms: number, run: () => void) {
  window.setTimeout(run, ms);
}

/** A right answer, a pair found, a tile snapped home: one soft tap. */
export function hapticRight() {
  buzz(() => impactFeedback("soft"), [18]);
}

/** Almost right: a single light tap, softer than wrong. */
export function hapticAlmost() {
  buzz(() => impactFeedback("light"), [12]);
}

/** A wrong answer: a firmer double buzz. */
export function hapticWrong() {
  buzz(() => notificationFeedback("error"), [40, 60, 40]);
  // Android's "error" is a single pulse; the second makes it a double.
  if (os === "android" && enabled) later(110, () => buzz(() => impactFeedback("medium"), []));
}

/** A primary button or a tile pressed: the lightest tick. */
export function hapticTick() {
  buzz(() => selectionFeedback(), [8]);
}

/** An ace, a lesson mastered, a record: a short rising roll. */
export function hapticCelebrate() {
  if (!enabled || !canBuzz) return;
  buzz(() => notificationFeedback("success"), [30, 50, 30, 50, 60]);
  later(140, () => buzz(() => impactFeedback("light"), []));
  later(260, () => buzz(() => impactFeedback("medium"), []));
  later(400, () => buzz(() => impactFeedback("heavy"), []));
}

