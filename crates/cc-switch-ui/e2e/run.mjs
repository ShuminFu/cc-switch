// Runs every scenario in e2e/scenarios/*.mjs against the built bundle with the
// Tauri mock (tauri-mock.js + every e2e/handlers/*.js) injected.
//
//   cd crates/cc-switch-ui && dx build --platform web --release && node e2e/run.mjs [scenario-name]
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright-core");
const here = path.dirname(fileURLToPath(import.meta.url));
const publicDir = path.resolve(process.env.PUBLIC_DIR ?? path.join(here, "../../target/dx/cc-switch-ui/release/web/public"));
const handlersDir = path.join(here, "handlers");
const mock = [path.join(here, "tauri-mock.js"), ...fs.readdirSync(handlersDir).filter((f) => f.endsWith(".js")).sort().map((f) => path.join(handlersDir, f))]
  .map((f) => fs.readFileSync(f, "utf8"))
  .join("\n;\n");

const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".css": "text/css", ".png": "image/png", ".svg": "image/svg+xml" };
const server = http.createServer((req, res) => {
  let file = path.join(publicDir, decodeURIComponent(new URL(req.url, "http://x").pathname));
  if (!fs.existsSync(file) || fs.statSync(file).isDirectory()) file = path.join(publicDir, "index.html");
  res.setHeader("Content-Type", types[path.extname(file)] ?? "application/octet-stream");
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome", args: ["--no-sandbox"] });

const only = process.argv[2];
const scenarios = fs.readdirSync(path.join(here, "scenarios")).filter((f) => f.endsWith(".mjs") && (!only || f.startsWith(only))).sort();
let failures = 0;
for (const file of scenarios) {
  const { run } = await import(pathToFileURL(path.join(here, "scenarios", file)).href);
  const context = await browser.newContext({ viewport: { width: 1000, height: 650 } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  await page.addInitScript(mock);
  let scenarioFailed = false;
  const expect = (cond, msg) => { if (!cond) { scenarioFailed = true; console.error(`  FAIL [${file}]: ${msg}`); } else console.log(`  ok [${file}]: ${msg}`); };
  console.log(`== ${file}`);
  try {
    await page.goto(`${base}/`);
    await run({ page, expect, errors, base });
    expect(errors.length === 0, `no page errors (${errors.join(" | ")})`);
  } catch (err) {
    scenarioFailed = true;
    console.error(`  FAIL [${file}]: ${err.message.split("\n")[0]}`);
    try { await page.screenshot({ path: path.join(process.env.SMOKE_OUT ?? here, `failure-${file}.png`) }); } catch {}
  }
  if (scenarioFailed) failures++;
  await context.close();
}
await browser.close();
server.close();
console.log(failures ? `${failures} scenario(s) failed` : `all ${scenarios.length} scenario(s) passed`);
process.exit(failures ? 1 : 0);
