// Apply the same app-specific Android resources locally and in CI.
import { cpSync, existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
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
writeFileSync(activities[0], activity);
writeFileSync(manifestPath, patched);
cpSync(join(root, "src-tauri/icons/android"), join(main, "res"), { recursive: true });
console.log("Prepared Polski's Android package, keyboard handling and launcher icons.");
