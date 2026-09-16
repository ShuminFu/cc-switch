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
  const header = page.locator("header");
  await header.getByText("Claude Code").waitFor({ timeout: 20000 });
  const expect = (cond, msg) => { if (!cond) { failed = true; console.error("FAIL:", msg); } else console.log("ok:", msg); };
  const switcher = await header.innerText();
  expect(/Claude Code[\s\S]*Codex[\s\S]*Gemini/.test(switcher), "app switcher lists the visible apps in order");
  expect(!switcher.includes("Claude Desktop"), "hidden app (visibleApps) is not listed");
  expect(!switcher.includes("OpenClaw"), "hidden OpenClaw is not listed");
  // Header actions are translated (English because the mocked settings say so).
  expect((await header.locator("button[title='Settings']").count()) === 1, "settings button rendered with English title");
  await header.locator("button[title='Settings']").click();
  await page.locator("header h1", { hasText: "Settings" }).waitFor({ timeout: 5000 });
  expect(await page.evaluate(() => localStorage.getItem("cc-switch-last-view")) === "settings", "view persisted under the React storage key");
  await header.locator("button[title='Back']").click();
  await header.getByText("Claude Code").waitFor({ timeout: 5000 });
  // Provider list for Claude: sorted by sortIndex, current badge, partner badge.
  const cards = page.locator("main ul > li");
  await cards.first().waitFor({ timeout: 10000 });
  const names = await cards.locator("span.font-semibold").allInnerTexts();
  expect(names[0] === "Kimi For Coding" && names[1] === "Anthropic Official", `providers sorted by sortIndex (${names.join(" | ")})`);
  expect((await cards.nth(1).innerText()).includes("Currently Using"), "current provider badge on the active provider");
  expect((await cards.nth(0).innerText()).includes("Official Partner"), "partner badge from meta.isPartner");
  expect((await cards.nth(0).locator("img, span[data-icon], span[title='Kimi For Coding'] svg").count()) > 0, "provider icon rendered");
  await cards.nth(0).getByRole("button", { name: "Enable" }).click();
  await cards.nth(0).getByText("Currently Using").waitFor({ timeout: 5000 });
  const switched = await page.evaluate(() => window.__TAURI_MOCK__.calls.filter((c) => c.cmd === "switch_provider").map((c) => c.args));
  expect(switched.length === 1 && switched[0].id === "p-kimi" && switched[0].app === "claude", `switch_provider invoked with id/app (${JSON.stringify(switched)})`);
  await page.locator("main input[type=search]").fill("team");
  expect((await cards.count()) === 1, "search filters by notes");
  await page.locator("main input[type=search]").fill("");
  await header.getByText("Codex").click();
  expect(await page.evaluate(() => localStorage.getItem("cc-switch-last-app")) === "codex", "active app persisted under the React storage key");
  await page.getByText("No providers added yet").waitFor({ timeout: 5000 });
  const htmlClass = await page.evaluate(() => document.documentElement.className);
  expect(/\b(light|dark)\b/.test(htmlClass), `theme class applied to <html> (${htmlClass})`);
  const calls = await page.evaluate(() => window.__TAURI_MOCK__.calls.map((c) => c.cmd));
  expect(calls.includes("get_settings"), `get_settings invoked (calls: ${calls.join(",")})`);
  expect(calls.includes("set_window_theme"), "native window theme synced");
  expect(errors.length === 0, `no page errors (${errors.join(" | ")})`);
  await page.screenshot({ path: path.join(process.env.SMOKE_OUT ?? here, "smoke.png") });
} finally {
  await browser.close();
  server.close();
}
process.exit(failed ? 1 : 0);
