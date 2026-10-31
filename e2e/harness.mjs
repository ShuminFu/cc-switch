// Playwright harness for the renderer (TEST-17, phase A).
//
// Starts the real Vite dev server (vite.config.ts) on a free port with one
// extra plugin that loads e2e/ipc-mock.ts before src/main.tsx, so the app
// talks to a mocked Tauri IPC backed by the vitest MSW handlers. Journeys run
// with node:test (`pnpm test:e2e`); on failure a screenshot and a Playwright
// trace are written to e2e/artifacts/.
import { createRequire } from "node:module";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const ARTIFACTS = path.join(ROOT, "e2e", "artifacts");
const MOCK_ENTRY = path.join(ROOT, "e2e", "ipc-mock.ts");

/** Playwright is provided by the environment (CI installs it globally). */
function loadPlaywright() {
  try {
    return require("playwright");
  } catch {
    throw new Error(
      "playwright is not installed. Install it with `npm i -g playwright@1` " +
        "and `npx playwright install chromium`, or set NODE_PATH.",
    );
  }
}

async function freePort() {
  return await new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.unref();
    srv.on("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

function ipcMockPlugin() {
  return {
    name: "cc-switch-e2e-ipc-mock",
    transformIndexHtml(html) {
      const src = `/@fs/${MOCK_ENTRY.replace(/\\/g, "/").replace(/^\//, "")}`;
      // Module scripts run in document order: the mock is installed before
      // the app's entry evaluates.
      return html.replace(
        /<script type="module" src="\.\/main\.tsx"><\/script>/,
        `<script type="module" src="${src}"></script>\n    $&`,
      );
    },
  };
}

export async function startApp() {
  const { createServer } = await import("vite");
  const { default: react } = await import("@vitejs/plugin-react");
  const port = Number(process.env.E2E_PORT) || (await freePort());
  // Mirrors the serve-relevant parts of vite.config.ts (root, React, the `@`
  // alias, env prefix) without the dev-only code-inspector plugin.
  const server = await createServer({
    configFile: false,
    root: path.join(ROOT, "src"),
    base: "./",
    plugins: [react(), ipcMockPlugin()],
    resolve: { alias: { "@": path.join(ROOT, "src") } },
    envPrefix: ["VITE_"],
    server: {
      port,
      strictPort: true,
      host: "127.0.0.1",
      hmr: false,
      fs: { allow: [ROOT] },
    },
    clearScreen: false,
    logLevel: "warn",
  });
  await server.listen();
  const url = `http://127.0.0.1:${port}/`;

  const { chromium } = loadPlaywright();
  const browser = await chromium.launch();

  return {
    url,
    browser,
    async close() {
      await browser.close();
      await server.close();
    },
  };
}

/**
 * Open a fresh page with tracing. `run(page)` is the journey; on failure the
 * screenshot and trace are kept under e2e/artifacts/<name>.*.
 */
export async function withPage(app, name, run, { locale = "en-US" } = {}) {
  const context = await app.browser.newContext({
    locale,
    viewport: { width: 1280, height: 860 },
  });
  // English UI regardless of the runner's system language.
  await context.addInitScript(() => {
    try {
      window.localStorage.setItem("language", "en");
    } catch {
      /* storage blocked */
    }
  });
  await context.tracing.start({ screenshots: true, snapshots: true });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (err) => errors.push(err));
  try {
    await page.goto(app.url);
    await page.waitForFunction(() => window.__E2E__ !== undefined);
    await run(page, { errors });
    if (errors.length) {
      throw new Error(`uncaught page errors: ${errors.map(String).join("; ")}`);
    }
    await context.tracing.stop();
  } catch (err) {
    fs.mkdirSync(ARTIFACTS, { recursive: true });
    const base = path.join(ARTIFACTS, name.replace(/[^a-z0-9-]+/gi, "_"));
    await page
      .screenshot({ path: `${base}.png`, fullPage: true })
      .catch(() => {});
    await context.tracing.stop({ path: `${base}.trace.zip` }).catch(() => {});
    throw err;
  } finally {
    await context.close();
  }
}

/** Every invoke the page made, optionally filtered by command. */
export async function invokes(page, cmd) {
  const calls = await page.evaluate(() => window.__E2E__.calls);
  return cmd ? calls.filter((c) => c.cmd === cmd) : calls;
}
