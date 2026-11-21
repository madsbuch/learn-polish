// Checks the spec (.spec/) against the rules of its format:
// https://raw.githubusercontent.com/madsbuch/better-github/refs/heads/main/spec-llms.txt
//
// - every file is valid JSON with only the keys the schema knows;
// - every story's, requirement's and named value's id is unique in the
//   whole spec, and a requirement prefix belongs to one item ("MAST-7" and
//   "US-MAST-1" are in Mastery and nowhere else);
// - a named value has its type's form, and a requirement that names
//   something dotted in backticks names a value that exists;
// - every reference (`related`, `replacedBy`) points at an item, a story or
//   a requirement that exists, and a story's `requirements` at requirements;
// - a deprecated story, requirement or item says why (`deprecation`), and
//   only a deprecated one has a `deprecation`;
// - a requirement above version 1 has the history of all its earlier versions;
// - every figure is there;
// - every `Spec: ID` comment in the code cites a requirement the spec has
//   and has not deprecated.
//
// Runs in `bun run build`. An id never changes and is never reused: a new
// requirement takes its item's next number, and one no longer wanted is
// deprecated, not deleted. The spec never says whether something is built.

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const root = join(repo, ".spec");

const ITEM_KEYS = ["$schema", "title", "summary", "status", "deprecation", "order", "description", "stories", "requirements", "values", "figures", "related"];
const VALUE_KEYS = ["id", "type", "value", "dark", "unit", "description", "status", "deprecation", "history"];
/** Whether a value has the form its type asks for. */
const VALUE_FORMS = {
  color: (value) => /^#(?:[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/.test(value),
  dimension: (value) => /^-?\d+(?:\.\d+)?(?:px|rem|em|%|pt|dp|vw|vh)$/.test(value),
  duration: (value) => /^\d+(?:\.\d+)?(?:ms|s|min|h|d)$/.test(value),
  number: (value) => typeof value === "number",
  text: (value) => typeof value === "string",
  font: (value) => typeof value === "string" && value !== "",
  list: (value) => Array.isArray(value) && value.every((name) => typeof name === "string" && name !== ""),
};
/** Where the code is, and the folders in it that are not ours. */
const CODE = ["src", "src-tauri/src", "src-tauri/plugins", "src-tauri/android", "scripts"];
const SKIPPED = new Set(["node_modules", "target", "build", "dist", "gen", ".gradle"]);
const SOURCE = new Set([".rs", ".ts", ".tsx", ".mjs", ".js", ".css", ".kt", ".kts", ".sql"]);
const STORY_KEYS = ["id", "as", "want", "soThat", "requirements", "status", "deprecation"];
const REQUIREMENT_KEYS = ["id", "text", "level", "status", "version", "history", "deprecation"];
const STATUSES = ["draft", "accepted", "deprecated"];
const LEVELS = ["must", "should", "may"];

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

// Where each id is used. Stories, requirements and values share one set of ids.
const owners = new Map();
const requirementIds = new Set();
const deprecatedIds = new Set();
const valueIds = new Set();
for (const [id, item] of items) {
  for (const thing of [...(item.stories ?? []), ...(item.requirements ?? []), ...(item.values ?? [])]) {
    owners.set(thing.id, [...(owners.get(thing.id) ?? []), id]);
  }
  for (const requirement of item.requirements ?? []) {
    requirementIds.add(requirement.id);
    if (requirement.status === "deprecated" || item.status === "deprecated") deprecatedIds.add(requirement.id);
  }
  for (const value of item.values ?? []) valueIds.add(value.id);
}
for (const [code, where] of owners) {
  if (where.length > 1) problems.push(`id ${code} is used ${where.length} times: ${where.map(name).join(", ")}`);
}

// A prefix belongs to one item, so a new requirement can't take an id
// another item will want.
const prefixes = new Map();
for (const [id, item] of items) {
  // Values are named for what they are ("mastery.full"), not for their item.
  for (const thing of [...(item.stories ?? []), ...(item.requirements ?? [])]) {
    const prefix = String(thing.id).replace(/-?\d+$/, "");
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

for (const [id, item] of items) {
  const here = name(id);
  for (const key of Object.keys(item)) {
    if (!ITEM_KEYS.includes(key)) problems.push(`${here}: unknown key "${key}"`);
  }
  if (typeof item.title !== "string" || item.title === "") problems.push(`${here}: no title`);
  if (item.status !== undefined && !STATUSES.includes(item.status)) problems.push(`${here}: unknown status "${item.status}"`);
  checkDeprecation(here, item, false);

  for (const story of item.stories ?? []) {
    const where = `${here}#${story.id}`;
    for (const key of Object.keys(story)) {
      if (!STORY_KEYS.includes(key)) problems.push(`${where}: unknown key "${key}"`);
    }
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(String(story.id))) problems.push(`${where}: not a valid id`);
    if (!story.as || !story.want) problems.push(`${where}: a story says who (as) wants what (want)`);
    if (story.status !== undefined && !STATUSES.includes(story.status)) problems.push(`${where}: unknown status "${story.status}"`);
    checkDeprecation(where, story, item.status === "deprecated");
    for (const reference of story.requirements ?? []) {
      const code = reference.split("#").pop().split("@")[0];
      if (!resolves(reference) || !requirementIds.has(code)) problems.push(`${where}: "${reference}" is not a requirement`);
    }
  }

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
    // A dotted name in backticks is a value's name ("mastery.full").
    for (const [, named] of String(requirement.text).matchAll(/`([a-z][a-z0-9-]*(?:\.[a-z0-9-]+)+)`/g)) {
      if (!valueIds.has(named) && !/\.(rs|tsx?|mjs|json|sqlite|md|css)$/.test(named)) problems.push(`${where}: names \`${named}\`, which is not a value`);
    }
  }

  for (const value of item.values ?? []) {
    const where = `${here}#${value.id}`;
    for (const key of Object.keys(value)) {
      if (!VALUE_KEYS.includes(key)) problems.push(`${where}: unknown key "${key}"`);
    }
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(String(value.id))) problems.push(`${where}: not a valid id`);
    const form = VALUE_FORMS[value.type];
    if (!form) problems.push(`${where}: unknown type "${value.type}"`);
    else if (!form(value.value)) problems.push(`${where}: ${JSON.stringify(value.value)} is not a ${value.type}`);
    if (value.dark !== undefined && (value.type !== "color" || !VALUE_FORMS.color(value.dark))) problems.push(`${where}: "dark" is a colour's value in dark mode`);
    if (value.unit !== undefined && value.type !== "number") problems.push(`${where}: only a number has a unit`);
    if (value.status !== undefined && !STATUSES.includes(value.status)) problems.push(`${where}: unknown status "${value.status}"`);
    checkDeprecation(where, value, item.status === "deprecated");
    for (const earlier of value.history ?? []) {
      if (earlier.value === undefined || !earlier.reason) problems.push(`${where}: its history needs what it was and the reason it changed`);
    }
  }

  for (const reference of item.related ?? []) {
    if (!resolves(reference)) problems.push(`${here}: related "${reference}" does not exist`);
  }
  for (const figure of item.figures ?? []) {
    if (!figure.src || !existsSync(join(root, figure.src))) problems.push(`${here}: figure "${figure.src}" is not in .spec`);
  }
}

// The code cites the spec: "Spec: RET-1" or "Spec: RET-1, RET-2@2", alone
// in a comment. Every id cited has to be a requirement the spec still wants.
let citations = 0;
for (const folder of CODE) {
  if (!existsSync(join(repo, folder))) continue;
  for (const path of files(join(repo, folder), (name) => SOURCE.has(extname(name)))) {
    const lines = readFileSync(path, "utf8").split("\n");
    lines.forEach((line, index) => {
      const cited = line.match(/^\s*(?:\/\/|\/\*|\*|#|--)\s*Spec:\s*(.+?)\s*(?:\*\/)?\s*$/);
      if (!cited) return;
      for (const reference of cited[1].split(",")) {
        const code = reference.trim().split("@")[0];
        const where = `${relative(repo, path).replaceAll("\\", "/")}:${index + 1}`;
        citations += 1;
        if (!requirementIds.has(code)) problems.push(`${where}: cites ${code}, which is not a requirement`);
        else if (deprecatedIds.has(code)) problems.push(`${where}: cites ${code}, which is deprecated`);
      }
    });
  }
}

if (problems.length > 0) {
  console.error(`The spec has ${problems.length} problem${problems.length === 1 ? "" : "s"}:\n` + problems.map((p) => `  ${p}`).join("\n"));
  process.exit(1);
}
const stories = [...items.values()].reduce((count, item) => count + (item.stories ?? []).length, 0);
console.log(`spec: ${items.size} items, ${stories} stories, ${requirementIds.size} requirements, ${valueIds.size} values, every id unique; ${citations} citations in the code`);
