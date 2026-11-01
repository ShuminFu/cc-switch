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
import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { handlers } from "../tests/msw/handlers";
import { resetProviderState } from "../tests/msw/state";

type Call = { cmd: string; args: unknown };

/** `(args, next)`: `next()` answers through the MSW handlers as usual. */
type Override =
  | unknown
  | ((args: unknown, next: () => Promise<unknown>) => unknown);

declare global {
  interface Window {
    __E2E__: {
      calls: Call[];
      unhandled: string[];
      /** Per-command overrides installed by a journey (value or function). */
      overrides: Record<string, Override>;
      reset: () => void;
      /** Deliver a backend event (e.g. `deeplink-import`) to the app. */
      emit: (event: string, payload: unknown) => Promise<void>;
    };
    /**
     * Overrides a journey installs with `context.addInitScript` before the
     * app boots (startup queries such as get_settings run before any
     * `page.evaluate`). See `withPage({ setup })` in e2e/harness.mjs.
     */
    __E2E_SETUP__?: { overrides?: Record<string, Override> };
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
  overrides: { ...(window.__E2E_SETUP__?.overrides ?? {}) },
  emit: (event, payload) => emit(event, payload),
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
      return typeof override === "function"
        ? override(args, () => viaHandlers(cmd, args))
        : override;
    }
    if (cmd in PLUGIN_DEFAULTS) return PLUGIN_DEFAULTS[cmd];
    if (cmd.startsWith("plugin:")) return null;
    return viaHandlers(cmd, args);
  },
  { shouldMockEvents: true },
);

async function viaHandlers(cmd: string, args: unknown): Promise<unknown> {
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
  if (cmd === "get_settings" && body && typeof body === "object") {
    // The MSW fixtures default to `zh`; keep the language the harness pinned
    // (e2e/harness.mjs) so the settings page does not switch the UI.
    const pinned = readPinnedLanguage();
    if (pinned) (body as { language?: string }).language = pinned;
  }
  return body;
}

function readPinnedLanguage(): string | null {
  try {
    return window.localStorage.getItem("language");
  } catch {
    return null;
  }
}
