// Headless smoke test for the built Dioxus bundle.
//
//   cd crates/cc-switch-ui && dx build --platform web --release
//   node e2e/smoke.mjs [path-to-public-dir]
//
// Requires `playwright-core` resolvable via NODE_PATH and a Chromium binary at
// $CHROMIUM (defaults to the Playwright-managed one).
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright-core");

const here = path.dirname(fileURLToPath(import.meta.url));
const publicDir = path.resolve(
  process.argv[2] ?? path.join(here, "../../target/dx/cc-switch-ui/release/web/public"),
);
const mock = fs.readFileSync(path.join(here, "tauri-mock.js"), "utf8");

const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".css": "text/css" };
const server = http.createServer((req, res) => {
  let file = path.join(publicDir, decodeURIComponent(new URL(req.url, "http://x").pathname));
  if (!fs.existsSync(file) || fs.statSync(file).isDirectory()) file = path.join(publicDir, "index.html");
  res.setHeader("Content-Type", types[path.extname(file)] ?? "application/octet-stream");
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--no-sandbox"],
});
let failed = false;
try {
  const page = await browser.newPage({ viewport: { width: 1000, height: 650 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  await page.addInitScript(mock);
  await page.goto(`${base}/`);
  await page.getByText("Backend settings loaded").waitFor({ timeout: 20000 });
  const text = await page.locator("main").innerText();
  const expect = (cond, msg) => { if (!cond) { failed = true; console.error("FAIL:", msg); } else console.log("ok:", msg); };
  expect(text.includes("Language: en"), "language rendered from backend settings");
  expect(text.includes("Local proxy: true"), "boolean field rendered");
  expect(/Visible apps:.*claude.*codex.*gemini/s.test(text), "visible apps filtered by visibleApps");
  expect(!text.includes("claude-desktop"), "hidden app not rendered");
  const calls = await page.evaluate(() => window.__TAURI_MOCK__.calls.map((c) => c.cmd));
  expect(calls.includes("get_settings"), `get_settings invoked (calls: ${calls.join(",")})`);
  expect(errors.length === 0, `no page errors (${errors.join(" | ")})`);
  await page.screenshot({ path: path.join(process.env.SMOKE_OUT ?? here, "smoke.png") });
} finally {
  await browser.close();
  server.close();
}
process.exit(failed ? 1 : 0);
