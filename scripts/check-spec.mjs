// Checks the spec (.spec/) against the rules of its format:
// https://raw.githubusercontent.com/madsbuch/better-github/refs/heads/main/spec-llms.txt
//
// - every file is valid JSON that fits the format's schema: only the keys it
//   knows (there is no `status`), the right types, levels and date forms;
// - every story's, requirement's and named value's id is unique in the whole
//   spec. Ids follow a requirement when it moves, so a prefix may appear in
//   any item;
// - a named value has its type's form, its history says what it was and
//   why, and a requirement that names something dotted in backticks names a
//   value that exists;
// - every reference (`related`, `replacedBy`, a story's `requirements`)
//   points at an item, story, requirement or value that exists, a story's
//   `requirements` at requirements, and a reference to a version ("#ID@2")
//   at a version the requirement has had;
// - a requirement above version 1 has the history of all its earlier
//   versions, each with its text and the reason it was replaced;
// - what is dropped carries a `deprecation` that says why; in a deprecated
//   item everything is dropped with it;
// - every figure is there;
// - every `Spec: ID` comment in the code cites a requirement the spec has and
//   has not deprecated;
// - no requirement, story, summary or description says a word a term rules
//   out: the "Not: a, b." that ends a `term.*` value's description; nor does
//   a string or JSX text in the screens (src/**/*.tsx).
//
// Runs in `bun run build`. An id never changes and is never reused.

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import ts from "typescript";

const repo = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const root = join(repo, ".spec");

const ITEM_KEYS = ["$schema", "title", "summary", "deprecation", "order", "description", "stories", "requirements", "values", "figures", "related"];
const STORY_KEYS = ["id", "as", "want", "soThat", "requirements", "deprecation"];
const REQUIREMENT_KEYS = ["id", "text", "level", "version", "history", "deprecation"];
const HISTORY_KEYS = ["version", "text", "level", "reason", "date"];
const VALUE_KEYS = ["id", "type", "value", "description", "deprecation", "history"];
const VALUE_HISTORY_KEYS = ["value", "dark", "reason", "date"];
const DEPRECATION_KEYS = ["reason", "replacedBy", "date"];
const FIGURE_KEYS = ["src", "alt", "caption"];
const LEVELS = ["must", "should", "may"];
const ID = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;
const COLOR = /^#(?:[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
/** Whether a value has the form its type asks for, and the keys only that type has. */
const VALUE_TYPES = {
  color: { form: (value) => typeof value === "string" && COLOR.test(value), keys: ["dark"] },
  dimension: { form: (value) => typeof value === "string" && /^-?\d+(?:\.\d+)?(?:px|rem|em|%|pt|dp|vw|vh)$/.test(value), keys: [] },
  duration: { form: (value) => typeof value === "string" && /^\d+(?:\.\d+)?(?:ms|s|min|h|d)$/.test(value), keys: [] },
  number: { form: (value) => typeof value === "number", keys: ["unit"] },
  text: { form: (value) => typeof value === "string", keys: [] },
  font: { form: (value) => typeof value === "string" && value !== "", keys: [] },
  list: { form: (value) => Array.isArray(value) && value.every((name) => typeof name === "string" && name !== ""), keys: [] },
};
/** Where the code is, and the folders in it that are not ours. */
const CODE = ["src", "src-tauri/src", "src-tauri/plugins", "src-tauri/android", "scripts"];
const SKIPPED = new Set(["node_modules", "target", "build", "dist", "gen", ".gradle"]);
const SOURCE = new Set([".rs", ".ts", ".tsx", ".mjs", ".js", ".css", ".kt", ".kts", ".sql"]);

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
const isObject = (thing) => typeof thing === "object" && thing !== null && !Array.isArray(thing);
const isText = (thing) => typeof thing === "string" && thing !== "";
const list = (item, key) => (Array.isArray(item[key]) ? item[key].filter(isObject) : []);

function unknownKeys(where, thing, known) {
  for (const key of Object.keys(thing)) {
    if (!known.includes(key)) problems.push(`${where}: unknown key "${key}"`);
  }
}

// Where each id is used. Stories, requirements and values share one set of ids.
const owners = new Map();
const requirements = new Map();
const deprecatedIds = new Set();
const valueIds = new Set();
for (const [id, item] of items) {
  if (!isObject(item)) continue;
  for (const thing of [...list(item, "stories"), ...list(item, "requirements"), ...list(item, "values")]) {
    owners.set(thing.id, [...(owners.get(thing.id) ?? []), id]);
  }
  for (const requirement of list(item, "requirements")) {
    requirements.set(requirement.id, requirement);
    // In a deprecated item every requirement is deprecated with it.
    if (requirement.deprecation !== undefined || item.deprecation !== undefined) deprecatedIds.add(requirement.id);
  }
  for (const value of list(item, "values")) valueIds.add(value.id);
}
for (const [code, where] of owners) {
  if (where.length > 1) problems.push(`id ${code} is used ${where.length} times: ${where.map(name).join(", ")}`);
}

/** What a reference names: "item", "item#ID", "#ID" or "ID", each id with an optional "@version". */
function resolve(reference, bare = false) {
  if (typeof reference !== "string" || reference === "") return { ok: false };
  const hash = reference.indexOf("#");
  if (hash === -1 && !bare) return { ok: items.has(reference), item: true };
  const target = hash === -1 ? "" : reference.slice(0, hash);
  const [code, version, ...rest] = reference.slice(hash + 1).split("@");
  const where = owners.get(code);
  if (where === undefined || rest.length > 0 || (target !== "" && !where.includes(target))) return { ok: false };
  if (version !== undefined) {
    const requirement = requirements.get(code);
    const number = Number(version);
    if (!requirement || !/^[1-9]\d*$/.test(version) || number > (requirement.version ?? 1)) return { ok: false };
  }
  return { ok: true, code };
}

function checkDate(where, date) {
  if (date !== undefined && (typeof date !== "string" || !DATE.test(date))) problems.push(`${where}: date "${date}" is not YYYY-MM-DD`);
}

function checkDeprecation(where, deprecation) {
  if (deprecation === undefined) return;
  if (!isObject(deprecation)) return problems.push(`${where}: deprecation is not an object`);
  unknownKeys(`${where} deprecation`, deprecation, DEPRECATION_KEYS);
  if (!isText(deprecation.reason)) problems.push(`${where}: deprecated without a reason`);
  checkDate(`${where} deprecation`, deprecation.date);
  if (deprecation.replacedBy !== undefined && !Array.isArray(deprecation.replacedBy)) problems.push(`${where}: replacedBy is not a list`);
  for (const reference of deprecation.replacedBy ?? []) {
    if (!resolve(reference).ok) problems.push(`${where}: replacedBy "${reference}" does not exist`);
  }
}

function checkId(where, thing) {
  if (typeof thing.id !== "string" || !ID.test(thing.id)) problems.push(`${where}: not a valid id`);
}

function checkArray(where, item, key) {
  if (item[key] !== undefined && !Array.isArray(item[key])) problems.push(`${where}: "${key}" is not a list`);
  for (const thing of Array.isArray(item[key]) ? item[key] : []) {
    if (!isObject(thing)) problems.push(`${where}: an entry of "${key}" is not an object`);
  }
}

for (const [id, item] of items) {
  const here = name(id);
  if (!isObject(item)) {
    problems.push(`${here}: not an object`);
    continue;
  }
  unknownKeys(here, item, ITEM_KEYS);
  if (!isText(item.title)) problems.push(`${here}: no title`);
  for (const key of ["$schema", "summary", "description"]) {
    if (item[key] !== undefined && typeof item[key] !== "string") problems.push(`${here}: "${key}" is not text`);
  }
  if (item.order !== undefined && typeof item.order !== "number") problems.push(`${here}: "order" is not a number`);
  checkDeprecation(here, item.deprecation);
  for (const key of ["stories", "requirements", "values", "figures"]) checkArray(here, item, key);

  for (const story of list(item, "stories")) {
    const where = `${here}#${story.id}`;
    unknownKeys(where, story, STORY_KEYS);
    checkId(where, story);
    if (!isText(story.as) || !isText(story.want)) problems.push(`${where}: a story says who (as) wants what (want)`);
    if (story.soThat !== undefined && !isText(story.soThat)) problems.push(`${where}: "soThat" is empty`);
    checkDeprecation(where, story.deprecation);
    if (story.requirements !== undefined && !Array.isArray(story.requirements)) problems.push(`${where}: "requirements" is not a list`);
    for (const reference of story.requirements ?? []) {
      const { ok, code } = resolve(reference);
      if (!ok || !requirements.has(code)) problems.push(`${where}: "${reference}" is not a requirement`);
    }
  }

  for (const requirement of list(item, "requirements")) {
    const where = `${here}#${requirement.id}`;
    unknownKeys(where, requirement, REQUIREMENT_KEYS);
    checkId(where, requirement);
    if (!isText(requirement.text)) problems.push(`${where}: no text`);
    if (requirement.level !== undefined && !LEVELS.includes(requirement.level)) problems.push(`${where}: unknown level "${requirement.level}"`);
    const version = requirement.version ?? 1;
    if (!Number.isInteger(version) || version < 1) problems.push(`${where}: version ${version} is not a whole number from 1`);
    checkDeprecation(where, requirement.deprecation);
    if (requirement.history !== undefined && !Array.isArray(requirement.history)) problems.push(`${where}: "history" is not a list`);
    const history = Array.isArray(requirement.history) ? requirement.history : [];
    const versions = history.map((earlier) => earlier?.version);
    const expected = Array.from({ length: Number.isInteger(version) ? version - 1 : 0 }, (_, index) => index + 1);
    if (versions.join() !== expected.join()) {
      problems.push(`${where}: at version ${version} its history should be versions [${expected.join(", ")}], not [${versions.join(", ")}]`);
    }
    for (const earlier of history) {
      if (!isObject(earlier)) continue;
      unknownKeys(`${where}@${earlier.version}`, earlier, HISTORY_KEYS);
      if (!isText(earlier.text) || !isText(earlier.reason)) problems.push(`${where}: version ${earlier.version} in its history needs its text and the reason it was replaced`);
      if (earlier.level !== undefined && !LEVELS.includes(earlier.level)) problems.push(`${where}@${earlier.version}: unknown level "${earlier.level}"`);
      checkDate(`${where}@${earlier.version}`, earlier.date);
    }
    // A dotted name in backticks is a value's name ("mastery.full").
    for (const [, named] of String(requirement.text).matchAll(/`([a-z][a-z0-9-]*(?:\.[a-z0-9-]+)+)`/g)) {
      if (!valueIds.has(named)) problems.push(`${where}: names \`${named}\`, which is not a value`);
    }
  }

  for (const value of list(item, "values")) {
    const where = `${here}#${value.id}`;
    const type = VALUE_TYPES[value.type];
    unknownKeys(where, value, [...VALUE_KEYS, ...(type?.keys ?? [])]);
    checkId(where, value);
    if (!type) problems.push(`${where}: unknown type "${value.type}"`);
    else if (!type.form(value.value)) problems.push(`${where}: ${JSON.stringify(value.value)} is not a ${value.type}`);
    if (value.dark !== undefined && !COLOR.test(String(value.dark))) problems.push(`${where}: "dark" is not a colour`);
    if (value.unit !== undefined && !isText(value.unit)) problems.push(`${where}: "unit" is empty`);
    if (value.description !== undefined && typeof value.description !== "string") problems.push(`${where}: "description" is not text`);
    checkDeprecation(where, value.deprecation);
    if (value.history !== undefined && !Array.isArray(value.history)) problems.push(`${where}: "history" is not a list`);
    for (const earlier of Array.isArray(value.history) ? value.history : []) {
      if (!isObject(earlier)) continue;
      unknownKeys(`${where} history`, earlier, VALUE_HISTORY_KEYS);
      const was = earlier.value;
      const fits = typeof was === "string" || typeof was === "number" || (Array.isArray(was) && was.every((name) => typeof name === "string"));
      if (!fits || !isText(earlier.reason)) problems.push(`${where}: its history needs what it was and the reason it changed`);
      if (earlier.dark !== undefined && typeof earlier.dark !== "string") problems.push(`${where}: a "dark" in its history is not text`);
      checkDate(`${where} history`, earlier.date);
    }
  }

  if (item.related !== undefined && !Array.isArray(item.related)) problems.push(`${here}: "related" is not a list`);
  for (const reference of item.related ?? []) {
    if (!resolve(reference).ok) problems.push(`${here}: related "${reference}" does not exist`);
  }
  for (const figure of list(item, "figures")) {
    unknownKeys(`${here} figure`, figure, FIGURE_KEYS);
    if (!isText(figure.src) || figure.src.startsWith("/") || !existsSync(join(root, figure.src))) problems.push(`${here}: figure "${figure.src}" is not in .spec`);
  }
}

// Spec: TERM-3
// The words the terms rule out, and every text the spec says them in: what
// is current, not a history or a deprecation's record. Backticked names and
// each term's own "Not:" sentence are not prose.
const NOT = /(?:^|\s)Not: ([^.]+)\.\s*$/;
const ruledOut = [];
for (const item of items.values()) {
  if (!isObject(item) || item.deprecation !== undefined) continue;
  for (const value of list(item, "values")) {
    if (value.deprecation !== undefined || !String(value.id).startsWith("term.") || typeof value.description !== "string") continue;
    const not = value.description.match(NOT);
    for (const word of not ? not[1].split(",") : []) {
      const escaped = word.trim().replaceAll("'", "’").replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replaceAll("’", "['’]");
      ruledOut.push({ term: value.id, word: word.trim(), pattern: new RegExp(`(?<![\\p{L}\\p{N}])${escaped}s?(?![\\p{L}\\p{N}])`, "iu") });
    }
  }
}
function checkWords(where, text) {
  if (typeof text !== "string") return;
  const prose = text.replace(/`[^`]*`/g, " ").replace(NOT, "");
  for (const { term, word, pattern } of ruledOut) {
    if (pattern.test(prose)) problems.push(`${where}: says "${word}", which \`${term}\` rules out`);
  }
}
for (const [id, item] of items) {
  if (!isObject(item) || item.deprecation !== undefined) continue;
  const here = name(id);
  checkWords(here, item.summary);
  checkWords(here, item.description);
  for (const story of list(item, "stories")) {
    if (story.deprecation === undefined) for (const key of ["as", "want", "soThat"]) checkWords(`${here}#${story.id}`, story[key]);
  }
  for (const requirement of list(item, "requirements")) {
    if (requirement.deprecation === undefined) checkWords(`${here}#${requirement.id}`, requirement.text);
  }
  for (const value of list(item, "values")) {
    if (value.deprecation === undefined) checkWords(`${here}#${value.id}`, value.description);
  }
}

// Spec: TERM-1
// The interface's own words: every string and JSX text in the screens.
function interfaceTexts(source) {
  const texts = [];
  const visit = (node) => {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node) || ts.isTemplateLiteralToken(node) || ts.isJsxText(node)) {
      texts.push({ text: node.text, line: source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1 });
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return texts;
}
for (const path of existsSync(join(repo, "src")) ? files(join(repo, "src"), (file) => file.endsWith(".tsx")) : []) {
  const source = ts.createSourceFile(path, readFileSync(path, "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  for (const { text, line } of interfaceTexts(source)) {
    const words = text.replaceAll("&apos;", "'");
    for (const { term, word, pattern } of ruledOut) {
      if (pattern.test(words)) problems.push(`${relative(repo, path).replaceAll("\\", "/")}:${line}: says "${word}", which \`${term}\` rules out`);
    }
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
        const where = `${relative(repo, path).replaceAll("\\", "/")}:${index + 1}`;
        const { ok, code } = resolve(reference.trim(), true);
        citations += 1;
        if (!ok || !requirements.has(code)) problems.push(`${where}: cites ${reference.trim()}, which is not a requirement`);
        else if (deprecatedIds.has(code)) problems.push(`${where}: cites ${code}, which is deprecated`);
      }
    });
  }
}

if (problems.length > 0) {
  console.error(`The spec has ${problems.length} problem${problems.length === 1 ? "" : "s"}:\n` + problems.map((p) => `  ${p}`).join("\n"));
  process.exit(1);
}
const stories = [...items.values()].reduce((count, item) => count + list(item, "stories").length, 0);
console.log(`spec: ${items.size} items, ${stories} stories, ${requirements.size} requirements, ${valueIds.size} values, every id unique; ${citations} citations in the code`);
