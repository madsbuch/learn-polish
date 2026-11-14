#!/usr/bin/env node
// Enforces the mechanical rules of the design system (see DESIGN.md), so a
// new screen cannot quietly drift back to one-off sizes and colours.
// Runs before every build: `bun run build` fails if a rule is broken.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const src = join(root, "src");
const problems = [];

function walk(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? walk(path) : [path];
  });
}

function report(file, line, message) {
  problems.push(`${relative(root, file)}:${line}  ${message}`);
}

function lineOf(text, index) {
  return text.slice(0, index).split("\n").length;
}

const files = walk(src);
const designDir = join(src, "design");

// 1. Screens and components style themselves only through the design system.
//    Inline styles may carry dynamic values (a width in %, a CSS variable),
//    never spacing, type or colour.
const STYLE_FORBIDDEN =
  /\b(margin\w*|padding\w*|gap|fontSize|fontWeight|lineHeight|letterSpacing|color|background\w*|border\w*|textAlign|display|opacity|boxShadow)\s*:/;
// Class names from before the design system. Use the components instead.
const RETIRED_CLASSES = new Set([
  "btn", "icon-btn", "chip", "row", "small", "tiny", "faint", "muted", "eyebrow",
  "scroll", "list-row", "stack-legacy", "segmented", "card", "banner", "hero", "section-title",
]);

for (const file of files.filter((f) => f.endsWith(".tsx") && !f.startsWith(designDir))) {
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(/style=\{\{([\s\S]*?)\}\}/g)) {
    const forbidden = match[1].match(STYLE_FORBIDDEN);
    if (forbidden) {
      report(file, lineOf(text, match.index), `inline style sets "${forbidden[1]}" — use a design component or a class built on tokens`);
    }
  }
  for (const match of text.matchAll(/className="([^"]+)"/g)) {
    for (const name of match[1].split(/\s+/)) {
      if (RETIRED_CLASSES.has(name)) {
        report(file, lineOf(text, match.index), `class "${name}" belongs to the design system — use its component`);
      }
    }
  }
  if (/<button[^>]*>\s*[‹›✕⌄]\s*<\/button>/.test(text)) {
    report(file, 1, "a button with a text glyph for an icon — use IconButton with an icon from components/icons");
  }
}

// 1b. Stored files reach the webview only through the files API, by id
//     (lib/files.ts → the media protocol), never by a path on disk.
for (const file of files.filter((f) => /\.tsx?$/.test(f) && !f.endsWith(join("lib", "files.ts")))) {
  const text = readFileSync(file, "utf8");
  const match = text.match(/\bconvertFileSrc\b/);
  if (match) {
    report(file, lineOf(text, match.index), "loads a file by path — use fileUrl(fileId) from lib/files.ts");
  }
}

// 2. Only tokens.css holds raw values: no hex colours, and no font sizes
//    off the type scale, anywhere else.
for (const file of files.filter((f) => f.endsWith(".css") && !f.endsWith("tokens.css"))) {
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(/#[0-9a-fA-F]{3,8}\b/g)) {
    report(file, lineOf(text, match.index), `raw colour ${match[0]} — add a token to src/design/tokens.css`);
  }
  for (const match of text.matchAll(/font-size:\s*([0-9.]+)px/g)) {
    report(file, lineOf(text, match.index), `font-size ${match[1]}px is off the type scale — use var(--font-…)`);
  }
}

if (problems.length > 0) {
  console.error(`Design check failed (${problems.length}):\n  ${problems.join("\n  ")}\n\nSee DESIGN.md.`);
  process.exit(1);
}
console.log("Design check passed.");
