/**
 * Browser-side Tauri IPC mock for the Playwright harness (TEST-17).
 *
 * Loaded by the harness's Vite plugin before `src/main.tsx`, it installs
 * `window.__TAURI_INTERNALS__` via `@tauri-apps/api/mocks` and answers every
 * `invoke()` with the SAME MSW handlers the vitest suite uses
 * (tests/msw/handlers.ts, backed by tests/msw/state.ts). No service worker is
 * involved: each invoke is turned into a `Request` and resolved with MSW's
 * `getResponse()`.
 *
 * Commands without a handler resolve to `null` and are recorded in
 * `window.__E2E__.unhandled`, so a journey can assert it exercised only
 * mocked behaviour. `window.__E2E__.calls` records every invoke.
 */
import { getResponse } from "msw";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { handlers } from "../tests/msw/handlers";
import { resetProviderState } from "../tests/msw/state";

type Call = { cmd: string; args: unknown };

declare global {
  interface Window {
    __E2E__: {
      calls: Call[];
      unhandled: string[];
      /** Per-command overrides installed by a journey (value or function). */
      overrides: Record<string, unknown>;
      reset: () => void;
    };
  }
}

const TAURI_ENDPOINT = "http://tauri.local";

// Plugin commands the app issues at startup that no unit test needs.
const PLUGIN_DEFAULTS: Record<string, unknown> = {
  "plugin:window|is_maximized": false,
  "plugin:window|is_fullscreen": false,
  "plugin:window|scale_factor": 1,
  "plugin:window|theme": "light",
  "plugin:app|version": "0.0.0-e2e",
  "plugin:app|name": "cc-switch",
  "plugin:path|resolve_directory": "/home/e2e",
  "plugin:updater|check": null,
};

window.__E2E__ = {
  calls: [],
  unhandled: [],
  overrides: {},
  reset: () => {
    window.__E2E__.calls = [];
    window.__E2E__.unhandled = [];
    resetProviderState();
  },
};

mockWindows("main");

mockIPC(
  async (cmd, args) => {
    window.__E2E__.calls.push({ cmd, args });

    if (cmd in window.__E2E__.overrides) {
      const override = window.__E2E__.overrides[cmd];
      return typeof override === "function" ? override(args) : override;
    }
    if (cmd in PLUGIN_DEFAULTS) return PLUGIN_DEFAULTS[cmd];
    if (cmd.startsWith("plugin:")) return null;

    const request = new Request(`${TAURI_ENDPOINT}/${cmd}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(args ?? {}),
    });
    const response = await getResponse(handlers, request);
    if (!response) {
      window.__E2E__.unhandled.push(cmd);
      return null;
    }
    const text = await response.text();
    const body = text ? JSON.parse(text) : null;
    if (!response.ok) throw body;
    return body;
  },
  { shouldMockEvents: true },
);
