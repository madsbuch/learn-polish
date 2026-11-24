// The type rules the app keeps: no escape hatches from TypeScript. Data from
// outside (the Rust core, events, plugins, stored JSON) is parsed with zod in
// src/lib/schemas.ts, so nothing needs casting. `bun run build` runs this.
import { fileURLToPath } from "node:url";

import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

/** `JSON.parse` hands back `any`: only the schemas take JSON in. */
const noJsonParse = {
  "no-restricted-properties": [
    "error",
    { object: "JSON", property: "parse", message: "Parse JSON with a schema in src/lib/schemas.ts." },
  ],
};

// Spec: keel.ENG-2
// Everything is linted: the app, its tests, its scripts and the configuration.
export default tseslint.config(
  { ignores: ["dist/**", "src-tauri/**", "node_modules/**"] },
  {
    files: ["**/*.{js,mjs,cjs}"],
    rules: noJsonParse,
  },
  {
    files: ["**/*.{ts,tsx,mts,cts}"],
    extends: [tseslint.configs.base],
    plugins: { "react-hooks": reactHooks },
    rules: {
      // Spec: keel.TYP-1
      // No `x as T` or `<T>x` (`as const` is not a cast): parse or narrow instead.
      "@typescript-eslint/consistent-type-assertions": ["error", { assertionStyle: "never" }],
      // No `x!`: handle the null.
      "@typescript-eslint/no-non-null-assertion": "error",
      // Spec: keel.TYP-2
      "@typescript-eslint/no-explicit-any": "error",
      // Spec: keel.TYP-3
      "@typescript-eslint/ban-ts-comment": [
        "error",
        { "ts-ignore": true, "ts-expect-error": true, "ts-nocheck": true, "ts-check": false },
      ],
      // Spec: keel.JSN-1
      ...noJsonParse,
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
    },
  },
  {
    // Spec: keel.TYP-2
    // In the app no `any` flows anywhere: not from a library, not from JSON.
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: fileURLToPath(new URL(".", import.meta.url)) },
    },
    rules: {
      "@typescript-eslint/no-unsafe-argument": "error",
      "@typescript-eslint/no-unsafe-assignment": "error",
      "@typescript-eslint/no-unsafe-call": "error",
      "@typescript-eslint/no-unsafe-member-access": "error",
      "@typescript-eslint/no-unsafe-return": "error",
    },
  },
  {
    // Spec: keel.JSN-1
    files: ["src/lib/schemas.ts"],
    rules: { "no-restricted-properties": "off" },
  },
);
