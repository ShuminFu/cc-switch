// tauri-driver + WebdriverIO smoke for the Linux build (CCS-201).
//
// Runs against a real CC Switch binary through tauri-driver, which proxies
// WebDriver to WebKitWebDriver (package webkit2gtk-driver). Needs an X display,
// so run it under xvfb-run on CI.
//
// Locally (Linux; needs webkit2gtk-driver and `cargo install tauri-driver`):
//   pnpm tauri build --debug --no-bundle
//   pnpm install --dir e2e --ignore-workspace
//   xvfb-run -a pnpm --dir e2e test
//
// - CC_SWITCH_E2E_APP: binary or AppImage to drive. Defaults to the debug build
//   from `pnpm tauri build --debug --no-bundle`.
// - TAURI_DRIVER: tauri-driver executable (default: ~/.cargo/bin/tauri-driver,
//   then PATH).
//
// The app runs with HOME pointed at a fresh temp dir, so the smoke never reads
// or writes the real ~/.claude or ~/.cc-switch.
import { spawn } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { SevereServiceError } from "webdriverio";

const here = path.dirname(fileURLToPath(import.meta.url));

const appPath = path.resolve(
  process.env.CC_SWITCH_E2E_APP ??
    path.join(here, "../src-tauri/target/debug/cc-switch"),
);

function resolveTauriDriver() {
  if (process.env.TAURI_DRIVER) return process.env.TAURI_DRIVER;
  const cargoBin = path.join(os.homedir(), ".cargo", "bin", "tauri-driver");
  return fs.existsSync(cargoBin) ? cargoBin : "tauri-driver";
}

// The launcher and every worker load this file; workers inherit the launcher's
// environment, so the temp HOME is created once and shared.
if (!process.env.CC_SWITCH_E2E_HOME) {
  process.env.CC_SWITCH_E2E_HOME = fs.mkdtempSync(
    path.join(os.tmpdir(), "cc-switch-e2e-home-"),
  );
}
const e2eHome = process.env.CC_SWITCH_E2E_HOME;
// First on the app's PATH, so a spec can put stub CLIs (e.g. a fake `claude`)
// in front of anything installed on the runner. Empty unless a spec fills it.
process.env.CC_SWITCH_E2E_BIN ??= path.join(e2eHome, "bin");
const e2eBin = process.env.CC_SWITCH_E2E_BIN;
const artifactsDir = path.join(here, "artifacts");

// Environment for tauri-driver -> WebKitWebDriver -> the app. Provider env vars
// from the runner would trip the env-conflict banner and leak into the app.
function appEnv() {
  const env = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (/^(ANTHROPIC|OPENAI|GEMINI|GOOGLE|CLAUDE|CODEX)_/.test(key)) continue;
    env[key] = value;
  }
  return {
    ...env,
    PATH: [e2eBin, env.PATH].filter(Boolean).join(path.delimiter),
    HOME: e2eHome,
    CC_SWITCH_TEST_HOME: e2eHome,
    XDG_CONFIG_HOME: path.join(e2eHome, ".config"),
    XDG_DATA_HOME: path.join(e2eHome, ".local", "share"),
    XDG_CACHE_HOME: path.join(e2eHome, ".cache"),
    // Lets an AppImage run without FUSE on CI runners.
    APPIMAGE_EXTRACT_AND_RUN: "1",
    NO_CLEANUP: "1",
  };
}

const DRIVER_PORT = 4444;

let tauriDriver;
let tauriDriverExited = false;

// tauri-driver does not stop its WebKitWebDriver child (not even when it dies
// itself), so it runs in its own process group and the whole group is killed.
function stopTauriDriver() {
  if (!tauriDriver?.pid) return;
  try {
    process.kill(-tauriDriver.pid, "SIGTERM");
  } catch {
    // Group already gone.
  }
}

function portInUse(port) {
  return new Promise((resolve) => {
    const socket = net.connect({ host: "127.0.0.1", port });
    socket.once("connect", () => {
      socket.destroy();
      resolve(true);
    });
    socket.once("error", () => resolve(false));
  });
}

export const config = {
  runner: "local",
  // The smoke goes first: it expects a first run on the fresh HOME.
  specs: ["./specs/smoke.e2e.mjs", "./specs/**/*.e2e.mjs"],
  maxInstances: 1,
  hostname: "127.0.0.1",
  port: DRIVER_PORT,
  capabilities: [
    {
      maxInstances: 1,
      "tauri:options": { application: appPath },
    },
  ],
  logLevel: "warn",
  bail: 1,
  waitforTimeout: 20_000,
  connectionRetryTimeout: 60_000,
  connectionRetryCount: 1,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    // Each step builds on the previous one; stop at the first failure.
    bail: true,
    timeout: 180_000,
  },

  // Runs in the launcher. A SevereServiceError aborts the run right away;
  // any other error would only be logged.
  async onPrepare() {
    if (!fs.existsSync(appPath)) {
      throw new SevereServiceError(
        `App binary not found: ${appPath} (build it with \`pnpm tauri build --debug --no-bundle\` or set CC_SWITCH_E2E_APP)`,
      );
    }
    // A leftover driver would serve the session for a different app/HOME.
    if (await portInUse(DRIVER_PORT)) {
      throw new SevereServiceError(
        `127.0.0.1:${DRIVER_PORT} is already in use (leftover tauri-driver?)`,
      );
    }
    // Pin the UI language and show the proxy takeover switch on the main
    // page. firstRunNoticeConfirmed stays unset, so this is still a first run.
    const settingsDir = path.join(e2eHome, ".cc-switch");
    fs.mkdirSync(settingsDir, { recursive: true });
    fs.writeFileSync(
      path.join(settingsDir, "settings.json"),
      JSON.stringify({ language: "en", enableLocalProxy: true }, null, 2),
    );
    console.log(`[e2e] app: ${appPath}`);
    console.log(`[e2e] HOME: ${e2eHome}`);

    tauriDriver = spawn(resolveTauriDriver(), [], {
      env: appEnv(),
      detached: true,
      stdio: ["ignore", "inherit", "inherit"],
    });
    process.once("exit", stopTauriDriver);
    let spawnError;
    tauriDriver.on("error", (error) => {
      spawnError = error;
      tauriDriverExited = true;
    });
    tauriDriver.on("exit", (code) => {
      tauriDriverExited = true;
      if (code) console.error(`[e2e] tauri-driver exited with code ${code}`);
    });
    // tauri-driver has no readiness signal; wait for it to accept connections.
    const deadline = Date.now() + 15_000;
    while (!(await portInUse(DRIVER_PORT))) {
      if (tauriDriverExited || Date.now() > deadline) {
        stopTauriDriver();
        throw new SevereServiceError(
          `tauri-driver did not start${spawnError ? `: ${spawnError.message}` : ""}`,
        );
      }
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
  },

  // Keep a screenshot and the page text of a failing step for the CI artifact.
  async afterTest(test, _context, { passed }) {
    if (passed) return;
    fs.mkdirSync(artifactsDir, { recursive: true });
    const name = test.title.replace(/[^a-z0-9]+/gi, "-").toLowerCase();
    await browser
      .saveScreenshot(path.join(artifactsDir, `${name}.png`))
      .catch(() => {});
    const text = await browser
      .execute(() => document.body.innerText)
      .catch(() => "");
    fs.writeFileSync(path.join(artifactsDir, `${name}.txt`), String(text));
  },

  onComplete() {
    stopTauriDriver();
    if (!process.env.CC_SWITCH_E2E_KEEP_HOME) {
      fs.rmSync(e2eHome, { recursive: true, force: true });
    }
  },
};
