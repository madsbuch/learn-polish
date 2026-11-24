// Moving the API keys between devices with a QR code. The core renders the
// QR code and reads what was scanned; this side only runs the camera.

import { platform } from "@tauri-apps/plugin-os";
import {
  Format,
  cancel,
  checkPermissions,
  openAppSettings,
  requestPermissions,
  scan,
} from "@tauri-apps/plugin-barcode-scanner";

import { PermissionStateSchema, PlatformSchema, ScannedSchema } from "./schemas";

/** Phones have the scanner plugin; desktops only show their QR code. */
export const canScan = (() => {
  try {
    const os = PlatformSchema.parse(platform());
    return os === "android" || os === "ios";
  } catch {
    return false;
  }
})();

/** Asks for the camera if needed. False when the user said no. */
export async function cameraAllowed(): Promise<boolean> {
  if (PermissionStateSchema.parse(await checkPermissions()) === "granted") return true;
  return PermissionStateSchema.parse(await requestPermissions()) === "granted";
}

export { openAppSettings };

/**
 * Scans one QR code. The camera runs behind the web view, so the page turns
 * transparent (the `scanning` class on <html>) until the scan ends.
 * Resolves to null when cancelled.
 */
export async function scanQr(): Promise<string | null> {
  document.documentElement.classList.add("scanning");
  try {
    const result = ScannedSchema.parse(await scan({ windowed: true, formats: [Format.QRCode] }));
    return result.content;
  } catch (cause) {
    if (String(cause).includes("cancel")) return null;
    throw cause;
  } finally {
    document.documentElement.classList.remove("scanning");
  }
}

export async function cancelScan() {
  await cancel().catch(() => undefined);
}
