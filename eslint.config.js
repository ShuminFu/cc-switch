// ESLint flat config for the React/TypeScript frontend (src/) and its tests (tests/).
//
// Pre-existing violations are recorded in eslint-suppressions.json (ESLint bulk
// suppressions), so `pnpm lint` fails only on *new* problems. Bulk suppressions
// only apply to rules set to "error", which is why the rules below use "error";
// `pnpm lint` also runs with --max-warnings 0, so a rule left at "warn" fails too.
// - Fixed some suppressed violations? Run `pnpm lint:prune` to shrink the baseline.
// - Never grow the baseline to get a PR green; fix the new problem instead.
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import pluginQuery from "@tanstack/eslint-plugin-query";
import globals from "globals";

export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "release/**",
      "node_modules/**",
      "src-tauri/**",
      "flatpak/**",
      "scripts/**",
      "docs/**",
    ],
  },
  {
    files: ["src/**/*.{ts,tsx}", "tests/**/*.{ts,tsx}"],
    extends: [
      js.configs.recommended,
      ...tseslint.configs.recommended,
      ...pluginQuery.configs["flat/recommended"],
    ],
    languageOptions: {
      ecmaVersion: 2020,
      globals: {
        ...globals.browser,
      },
    },
    plugins: {
      "react-hooks": reactHooks,
    },
    linterOptions: {
      reportUnusedDisableDirectives: "error",
    },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
      // Recommended at "warn"; promoted so the baseline can track it.
      "@tanstack/query/no-rest-destructuring": "error",
      // TypeScript (noUnusedLocals/noUnusedParameters) already reports these.
      "@typescript-eslint/no-unused-vars": "off",
    },
  },
  {
    files: ["tests/**/*.{ts,tsx}"],
    languageOptions: {
      globals: {
        ...globals.node,
        ...globals.vitest,
      },
    },
  },
);
