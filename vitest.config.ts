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
    // Local agent worktrees under .claude/ carry their own copies of the tests.
    exclude: [...configDefaults.exclude, ".claude/**"],
    coverage: {
      // `pnpm test:coverage`; reporting only for now (no thresholds yet).
      provider: "v8",
      reporter: ["text-summary", "lcov"],
      reportsDirectory: "./coverage",
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/components/ui/**", "src/i18n/**", "**/*.d.ts"],
    },
  },
});
