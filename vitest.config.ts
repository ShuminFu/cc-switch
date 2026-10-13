import path from "node:path";
import { configDefaults, defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./tests/setupGlobals.ts", "./tests/setupTests.ts"],
    globals: true,
    // Agent worktrees under .claude/ contain full repo copies; never collect their tests.
    exclude: [...configDefaults.exclude, ".claude/**"],
    coverage: {
      reporter: ["text", "lcov"],
    },
  },
});
