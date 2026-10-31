import { StrictMode, type ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it, vi } from "vitest";
import { APP_EVENT_NAMES } from "@/lib/events";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { emitTauriEvent } from "../msw/tauriMocks";

const ROOT = resolve(__dirname, "../..");

function walk(dir: string, ext: RegExp, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, ext, out);
    else if (ext.test(name)) out.push(path);
  }
  return out;
}

/** Event names the Rust side emits: `.emit("x", ..)`, `*_EVENT` consts and events.rs. */
function rustEmittedNames(): Set<string> {
  const names = new Set<string>();
  for (const file of walk(join(ROOT, "src-tauri/src"), /\.rs$/)) {
    const src = readFileSync(file, "utf8");
    for (const m of src.matchAll(
      /\.emit(?:_to|_all)?\(\s*(?:[^,()"]+,\s*)?"([a-z0-9:_-]+)"/g,
    )) {
      names.add(m[1]);
    }
    for (const m of src.matchAll(
      /const [A-Z_]*EVENT[A-Z_]*: &str = "([^"]+)"/g,
    )) {
      names.add(m[1]);
    }
    // src-tauri/src/events.rs: the central event-name constants.
    if (file.endsWith("/events.rs")) {
      for (const m of src.matchAll(/pub const [A-Z_]+: &str = "([^"]+)"/g)) {
        names.add(m[1]);
      }
    }
  }
  return names;
}

/** Event names the frontend subscribes to by string literal. */
function frontendListenedNames(): Set<string> {
  const names = new Set<string>();
  for (const file of walk(join(ROOT, "src"), /\.(ts|tsx)$/)) {
    if (file.endsWith("bindings.ts")) continue;
    const src = readFileSync(file, "utf8");
    for (const m of src.matchAll(
      /(?:useTauriEvent|listen)(?:<[^>]*>)?\(\s*"([a-z0-9:_-]+)"/g,
    )) {
      names.add(m[1]);
    }
  }
  return names;
}

// FE-ARCH-19: event names are one shared contract.
describe("AppEvents contract", () => {
  const contract = new Set<string>(APP_EVENT_NAMES);

  it("covers every event the backend emits", () => {
    const emitted = rustEmittedNames();
    expect(emitted.has("provider-switched")).toBe(true);
    expect(emitted.has("usage-log-recorded")).toBe(true);
    const missing = [...emitted].filter((n) => !contract.has(n));
    expect(missing).toEqual([]);
  });

  it("only subscribes to events in the contract", () => {
    const unknown = [...frontendListenedNames()].filter(
      (n) => !contract.has(n),
    );
    expect(unknown).toEqual([]);
  });
});

describe("useTauriEvent lifecycle", () => {
  it("subscribes once under a StrictMode double mount and unsubscribes on unmount", async () => {
    const handler = vi.fn();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <StrictMode>{children}</StrictMode>
    );
    const { unmount } = renderHook(
      () => useTauriEvent("profile-applied", handler),
      { wrapper },
    );
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    emitTauriEvent("profile-applied", { ok: true });
    expect(handler).toHaveBeenCalledTimes(1);
    expect(handler).toHaveBeenCalledWith({ ok: true });

    unmount();
    emitTauriEvent("profile-applied", null);
    expect(handler).toHaveBeenCalledTimes(1);
  });
});
