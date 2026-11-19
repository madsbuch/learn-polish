// Apply the same app-specific Android resources locally and in CI.
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const main = join(root, "src-tauri/gen/android/app/src/main");
if (!existsSync(main)) {
  throw new Error("Android project is missing. Run bun run android:init first.");
}
function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory() ? files(join(dir, entry.name)) : [join(dir, entry.name)],
  );
}
const activities = files(main).filter((path) => path.endsWith("/MainActivity.kt"));
if (activities.length !== 1) throw new Error(`Expected one MainActivity, found ${activities.length}`);
const activity = readFileSync(join(root, "src-tauri/android/MainActivity.kt"), "utf8");
const packageName = (source) => source.match(/^package\s+(\S+)/m)?.[1];
if (packageName(readFileSync(activities[0], "utf8")) !== packageName(activity)) {
  throw new Error("Generated Android package does not match Polski's MainActivity.");
}
const manifestPath = join(main, "AndroidManifest.xml");
const manifest = readFileSync(manifestPath, "utf8");
let matched = false;
const patched = manifest.replace(/<activity\b[^>]*>/g, (tag) => {
  if (!/android:name="(?:\.|com\.madsbuch\.learnpolish\.)MainActivity"/.test(tag)) return tag;
  matched = true;
  if (/android:windowSoftInputMode="[^"]*"/.test(tag)) {
    return tag.replace(/android:windowSoftInputMode="[^"]*"/, 'android:windowSoftInputMode="adjustResize"');
  }
  return tag.replace(/>$/, ' android:windowSoftInputMode="adjustResize">');
});
if (!matched) throw new Error("MainActivity was not found in the Android manifest.");
// Sync between the learner's devices talks to the local network: TCP and
// UDP broadcast (INTERNET), and whether Wi-Fi is up (ACCESS_NETWORK_STATE).
// Answers are felt as well as heard (src/lib/haptics.ts): VIBRATE, a normal
// permission granted at install. The haptics plugin's own manifest asks for
// it too; naming it here keeps it if that ever changes.
let permitted = patched;
for (const permission of [
  "android.permission.INTERNET",
  "android.permission.ACCESS_NETWORK_STATE",
  "android.permission.VIBRATE",
]) {
  if (permitted.includes(`android:name="${permission}"`)) continue;
  permitted = permitted.replace(/<application\b/, `<uses-permission android:name="${permission}" />\n    <application`);
}
// No Android backup, and no copying to a new phone, of the app's files.
// The database carries this device's sync identity (its device id, key and
// history: src-tauri/src/sync/). Brought back by Android onto a new phone,
// or onto this one after a reinstall, it would be an older copy of this
// device posing as it — what it wrote since would never reach the vault, and
// the vault's devices would take it for this one. A new phone joins the
// vault instead (Settings → Devices), which brings everything. Android 12+
// reads dataExtractionRules; older versions allowBackup and
// fullBackupContent. Applied whether or not the project was prepared before:
// each attribute is set to its value, never added twice.
const backupOff = {
  "android:allowBackup": "false",
  "android:fullBackupContent": "false",
  "android:dataExtractionRules": "@xml/data_extraction_rules",
};
let backupMatched = false;
const noBackup = permitted.replace(/<application\b[^>]*>/, (tag) => {
  backupMatched = true;
  let out = tag;
  for (const [name, value] of Object.entries(backupOff)) {
    const attribute = new RegExp(`${name}="[^"]*"`);
    out = attribute.test(out)
      ? out.replace(attribute, `${name}="${value}"`)
      : out.replace(/<application\b/, `<application\n        ${name}="${value}"`);
  }
  return out;
});
if (!backupMatched) throw new Error("The application element was not found in the Android manifest.");
const domains = ["root", "file", "database", "sharedpref", "external"];
const excluded = domains.map((domain) => `        <exclude domain="${domain}" path="." />`).join("\n");
const rules = `<?xml version="1.0" encoding="utf-8"?>
<!-- Nothing goes to cloud backup or device-to-device transfer: the database
     carries this device's sync identity. Sync between the learner's devices
     is the backup. Written by scripts/prepare-android.mjs, which says more. -->
<data-extraction-rules>
    <cloud-backup>
${excluded}
    </cloud-backup>
    <device-transfer>
${excluded}
    </device-transfer>
</data-extraction-rules>
`;
mkdirSync(join(main, "res/xml"), { recursive: true });
writeFileSync(join(main, "res/xml/data_extraction_rules.xml"), rules);
writeFileSync(activities[0], activity);
writeFileSync(manifestPath, noBackup);
cpSync(join(root, "src-tauri/icons/android"), join(main, "res"), { recursive: true });
console.log("Prepared Polski's Android package, keyboard handling, backup rules and launcher icons.");
