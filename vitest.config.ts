import path from "node:path";
import { coverageConfigDefaults, defineConfig } from "vitest/config";
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
    coverage: {
      // Report-only (CCS-019): no thresholds until the P5 ratchet.
      provider: "v8",
      reporter: ["text-summary", "lcov", "json-summary"],
      reportsDirectory: "./coverage/frontend",
      // Still write the report when a test fails, so CI keeps the artifact.
      reportOnFailure: true,
      include: ["src/**/*.{ts,tsx}"],
      // Keep the defaults (test files, *.d.ts, ...); src/ has colocated tests.
      exclude: [...coverageConfigDefaults.exclude],
    },
  },
});
