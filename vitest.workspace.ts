import { defineWorkspace } from "vitest/config";

// TEST-24: pure-logic tests (no DOM) run in the cheap `node` environment;
// only component/hook/integration tests pay for jsdom. Both projects share
// vitest.config.ts (aliases, setup files, coverage) via `extends`.
//
//   pnpm test:unit                      # everything
//   pnpm test:unit --project unit       # node-only project
//   pnpm test:unit --project dom        # jsdom project
const NODE_TESTS = [
  "src/**/*.test.ts",
  "tests/{config,contracts,i18n,scripts,utils,lib}/**/*.test.ts",
];

export default defineWorkspace([
  {
    extends: "./vitest.config.ts",
    test: {
      name: "unit",
      environment: "node",
      include: NODE_TESTS,
    },
  },
  {
    extends: "./vitest.config.ts",
    test: {
      name: "dom",
      environment: "jsdom",
      include: ["tests/**/*.test.{ts,tsx}"],
      exclude: [
        "**/node_modules/**",
        ".claude/**",
        "tests/{config,contracts,i18n,scripts,utils,lib}/**/*.test.ts",
      ],
    },
  },
]);
