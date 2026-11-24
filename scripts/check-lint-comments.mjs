#!/usr/bin/env node
// A lint rule is not switched off in a comment: the only one a comment may
// turn off is react-hooks/exhaustive-deps, for an effect that runs on
// purpose when less than all it reads changes. Checked here rather than as a
// lint rule, which the comment it looks for could switch off too.
// Runs before every build: `bun run build` fails if a comment breaks it.

// Spec: keel.TYP-4

import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const SKIPPED = new Set(["node_modules", "dist", "target", "gen", ".git", ".gradle", "build"]);
const CODE = new Set([".ts", ".tsx", ".mts", ".cts", ".js", ".mjs", ".cjs"]);
const ALLOWED = "react-hooks/exhaustive-deps";
// A directive is a comment that starts with "eslint-disable…" or "eslint <rule>: …".
const DIRECTIVE = /(?:\/\/|\/\*)\s*(eslint-disable(?:-next-line|-line)?|eslint(?=\s+[\w@/-]+\s*:))([^\n]*?)(?:\*\/|$)/gm;

function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    if (entry.isDirectory()) return SKIPPED.has(entry.name) ? [] : files(join(dir, entry.name));
    return CODE.has(extname(entry.name)) ? [join(dir, entry.name)] : [];
  });
}

const problems = [];
for (const path of files(repo)) {
  const text = readFileSync(path, "utf8");
  for (const match of text.matchAll(DIRECTIVE)) {
    const [, kind, rest] = match;
    // What it names, without the "-- why" a directive can end with.
    const rules = rest.split("--")[0].split(",").map((rule) => rule.trim()).filter(Boolean);
    if (kind.startsWith("eslint-disable") && rules.length > 0 && rules.every((rule) => rule === ALLOWED)) continue;
    const line = text.slice(0, match.index).split("\n").length;
    problems.push(`${relative(repo, path)}:${line}  "${match[0].trim()}" switches lint rules off; only ${ALLOWED} may be`);
  }
}

if (problems.length > 0) {
  console.error(`Lint comments: ${problems.length} problem${problems.length === 1 ? "" : "s"}\n` + problems.map((p) => `  ${p}`).join("\n"));
  process.exit(1);
}
console.log("Lint comments: only exhaustive-deps is switched off in a comment.");
