// The type rules the app keeps: no escape hatches from TypeScript. Data from
// outside (the Rust core, events) is parsed with zod (src/lib/types.ts), so
// nothing needs casting. `bun run build` runs this.
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/**", "src-tauri/**", "node_modules/**"] },
  {
    files: ["src/**/*.{ts,tsx}", "scripts/**/*.ts"],
    extends: [tseslint.configs.base],
    plugins: { "react-hooks": reactHooks },
    rules: {
      // No `x as T` or `<T>x`: parse or narrow instead.
      "@typescript-eslint/consistent-type-assertions": ["error", { assertionStyle: "never" }],
      // No `x!`: handle the null.
      "@typescript-eslint/no-non-null-assertion": "error",
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/ban-ts-comment": "error",
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
    },
  },
);
