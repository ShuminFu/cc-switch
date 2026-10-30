// ESLint flat config (ESLint 9).
//
// Baseline: rules that the current codebase violates are set to "warn" so
// `pnpm lint` passes today; ratchet them to "error" as the warnings are fixed.
import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      "coverage/**",
      "src-tauri/**",
      "flatpak/**",
      "release/**",
      "src/icons/extracted/**",
    ],
  },
  {
    files: ["**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: {
      ecmaVersion: 2022,
      globals: { ...globals.browser },
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-hooks/exhaustive-deps": "warn",
      "react-refresh/only-export-components": [
        "warn",
        { allowConstantExport: true },
      ],
      // tsc (noUnusedLocals / noUnusedParameters) already enforces these.
      "@typescript-eslint/no-unused-vars": "off",
      // Baseline warnings (existing violations); promote to "error" once fixed.
      "@typescript-eslint/no-explicit-any": "warn",
      "no-empty": "warn",
      "no-empty-pattern": "warn",
      "no-control-regex": "warn",
    },
  },
  {
    files: ["tests/**/*.{ts,tsx}", "**/*.test.{ts,tsx}"],
    languageOptions: {
      globals: { ...globals.browser, ...globals.node, ...globals.vitest },
    },
  },
  {
    files: ["**/*.{js,cjs,mjs}"],
    extends: [js.configs.recommended],
    languageOptions: {
      globals: { ...globals.node },
    },
  },
  {
    files: ["*.config.{ts,js,cjs}"],
    languageOptions: {
      globals: { ...globals.node },
    },
  },
);
