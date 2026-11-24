// Parses what the Rust core sends the screens (written to
// src-tauri/target/shapes by the `shapes_for_the_screens` test) with the zod
// schemas in src/lib/schemas.ts, so the two cannot drift apart unnoticed.
// Run with `bun run check:shapes`.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";

import * as types from "../src/lib/schemas";

const dir = join(import.meta.dir, "..", "src-tauri", "target", "shapes");
const schemas = new Map<string, z.ZodTypeAny>(
  Object.entries(types).flatMap(([name, value]) =>
    name.endsWith("Schema") && value instanceof z.ZodType ? [[name.slice(0, -"Schema".length), value]] : [],
  ),
);

let failed = 0;
const files = readdirSync(dir).filter((file) => file.endsWith(".json"));
for (const file of files) {
  const [name, list] = file.replace(/\.json$/, "").split(".");
  const schema = schemas.get(name);
  if (!schema) {
    console.error(`✗ ${file}: no ${name}Schema in src/lib/schemas.ts`);
    failed++;
    continue;
  }
  const parsed = types.parseJson(list === "list" ? z.array(schema) : schema, readFileSync(join(dir, file), "utf8"));
  if (parsed.success) {
    console.log(`✓ ${file}`);
  } else {
    failed++;
    for (const issue of parsed.error.issues.slice(0, 8)) {
      console.error(`✗ ${file}: ${issue.path.join(".")}: ${issue.message}`);
    }
  }
}
if (files.length === 0) {
  console.error("No shapes: run the Rust test shapes_for_the_screens first.");
  process.exit(1);
}
process.exit(failed ? 1 : 0);
