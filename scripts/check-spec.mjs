// Checks the spec (.spec/) against the rules of its format:
// https://raw.githubusercontent.com/madsbuch/better-github/refs/heads/main/spec-llms.txt
//
// - every file is valid JSON with only the keys the schema knows;
// - every requirement's id is unique in the whole spec, and a prefix belongs
//   to one item ("MAST-7" is in Mastery and nowhere else);
// - every reference (`related`, `replacedBy`) points at an item or a
//   requirement that exists;
// - a deprecated requirement or item says why (`deprecation`), and only a
//   deprecated one has a `deprecation`;
// - a requirement above version 1 has the history of all its earlier versions;
// - every `code` path or pattern matches a file, and every figure is there.
//
// Runs in `bun run build`. An id never changes and is never reused: a new
// requirement takes its item's next number, and one no longer wanted is
// deprecated, not deleted.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const root = join(repo, ".spec");

const ITEM_KEYS = ["$schema", "title", "summary", "status", "deprecation", "order", "description", "requirements", "figures", "questions", "code", "related"];
const REQUIREMENT_KEYS = ["id", "text", "level", "status", "version", "history", "deprecation"];
const STATUSES = ["draft", "accepted", "implemented", "deprecated"];
const LEVELS = ["must", "should", "may"];
/** Folders no `code` pattern is meant to reach. */
const SKIPPED = new Set([".git", "node_modules", "target", "dist", "gen"]);

function files(dir, wanted) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory() ? (SKIPPED.has(entry.name) ? [] : files(join(dir, entry.name), wanted)) : wanted(entry.name) ? [join(dir, entry.name)] : [],
  );
}

/** An item's id: its path without ".json"; a folder's own item is its index. */
function idOf(path) {
  const id = relative(root, path).replaceAll("\\", "/").replace(/\.json$/, "");
  return id === "index" ? "" : id.replace(/\/index$/, "");
}

const problems = [];
const items = new Map();
for (const path of files(root, (name) => name.endsWith(".json"))) {
  try {
    items.set(idOf(path), JSON.parse(readFileSync(path, "utf8")));
  } catch (error) {
    problems.push(`${relative(root, path)}: not valid JSON (${error.message})`);
  }
}
const name = (id) => id || "(root)";

// Where each requirement id is used.
const owners = new Map();
for (const [id, item] of items) {
  for (const requirement of item.requirements ?? []) {
    owners.set(requirement.id, [...(owners.get(requirement.id) ?? []), id]);
  }
}
for (const [code, where] of owners) {
  if (where.length > 1) problems.push(`requirement id ${code} is used ${where.length} times: ${where.map(name).join(", ")}`);
}

// A prefix belongs to one item, so a new requirement can't take an id
// another item will want.
const prefixes = new Map();
for (const [id, item] of items) {
  for (const requirement of item.requirements ?? []) {
    const prefix = String(requirement.id).replace(/-?\d+$/, "");
    const owner = prefixes.get(prefix);
    if (owner === undefined) prefixes.set(prefix, id);
    else if (owner !== id) problems.push(`prefix ${prefix} is used by two items: ${name(owner)} and ${name(id)}`);
  }
}

/** Whether a reference names something: "item", "item#ID", "#ID", each with an optional "@version". */
function resolves(reference) {
  const [target, rest] = reference.split("#");
  if (rest === undefined) return items.has(target);
  const code = rest.split("@")[0];
  const where = owners.get(code);
  return where !== undefined && (target === "" || where.includes(target));
}

function checkDeprecation(where, thing, inherited) {
  const deprecated = thing.status === "deprecated";
  if (deprecated && !thing.deprecation?.reason && !inherited) problems.push(`${where}: deprecated without a deprecation that says why`);
  if (!deprecated && thing.deprecation) problems.push(`${where}: has a deprecation but is not deprecated`);
  for (const reference of thing.deprecation?.replacedBy ?? []) {
    if (!resolves(reference)) problems.push(`${where}: replacedBy "${reference}" does not exist`);
  }
}

/** A `code` entry as a pattern over paths from the repository's root. */
function pattern(entry) {
  const source = entry
    .split(/(\*\*|\*)/)
    .map((part) => (part === "**" ? ".*" : part === "*" ? "[^/]*" : part.replace(/[.+?^${}()|[\]\\]/g, "\\$&")))
    .join("");
  return new RegExp(`^${source}$`);
}

let everyFile = null;
function matchesAFile(entry) {
  if (!entry.includes("*")) return existsSync(join(repo, entry)) && statSync(join(repo, entry)).isFile();
  everyFile ??= files(repo, () => true).map((path) => relative(repo, path).replaceAll("\\", "/"));
  const wanted = pattern(entry);
  return everyFile.some((path) => wanted.test(path));
}

for (const [id, item] of items) {
  const here = name(id);
  for (const key of Object.keys(item)) {
    if (!ITEM_KEYS.includes(key)) problems.push(`${here}: unknown key "${key}"`);
  }
  if (typeof item.title !== "string" || item.title === "") problems.push(`${here}: no title`);
  if (item.status !== undefined && !STATUSES.includes(item.status)) problems.push(`${here}: unknown status "${item.status}"`);
  checkDeprecation(here, item, false);

  for (const requirement of item.requirements ?? []) {
    const where = `${here}#${requirement.id}`;
    for (const key of Object.keys(requirement)) {
      if (!REQUIREMENT_KEYS.includes(key)) problems.push(`${where}: unknown key "${key}"`);
    }
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(String(requirement.id))) problems.push(`${where}: not a valid id`);
    if (typeof requirement.text !== "string" || requirement.text === "") problems.push(`${where}: no text`);
    if (requirement.level !== undefined && !LEVELS.includes(requirement.level)) problems.push(`${where}: unknown level "${requirement.level}"`);
    if (requirement.status !== undefined && !STATUSES.includes(requirement.status)) problems.push(`${where}: unknown status "${requirement.status}"`);
    // In a deprecated item every requirement is deprecated with it.
    checkDeprecation(where, requirement, item.status === "deprecated");
    const version = requirement.version ?? 1;
    const history = (requirement.history ?? []).map((earlier) => earlier.version);
    const expected = Array.from({ length: version - 1 }, (_, index) => index + 1);
    if (history.join() !== expected.join()) {
      problems.push(`${where}: at version ${version} its history should be versions [${expected.join(", ")}], not [${history.join(", ")}]`);
    }
    for (const earlier of requirement.history ?? []) {
      if (!earlier.text || !earlier.reason) problems.push(`${where}: version ${earlier.version} in its history needs its text and the reason it was replaced`);
    }
  }

  for (const reference of item.related ?? []) {
    if (!resolves(reference)) problems.push(`${here}: related "${reference}" does not exist`);
  }
  for (const entry of item.code ?? []) {
    if (entry.startsWith("/") || !matchesAFile(entry)) problems.push(`${here}: code "${entry}" matches no file`);
  }
  for (const figure of item.figures ?? []) {
    if (!figure.src || !existsSync(join(root, figure.src))) problems.push(`${here}: figure "${figure.src}" is not in .spec`);
  }
}

if (problems.length > 0) {
  console.error(`The spec has ${problems.length} problem${problems.length === 1 ? "" : "s"}:\n` + problems.map((p) => `  ${p}`).join("\n"));
  process.exit(1);
}
console.log(`spec: ${items.size} items, ${owners.size} requirements, every id unique`);
