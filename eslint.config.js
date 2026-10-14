// ESLint is used only to catch unhandled promises (type-aware).
// Formatting is Prettier's job and type errors are `pnpm typecheck`'s,
// so no stylistic or recommended rule sets are enabled here.
import tseslint from "typescript-eslint";

export default tseslint.config({
  files: ["src/**/*.{ts,tsx}"],
  languageOptions: {
    parser: tseslint.parser,
    parserOptions: {
      projectService: true,
      tsconfigRootDir: import.meta.dirname,
    },
  },
  plugins: {
    "@typescript-eslint": tseslint.plugin,
  },
  linterOptions: {
    // Existing directives target rules from the core/react-hooks sets, which
    // are intentionally not enabled here.
    reportUnusedDisableDirectives: "off",
  },
  rules: {
    "@typescript-eslint/no-floating-promises": "error",
    "@typescript-eslint/no-misused-promises": "error",
  },
});
