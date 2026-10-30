// Session-log sync responsiveness (CCS-224, follow-up of CCS-038): a manual
// sync over ~200 MB of Claude session logs must run off the main thread, so
// the window keeps answering WebDriver and keeps painting frames meanwhile.
//
// Negative control: turn `sync_session_usage` (src-tauri/src/commands/usage.rs)
// into a plain `pub fn` that calls `sync_all_session_usage(&state.db)`; Tauri
// then runs it on the main thread and this spec fails.
import fs from "node:fs";
import path from "node:path";

const home = process.env.CC_SWITCH_E2E_HOME;
const projectDir = path.join(
  home,
  ".claude",
  "projects",
  "-home-user-ccs224-session-sync",
);

const FILES = 300;
const BYTES_PER_FILE = 700 * 1024;
// Largest WebDriver round trip / animation-frame gap tolerated during the sync.
const MAX_STALL_MS = 250;
const SYNC_TIMEOUT_MS = 150_000;

// Same shape as the session_usage.rs fixtures: user and assistant lines
// alternate, assistant lines carry message.usage, model and requestId, and
// both carry a large text body like real transcripts do.
function writeSessionLogs() {
  fs.mkdirSync(projectDir, { recursive: true });
  const padding = "lorem ipsum dolor sit amet ".repeat(300);
  let bytes = 0;
  for (let file = 0; file < FILES; file++) {
    const sessionId = `ccs224-session-${file}`;
    const lines = [];
    let size = 0;
    for (let turn = 0; size < BYTES_PER_FILE; turn++) {
      const user = JSON.stringify({
        type: "user",
        sessionId,
        timestamp: "2026-06-07T13:01:22Z",
        message: { role: "user", content: padding },
      });
      const assistant = JSON.stringify({
        type: "assistant",
        sessionId,
        requestId: `req_ccs224_${file}_${turn}`,
        timestamp: "2026-06-07T13:01:23Z",
        message: {
          id: `msg_ccs224_${file}_${turn}`,
          type: "message",
          role: "assistant",
          model: "claude-sonnet-4-5",
          content: [{ type: "text", text: padding }],
          stop_reason: "end_turn",
          usage: {
            input_tokens: 12,
            output_tokens: 340,
            cache_read_input_tokens: 4096,
            cache_creation_input_tokens: 512,
          },
        },
      });
      lines.push(user, assistant);
      size += user.length + assistant.length + 2;
    }
    fs.writeFileSync(
      path.join(projectDir, `${sessionId}.jsonl`),
      `${lines.join("\n")}\n`,
    );
    bytes += size;
  }
  return bytes;
}

async function timedProbe() {
  const started = Date.now();
  const done = await browser.execute(() => window.__ccs224Sync.done);
  return { done, ms: Date.now() - started };
}

describe("Session-log sync responsiveness", () => {
  it("opens the usage page", async () => {
    const settings = await $('button[title="Settings"]');
    await settings.waitForDisplayed({ timeout: 60_000 });
    // Only shown when this spec runs on a fresh HOME.
    const notice = await $('[data-testid="first-run-notice-confirm"]');
    if (await notice.isDisplayed()) {
      await notice.click();
      await notice.waitForExist({ reverse: true });
    }

    await settings.click();
    const usageTab = await $(
      '//button[@role="tab"][contains(., "Usage Statistics")]',
    );
    await usageTab.waitForClickable();
    await usageTab.click();
    await expect(usageTab).toHaveAttribute("data-state", "active");
  });

  it("keeps the window responsive while syncing large session logs", async () => {
    const bytes = writeSessionLogs();
    console.log(
      `[e2e] wrote ${FILES} session logs (${(bytes / 1024 / 1024).toFixed(0)} MB)`,
    );

    const baseline = [];
    for (let i = 0; i < 5; i++) {
      const started = Date.now();
      await browser.execute(() => document.readyState);
      baseline.push(Date.now() - started);
    }

    // Same IPC call the usage page's sync button makes. A rAF loop records the
    // largest gap between painted frames until the command settles. The call
    // that starts the sync is timed too: with a main-thread sync the IPC
    // request is served before this call returns.
    const startedAt = Date.now();
    await browser.execute(() => {
      const probe = {
        done: false,
        result: null,
        error: null,
        frames: 0,
        maxFrameGap: 0,
        startedAt: performance.now(),
        finishedAt: 0,
      };
      window.__ccs224Sync = probe;
      let last = performance.now();
      const onFrame = (now) => {
        probe.maxFrameGap = Math.max(probe.maxFrameGap, now - last);
        last = now;
        probe.frames += 1;
        if (!probe.done) requestAnimationFrame(onFrame);
      };
      requestAnimationFrame(onFrame);
      window.__TAURI_INTERNALS__
        .invoke("sync_session_usage")
        .then(
          (result) => {
            probe.result = result;
          },
          (error) => {
            probe.error = String(error);
          },
        )
        .finally(() => {
          probe.finishedAt = performance.now();
          probe.done = true;
        });
    });

    const roundTrips = [Date.now() - startedAt];
    let probesWhileSyncing = 0;
    const deadline = Date.now() + SYNC_TIMEOUT_MS;
    for (;;) {
      const { done, ms } = await timedProbe();
      roundTrips.push(ms);
      if (done) break;
      probesWhileSyncing += 1;
      if (Date.now() > deadline) {
        throw new Error(`sync did not finish in ${SYNC_TIMEOUT_MS} ms`);
      }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }

    const probe = await browser.execute(() => window.__ccs224Sync);
    const syncMs = Math.round(probe.finishedAt - probe.startedAt);
    const maxRoundTrip = Math.max(...roundTrips);
    console.log(
      `[e2e] sync took ${syncMs} ms; WebDriver round trips: baseline max ${Math.max(...baseline)} ms, ` +
        `${probesWhileSyncing} probes while syncing, max ${maxRoundTrip} ms; ` +
        `${probe.frames} frames, max frame gap ${Math.round(probe.maxFrameGap)} ms; ` +
        `result ${JSON.stringify(probe.result ?? probe.error)}`,
    );

    expect(probe.error).toBeNull();
    expect(probe.result.filesScanned).toBeGreaterThanOrEqual(FILES);
    // WebDriver is served on the main thread, so a blocked main thread shows
    // up here. Frames are painted by the web process and catch jank in the
    // page itself (e.g. refetch storms while rows are imported).
    expect(maxRoundTrip).toBeLessThan(MAX_STALL_MS);
    expect(probe.maxFrameGap).toBeLessThan(MAX_STALL_MS);
    // The probes have to overlap the sync, or they prove nothing.
    expect(probesWhileSyncing).toBeGreaterThanOrEqual(3);
    expect(probe.frames).toBeGreaterThanOrEqual(3);
  });
});
