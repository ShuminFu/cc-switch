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
import ccSwitch from "./eslint-rules/index.js";

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
      "coverage/**",
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
    // Ratchets for the app code: the existing violations live in the baseline,
    // new ones fail `pnpm lint`. tests/lint/eslintConfig.test.ts caps the baseline.
    files: ["src/**/*.{ts,tsx}"],
    plugins: {
      "cc-switch": ccSwitch,
    },
    rules: {
      // Branch on registry capabilities, not on app-id literals.
      "cc-switch/no-app-literal-comparison": "error",
      // Backend calls go through the typed API layer, not raw invoke().
      "no-restricted-imports": [
        "error",
        {
          paths: [
            {
              name: "@tauri-apps/api/core",
              importNames: ["invoke"],
              message:
                "Call the backend through src/lib/api (or src/shared/bindings.ts) instead of importing invoke directly.",
            },
          ],
        },
      ],
    },
  },
  {
    // The bindings layer is the one place allowed to call invoke().
    files: ["src/lib/api/**/*.{ts,tsx}", "src/shared/bindings.ts"],
    rules: {
      "no-restricted-imports": "off",
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
