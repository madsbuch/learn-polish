// How the app is built (the shared part Keel, .spec/keel): the build's
// steps, the lint rules, the crates' lints and how old a package must be.
// Run with `bun test`.
import { describe, expect, test } from "bun:test";
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { ESLint } from "eslint";
import { z } from "zod";

const repo = join(import.meta.dir, "..");

const PartSchema = z.object({
  values: z.array(z.object({ id: z.string(), value: z.unknown() })).default([]),
});
const RootSchema = z.object({
  imports: z
    .array(z.object({ id: z.string(), values: z.array(z.object({ id: z.string(), value: z.unknown() })).default([]) }))
    .default([]),
});

/** A named value of the spec, as this product sets it: the import's own setting, else the part's. */
async function specValue(id: string): Promise<unknown> {
  const root = RootSchema.parse(await Bun.file(join(repo, ".spec", "index.json")).json());
  const set = root.imports.flatMap((part) => part.values).find((value) => value.id === id);
  if (set) return set.value;
  const part = PartSchema.parse(await Bun.file(join(repo, ".spec", "keel", "index.json")).json());
  return part.values.find((value) => value.id === id)?.value;
}

const SECONDS = { s: 1, min: 60, h: 3600, d: 86400 } as const;
const UnitSchema = z.enum(["s", "min", "h", "d"]);

function seconds(duration: unknown): number {
  const match = /^(\d+)(s|min|h|d)$/.exec(String(duration));
  if (!match) throw new Error(`not a duration in whole seconds or more: ${String(duration)}`);
  return Number(match[1]) * SECONDS[UnitSchema.parse(match[2])];
}

const BuildSchema = z.object({ scripts: z.object({ build: z.string() }) });
const TsconfigSchema = z.object({ include: z.array(z.string()) });
const BunfigSchema = z.object({ install: z.object({ minimumReleaseAge: z.number() }) });
const CrateSchema = z.object({
  package: z.object({ name: z.string() }),
  lints: z.object({ rust: z.object({ unsafe_code: z.string() }) }),
});

describe("the build", () => {
  // Spec: keel.ENG-2
  test("typechecks src/, lints everything and bundles", async () => {
    const steps = BuildSchema.parse(await Bun.file(join(repo, "package.json")).json()).scripts.build.split("&&").map((step) => step.trim());
    expect(steps).toContain("eslint .");
    expect(steps).toContain("tsc --noEmit");
    expect(steps).toContain("vite build");
    expect(steps.indexOf("vite build")).toBe(steps.length - 1);
    expect(TsconfigSchema.parse(await Bun.file(join(repo, "tsconfig.json")).json()).include).toContain("src");
  });

  // Spec: keel.ENG-3
  test("installs a package only once it has been published for keel.deps-minimum-age", async () => {
    const bunfig = BunfigSchema.parse(Bun.TOML.parse(await Bun.file(join(repo, "bunfig.toml")).text()));
    expect(bunfig.install.minimumReleaseAge).toBe(seconds(await specValue("keel.deps-minimum-age")));
  });
});

describe("the lint rules", () => {
  const eslint = new ESLint({ cwd: repo });
  async function problems(code: string): Promise<string[]> {
    const [result] = await eslint.lintText(code, { filePath: join(repo, "tests", "probe.ts") });
    return (result?.messages ?? []).map((message) => message.ruleId ?? message.message);
  }

  // Spec: keel.TYP-1
  test("refuse a cast and a non-null assertion, and allow `as const`", async () => {
    expect(await problems("export const a = (1 as unknown) as number;\n")).toContain("@typescript-eslint/consistent-type-assertions");
    expect(await problems("export const b = <number>(1 as unknown);\n")).toContain("@typescript-eslint/consistent-type-assertions");
    expect(await problems("declare const c: string | null;\nexport const d = c!;\n")).toContain("@typescript-eslint/no-non-null-assertion");
    expect(await problems("export const e = [1, 2] as const;\n")).toEqual([]);
  });

  // Spec: keel.TYP-2
  test("refuse an explicit any", async () => {
    expect(await problems("export const a: any = 1;\n")).toContain("@typescript-eslint/no-explicit-any");
  });

  // Spec: keel.TYP-3
  test("refuse @ts-ignore, @ts-expect-error and @ts-nocheck", async () => {
    for (const directive of ["@ts-ignore", "@ts-expect-error because", "@ts-nocheck"]) {
      expect(await problems(`// ${directive}\nexport const a = 1;\n`)).toContain("@typescript-eslint/ban-ts-comment");
    }
  });

  // Spec: keel.JSN-1
  test("refuse JSON.parse outside src/lib/schemas.ts", async () => {
    expect(await problems("export const a: unknown = JSON.parse('1');\n")).toContain("no-restricted-properties");
  });
});

describe("the Rust crates", () => {
  // Spec: keel.TYP-5
  test("all forbid unsafe code", async () => {
    const plugins = readdirSync(join(repo, "src-tauri", "plugins")).map((name) => join("plugins", name));
    const crates = [".", ...plugins].map((dir) => join(repo, "src-tauri", dir, "Cargo.toml"));
    expect(crates.length).toBeGreaterThan(1);
    for (const path of crates) {
      const crate = CrateSchema.parse(Bun.TOML.parse(await Bun.file(path).text()));
      expect(`${crate.package.name}: ${crate.lints.rust.unsafe_code}`).toBe(`${crate.package.name}: forbid`);
    }
  });
});
