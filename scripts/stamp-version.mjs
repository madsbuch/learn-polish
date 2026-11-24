// Gives a CI build its own version, so a phone (or an updater) sees every
// build as newer than the last: 0.<1 + n/1000>.<n % 1000> for build number n
// (GITHUB_RUN_NUMBER). Android derives its versionCode from the version
// (major·1 000 000 + minor·1 000 + patch), so it only ever goes up.
// Locally (no build number) the version is left as it is.
import { writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const run = Number(process.env.GITHUB_RUN_NUMBER);
if (!Number.isInteger(run) || run <= 0) {
  console.log("No build number: the version stays as it is.");
  process.exit(0);
}
const version = `0.${1 + Math.floor(run / 1000)}.${run % 1000}`;
const path = new URL("../src-tauri/tauri.conf.json", import.meta.url);
// Read by `require`: JSON.parse is the app's boundary alone (src/lib/schemas.ts).
const config = createRequire(import.meta.url)(fileURLToPath(path));
config.version = version;
writeFileSync(path, JSON.stringify(config, null, 2) + "\n");
console.log(`Version ${version} (build ${run}).`);
