// Checks the spec (.spec/): every requirement's code is unique across the
// whole spec, not just within its item, and every reference to another item
// or requirement points at one that exists. Runs in `bun run build`.
//
// A requirement's code is its item's prefix and a number ("MAST-7"). It never
// changes and is never reused: a new requirement takes the next number.

import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(fileURLToPath(new URL(".", import.meta.url)), "..", ".spec");

function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory() ? files(join(dir, entry.name)) : entry.name.endsWith(".json") ? [join(dir, entry.name)] : [],
  );
}

/** An item's id: its path without ".json"; a folder's own item is its index. */
function idOf(path) {
  const id = relative(root, path).replaceAll("\\", "/").replace(/\.json$/, "");
  return id === "index" ? "" : id.replace(/\/index$/, "");
}

const problems = [];
const items = new Map();
for (const path of files(root)) {
  try {
    items.set(idOf(path), JSON.parse(readFileSync(path, "utf8")));
  } catch (error) {
    problems.push(`${relative(root, path)}: not valid JSON (${error.message})`);
  }
}

// Where each requirement code is used.
const codes = new Map();
for (const [id, item] of items) {
  for (const requirement of item.requirements ?? []) {
    codes.set(requirement.id, [...(codes.get(requirement.id) ?? []), id || "(root)"]);
  }
}
for (const [code, where] of codes) {
  if (where.length > 1) problems.push(`requirement code ${code} is used ${where.length} times: ${where.join(", ")}`);
}

// A prefix belongs to one item, so a new requirement can't take a code
// another item will want.
const prefixes = new Map();
for (const [id, item] of items) {
  for (const requirement of item.requirements ?? []) {
    const prefix = requirement.id.replace(/-?\d+$/, "");
    const owner = prefixes.get(prefix);
    if (owner === undefined) prefixes.set(prefix, id);
    else if (owner !== id) problems.push(`prefix ${prefix} is used by two items: ${owner || "(root)"} and ${id || "(root)"}`);
  }
}

for (const [id, item] of items) {
  for (const reference of item.related ?? []) {
    const [target, code] = reference.split("#");
    const other = items.get(target);
    if (!other) problems.push(`${id || "(root)"}: related item "${target}" does not exist`);
    else if (code && !(other.requirements ?? []).some((r) => r.id === code)) {
      problems.push(`${id || "(root)"}: related requirement "${reference}" does not exist`);
    }
  }
}

if (problems.length > 0) {
  console.error(`The spec has ${problems.length} problem${problems.length === 1 ? "" : "s"}:\n` + problems.map((p) => `  ${p}`).join("\n"));
  process.exit(1);
}
console.log(`spec: ${items.size} items, ${codes.size} requirement codes, all unique`);
