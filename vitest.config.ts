import path from "node:path";
import { defineConfig } from "vitest/config";
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
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/**/*.d.ts"],
    },
  },
});
