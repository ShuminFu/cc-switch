// Linux session resume (CCS-248, automated check for CCS-184): Resume in the
// session manager opens the preferred terminal (xterm) in the session's cwd
// running `claude --resume <id>`, the temp launch script deletes itself and
// the window stays open on the user's shell.
//
// A fake `claude` on the app's PATH (CC_SWITCH_E2E_BIN, see wdio.conf.mjs)
// records `pwd -P` and its arguments instead of starting Claude Code.
//
// Negative control: drop the `cd` from build_user_shell_command_line or the
// `exec` line from build_posix_resume_script
// (src-tauri/src/platform/terminal/) and rebuild; the cwd or the
// "window stays open" step then fails.
import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const home = process.env.CC_SWITCH_E2E_HOME;
const binDir = process.env.CC_SWITCH_E2E_BIN;
const fakeClaude = path.join(binDir, "claude");
const marker = path.join(home, "ccs248-claude-invocation.txt");
// The launch script the fake found among its ancestors, and whether it still
// existed while the resume command ran.
const scriptMarker = `${marker}.script`;

const SESSION_ID = randomUUID();
const SESSION_TITLE = `ccs248 resume probe ${SESSION_ID.slice(0, 8)}`;
// How long the terminal must outlive the resume command.
const STAYS_OPEN_MS = 2_000;

let sessionDir;
let projectDir;
let previousTerminal;
let terminalPids = [];

function shellQuote(value) {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

function writeFakeClaude() {
  fs.mkdirSync(binDir, { recursive: true });
  // Version probes (About page, tool detection) must not count as a resume.
  fs.writeFileSync(
    fakeClaude,
    [
      "#!/bin/sh",
      'if [ "$1" = "--version" ]; then echo "0.0.0 (fake Claude Code)"; exit 0; fi',
      // xterm -> sh <script> -> user shell -> claude: find the script path.
      "pid=$PPID",
      'while [ "${pid:-1}" -gt 1 ]; do',
      '  script=$(tr "\\0" "\\n" < "/proc/$pid/cmdline" | grep "cc_switch_resume_.*\\.sh$")',
      '  [ -n "$script" ] && break',
      "  pid=$(sed -n 's/^PPid:[[:space:]]*//p' \"/proc/$pid/status\")",
      "done",
      `{ printf '%s\\n' "$script"; [ -f "$script" ] && echo present; } > ${shellQuote(scriptMarker)}`,
      `{ pwd -P; for arg in "$@"; do printf '[%s]\\n' "$arg"; done; } > ${shellQuote(`${marker}.tmp`)}`,
      `mv ${shellQuote(`${marker}.tmp`)} ${shellQuote(marker)}`,
      "",
    ].join("\n"),
    { mode: 0o755 },
  );
}

// Claude Code stores a session under projects/<cwd with every
// non-alphanumeric character replaced by "-">/<session id>.jsonl.
function writeSessionLog() {
  sessionDir = fs.mkdtempSync(path.join(os.tmpdir(), "ccs248 resume (dir) "));
  projectDir = path.join(
    home,
    ".claude",
    "projects",
    sessionDir.replace(/[^a-zA-Z0-9]/g, "-"),
  );
  fs.mkdirSync(projectDir, { recursive: true });
  const now = Date.now();
  const lines = [
    {
      type: "user",
      sessionId: SESSION_ID,
      cwd: sessionDir,
      timestamp: new Date(now - 60_000).toISOString(),
      message: { role: "user", content: SESSION_TITLE },
    },
    {
      type: "assistant",
      sessionId: SESSION_ID,
      cwd: sessionDir,
      timestamp: new Date(now).toISOString(),
      message: {
        role: "assistant",
        content: [{ type: "text", text: "ccs248 resume probe reply" }],
      },
    },
  ];
  fs.writeFileSync(
    path.join(projectDir, `${SESSION_ID}.jsonl`),
    `${lines.map((line) => JSON.stringify(line)).join("\n")}\n`,
  );
}

// Every live process as { pid, ppid, comm }.
function processes() {
  const list = [];
  for (const entry of fs.readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue;
    try {
      const stat = fs.readFileSync(`/proc/${entry}/stat`, "utf8");
      // comm is in parentheses and may itself contain spaces or ")".
      const close = stat.lastIndexOf(")");
      const comm = stat.slice(stat.indexOf("(") + 1, close);
      const [state, ppid] = stat.slice(close + 2).split(" ");
      if (state === "Z") continue;
      list.push({ pid: Number(entry), ppid: Number(ppid), comm });
    } catch {
      // Exited while listing.
    }
  }
  return list;
}

const xtermPids = () =>
  new Set(
    processes()
      .filter((proc) => proc.comm === "xterm")
      .map((proc) => proc.pid),
  );

// Launch scripts go to $XDG_RUNTIME_DIR when it is an absolute dir, otherwise
// to <app config dir>/tmp (private_launch_temp_dir).
function launchScriptDirs() {
  const dirs = [path.join(home, ".cc-switch", "tmp")];
  const runtimeDir = process.env.XDG_RUNTIME_DIR;
  if (runtimeDir && path.isAbsolute(runtimeDir)) dirs.push(runtimeDir);
  return dirs;
}

const invoke = (command, args = {}) =>
  browser.executeAsync(
    (name, payload, done) => {
      window.__TAURI_INTERNALS__.invoke(name, payload).then(
        (value) => done({ value }),
        (error) => done({ error: String(error?.message ?? error) }),
      );
    },
    command,
    args,
  );

describe("Session resume on Linux", () => {
  before(function () {
    if (process.platform !== "linux") this.skip();
    try {
      execFileSync("sh", ["-c", "command -v xterm"], { stdio: "ignore" });
    } catch {
      throw new Error("xterm is required for this spec (apt install xterm)");
    }
  });

  after(async () => {
    for (const pid of terminalPids) {
      try {
        process.kill(pid, "SIGTERM");
      } catch {
        // Already gone.
      }
    }
    // The app reopens on the last view; later specs start from the main one.
    const back = await $('button[aria-label="Back"]');
    if (await back.isDisplayed().catch(() => false)) {
      await back.click();
      await $('button[title="Session Manager"]').waitForDisplayed();
    }
    if (previousTerminal !== undefined) {
      const current = await invoke("get_settings").catch(() => null);
      if (current?.value) {
        await invoke("save_settings", {
          settings: { ...current.value, preferredTerminal: previousTerminal },
        }).catch(() => {});
      }
    }
    fs.rmSync(fakeClaude, { force: true });
    fs.rmSync(marker, { force: true });
    fs.rmSync(scriptMarker, { force: true });
    if (projectDir) fs.rmSync(projectDir, { recursive: true, force: true });
    if (sessionDir) fs.rmSync(sessionDir, { recursive: true, force: true });
  });

  it("prefers xterm and finds the seeded session", async () => {
    const nav = await $('button[title="Session Manager"]');
    await nav.waitForDisplayed({ timeout: 60_000 });
    // Only shown when this spec runs on a fresh HOME.
    const notice = await $('[data-testid="first-run-notice-confirm"]');
    if (await notice.isDisplayed()) {
      await notice.click();
      await notice.waitForExist({ reverse: true });
    }

    // xterm is not in the built-in Linux list, so it is launched with `-e`.
    const settings = await invoke("get_settings");
    expect(settings.error).toBeUndefined();
    previousTerminal = settings.value.preferredTerminal ?? null;
    const saved = await invoke("save_settings", {
      settings: { ...settings.value, preferredTerminal: "xterm" },
    });
    expect(saved).toEqual({ value: true });

    writeFakeClaude();
    writeSessionLog();
    fs.rmSync(marker, { force: true });
    fs.rmSync(scriptMarker, { force: true });

    await nav.click();
    const item = await $(`//button[contains(., "${SESSION_TITLE}")]`);
    await item.waitForClickable({ timeout: 30_000 });
    await item.click();
    // The resume command preview of the selected session.
    await expect(
      $(
        `//div[contains(@class, "font-mono")][normalize-space(.)="claude --resume ${SESSION_ID}"]`,
      ),
    ).toBeExisting({ wait: 10_000 });
  });

  it("resumes the session in xterm in the session directory", async () => {
    const before = xtermPids();

    const resume = await $('//button[contains(., "Resume Session")]');
    await resume.waitForClickable();
    await resume.click();

    await browser.waitUntil(() => fs.existsSync(marker), {
      timeout: 30_000,
      interval: 250,
      timeoutMsg: `the fake claude was not run (${marker})`,
    });

    terminalPids = [...xtermPids()].filter((pid) => !before.has(pid));
    console.log(
      `[e2e] session dir: ${sessionDir}; new xterm pids: ${terminalPids.join(", ")}`,
    );
    expect(terminalPids).toHaveLength(1);

    // cwd is the session dir (space and parentheses intact) and the id
    // arrives as the only argument after --resume.
    expect(fs.readFileSync(marker, "utf8")).toBe(
      `${fs.realpathSync(sessionDir)}\n[--resume]\n[${SESSION_ID}]\n`,
    );

    await expect(
      $('//*[@data-sonner-toast][contains(., "Terminal launched")]'),
    ).toBeExisting({ wait: 10_000 });
    await expect(
      $('[data-sonner-toast][data-type="error"]'),
    ).not.toBeExisting();
  });

  it("deletes the launch script and keeps the terminal open", async () => {
    // The resume command ran from a cc_switch_resume_*.sh in the private
    // launch dir, which still existed at that point.
    const [script, present] = fs.readFileSync(scriptMarker, "utf8").split("\n");
    console.log(`[e2e] launch script: ${script}`);
    expect(path.basename(script)).toMatch(/^cc_switch_resume_.*\.sh$/);
    expect(launchScriptDirs()).toContain(path.dirname(script));
    expect(present).toBe("present");

    await browser.waitUntil(() => !fs.existsSync(script), {
      timeout: 10_000,
      interval: 250,
      timeoutMsg: `resume script left behind: ${script}`,
    });

    await browser.pause(STAYS_OPEN_MS);
    const [xterm] = terminalPids;
    const all = processes();
    expect(all.some((proc) => proc.pid === xterm)).toBe(true);

    // The launch script exec'd the user's shell, which waits on the terminal
    // in the session directory.
    const children = all.filter((proc) => proc.ppid === xterm);
    expect(children).toHaveLength(1);
    const [shell] = children;
    console.log(`[e2e] xterm ${xterm} -> ${shell.comm} (${shell.pid})`);
    expect(fs.readlinkSync(`/proc/${shell.pid}/cwd`)).toBe(
      fs.realpathSync(sessionDir),
    );
    const cmdline = fs
      .readFileSync(`/proc/${shell.pid}/cmdline`, "utf8")
      .split("\0");
    expect(cmdline.some((arg) => arg.includes("cc_switch_resume_"))).toBe(
      false,
    );
  });
});
