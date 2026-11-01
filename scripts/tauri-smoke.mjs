#!/usr/bin/env node
// Smoke test of the real Tauri binary through tauri-driver (TEST-17 phase B,
// PRODUCT-45). Linux only; run by the nightly workflow under xvfb:
//
//   cargo install tauri-driver --locked
//   pnpm tauri build --debug --no-bundle
//   xvfb-run -a node scripts/tauri-smoke.mjs src-tauri/target/debug/cc-switch
//
// It starts tauri-driver (which drives WebKitWebDriver), launches the app with
// HOME pointed at a throw-away directory seeded with a Claude Code config, and
// checks that:
//   1. the main window renders the app shell,
//   2. the database is created under $HOME/.cc-switch/,
//   3. the seeded ~/.claude/settings.json is still byte-identical after start
//      (AGENTS.md rule 2: never clobber a live config the app did not change).
// Plain W3C WebDriver over fetch: no WebdriverIO dependency.
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const binary = path.resolve(
  process.argv[2] ?? "src-tauri/target/debug/cc-switch",
);
const DRIVER_PORT = Number(process.env.TAURI_DRIVER_PORT ?? 4444);
const DRIVER = `http://127.0.0.1:${DRIVER_PORT}`;
const TIMEOUT_MS = 60_000;

if (process.platform !== "linux") {
  console.error("tauri-smoke: tauri-driver smoke runs on Linux only");
  process.exit(2);
}
if (!fs.existsSync(binary)) {
  console.error(`tauri-smoke: binary not found: ${binary}`);
  process.exit(2);
}

const home = fs.mkdtempSync(path.join(os.tmpdir(), "cc-switch-smoke-"));
const claudeDir = path.join(home, ".claude");
fs.mkdirSync(claudeDir, { recursive: true });
const seededSettings = `${JSON.stringify(
  {
    env: {
      ANTHROPIC_BASE_URL: "https://smoke.example.invalid",
      ANTHROPIC_AUTH_TOKEN: "sk-smoke-placeholder",
    },
    permissions: { allow: ["Bash(ls:*)"] },
  },
  null,
  2,
)}\n`;
fs.writeFileSync(path.join(claudeDir, "settings.json"), seededSettings);

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function wd(method, url, body) {
  const res = await fetch(`${DRIVER}${url}`, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await res.json().catch(() => ({}));
  if (!res.ok) {
    throw new Error(
      `${method} ${url} -> ${res.status} ${JSON.stringify(json.value ?? json)}`,
    );
  }
  return json.value;
}

async function waitFor(what, check) {
  const deadline = Date.now() + TIMEOUT_MS;
  let last;
  while (Date.now() < deadline) {
    try {
      const value = await check();
      if (value) return value;
    } catch (error) {
      last = error;
    }
    await sleep(500);
  }
  throw new Error(`timed out waiting for ${what}${last ? `: ${last}` : ""}`);
}

const driver = spawn("tauri-driver", ["--port", String(DRIVER_PORT)], {
  stdio: "inherit",
  env: { ...process.env, HOME: home },
});
let session;
let failed = false;
try {
  await waitFor("tauri-driver", () => fetch(`${DRIVER}/status`).then(() => 1));
  const created = await wd("POST", "/session", {
    capabilities: {
      alwaysMatch: { "tauri:options": { application: binary } },
    },
  });
  session = created.sessionId;

  const text = await waitFor("the app shell", async () => {
    const body = await wd("POST", `/session/${session}/execute/sync`, {
      script: "return document.body ? document.body.innerText : ''",
      args: [],
    });
    return /CC Switch/.test(body) ? body : null;
  });
  console.log(`ok - app shell rendered (${text.length} chars of text)`);

  await waitFor("the database", () =>
    fs.existsSync(path.join(home, ".cc-switch", "cc-switch.db")),
  );
  console.log("ok - ~/.cc-switch/cc-switch.db created");

  const after = fs.readFileSync(path.join(claudeDir, "settings.json"), "utf8");
  if (after !== seededSettings) {
    throw new Error("~/.claude/settings.json changed on startup");
  }
  console.log("ok - seeded ~/.claude/settings.json left byte-identical");
} catch (error) {
  failed = true;
  console.error(`not ok - ${error instanceof Error ? error.message : error}`);
} finally {
  if (session) {
    await wd("DELETE", `/session/${session}`).catch(() => {});
  }
  driver.kill();
  fs.rmSync(home, { recursive: true, force: true });
}
process.exit(failed ? 1 : 0);
