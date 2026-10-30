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
      ".claude/**",
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
    // FE-ARCH-04: every TanStack Query key comes from src/lib/query/keys.ts,
    // so producers, consumers and invalidations cannot drift apart.
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/lib/query/**"],
    rules: {
      "no-restricted-syntax": [
        "error",
        {
          selector:
            "Property[key.name='queryKey'] > ArrayExpression, Property[key.name='queryKey'] > TSAsExpression > ArrayExpression",
          message:
            "Use a key from src/lib/query/keys.ts instead of a literal queryKey array.",
        },
        {
          selector:
            "CallExpression[callee.property.name=/^(setQueryData|getQueryData|getQueryState|setQueriesData|getQueriesData|fetchQuery|prefetchQuery|ensureQueryData)$/] > ArrayExpression.arguments",
          message:
            "Use a key from src/lib/query/keys.ts instead of a literal query key array.",
        },
        {
          selector:
            "VariableDeclarator[id.name=/^(queryKey|QUERY_KEY|[a-z]\\w*QueryKey)$/] > ArrayExpression, VariableDeclarator[id.name=/^(queryKey|QUERY_KEY|[a-z]\\w*QueryKey)$/] > TSAsExpression > ArrayExpression",
          message:
            "Define query keys in src/lib/query/keys.ts instead of a local array.",
        },
      ],
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
