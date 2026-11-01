import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { codeInspectorPlugin } from "code-inspector-plugin";

export default defineConfig(({ command }) => ({
  root: "src",
  plugins: [
    command === "serve" &&
      codeInspectorPlugin({
        bundler: "vite",
      }),
    react(),
  ].filter(Boolean),
  base: "./",
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    rollupOptions: {
      output: {
        // Heavy libraries used only by lazily loaded pages/dialogs get their
        // own chunks, so they are fetched once on demand and shared between
        // the pages that need them instead of being inlined into one of them
        // (PERF-15). React and shared helpers go in a stable "vendor-core" chunk.
        manualChunks(id) {
          // Small helpers shared by startup code and the lazy vendor chunks
          // (clsx is used by `cn()` and by recharts, the CommonJS interop
          // helper by several CJS deps) must be pinned to the eager core
          // chunk; otherwise Rollup folds them into a lazy vendor chunk and
          // the entry has to load that whole chunk at startup.
          if (id.includes("commonjsHelpers")) return "vendor-core";
          if (!id.includes("node_modules")) return undefined;
          if (
            /[\\/]node_modules[\\/](@codemirror|@lezer|codemirror|@uiw)[\\/]/.test(
              id,
            )
          ) {
            return "vendor-codemirror";
          }
          if (
            /[\\/]node_modules[\\/](recharts|recharts-scale|victory-vendor|d3-[^\\/]+|internmap|decimal\.js-light)[\\/]/.test(
              id,
            )
          ) {
            return "vendor-recharts";
          }
          if (
            /[\\/]node_modules[\\/](react|react-dom|scheduler|clsx)[\\/]/.test(
              id,
            )
          ) {
            return "vendor-core";
          }
          return undefined;
        },
      },
    },
  },
  server: {
    port: 3000,
    strictPort: true,
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  // Only VITE_* variables are exposed to client code. Never widen this to
  // TAURI_*: the release build runs with TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)
  // in its environment and Vite would inline any referenced match into dist/.
  envPrefix: ["VITE_"],
}));
