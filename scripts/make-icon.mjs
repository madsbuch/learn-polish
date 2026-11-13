// Regenerates every app icon from the vector logo in public/icon.svg.
//
//   node scripts/make-icon.mjs
//
// 1. Renders public/icon.svg to src-tauri/icons/source.png (1024x1024).
// 2. Runs `tauri icon` to derive the desktop, iOS and Android icons from it.
// 3. Replaces the Android adaptive-icon foregrounds with
//    scripts/icon-foreground.svg (the bubble alone, inside the safe zone) and
//    sets the matching background colour, so launchers that mask icons into
//    circles or squircles do not show a squircle-inside-a-circle.
//
// Rendering uses Playwright's Chromium, which draws the SVG gradients and
// shadows exactly as the browser shows the in-app logo.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const icons = resolve(root, "src-tauri/icons");

let chromium;
try {
  ({ chromium } = createRequire(import.meta.url)("playwright"));
} catch {
  console.error("Playwright is needed to render the SVG: `npm i -g playwright && npx playwright install chromium`.");
  process.exit(1);
}

const browser = await chromium.launch();
const page = await browser.newPage();

async function render(svgPath, size, outPath) {
  const svg = readFileSync(svgPath, "utf8").replace("<svg ", `<svg width="${size}" height="${size}" `);
  await page.setViewportSize({ width: size, height: size });
  await page.setContent(
    `<html><body style="margin:0;background:transparent">${svg}</body></html>`,
  );
  const png = await page.locator("svg").screenshot({ omitBackground: true });
  writeFileSync(outPath, png);
}

const source = resolve(icons, "source.png");
await render(resolve(root, "public/icon.svg"), 1024, source);
console.log("wrote", source);

execFileSync(resolve(root, "node_modules/.bin/tauri"), ["icon", source], {
  cwd: root,
  stdio: "inherit",
});

const densities = { mdpi: 108, hdpi: 162, xhdpi: 216, xxhdpi: 324, xxxhdpi: 432 };
for (const [density, size] of Object.entries(densities)) {
  await render(
    resolve(root, "scripts/icon-foreground.svg"),
    size,
    resolve(icons, `android/mipmap-${density}/ic_launcher_foreground.png`),
  );
}
writeFileSync(
  resolve(icons, "android/values/ic_launcher_background.xml"),
  `<?xml version="1.0" encoding="utf-8"?>
<resources>
  <color name="ic_launcher_background">#5236E0</color>
</resources>
`,
);
console.log("wrote Android adaptive foregrounds");

await browser.close();
