import { act, render, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { StartupWarningsNotifier } from "@/components/notifications/StartupWarningsNotifier";
import { server } from "../msw/server";
import { emitTauriEvent } from "../msw/tauriMocks";

const toastWarning = vi.fn();
vi.mock("sonner", () => ({
  toast: { warning: (...a: unknown[]) => toastWarning(...a) },
}));

describe("StartupWarningsNotifier", () => {
  beforeEach(() => {
    toastWarning.mockReset();
    sessionStorage.clear();
  });

  it("shows each startup warning once as a dismissible toast", async () => {
    server.use(
      http.post("http://tauri.local/get_startup_warnings", () =>
        HttpResponse.json([
          {
            kind: "settings_corrupt",
            path: "/home/u/.cc-switch/settings.json.corrupt",
            message: "expected value",
          },
          {
            kind: "proxy_restore_failed",
            appType: "claude",
            message: "port busy",
          },
        ]),
      ),
    );
    render(<StartupWarningsNotifier />);

    await waitFor(() => expect(toastWarning).toHaveBeenCalledTimes(2));
    const [title, opts] = toastWarning.mock.calls[0];
    expect(title).toBe("Settings file could not be read");
    expect(opts.description).toContain("settings.json.corrupt");
    expect(opts.closeButton).toBe(true);
    expect(toastWarning.mock.calls[1][1].description).toContain("port busy");

    // The live event for the same app does not show a duplicate toast.
    await act(async () => {
      emitTauriEvent("proxy-restore-failed", {
        appType: "claude",
        error: "port busy",
      });
    });
    expect(toastWarning).toHaveBeenCalledTimes(2);

    // Dismissing remembers the warning for this session.
    act(() => opts.onDismiss());
    expect(sessionStorage.getItem("startup_warnings_dismissed")).toContain(
      "settings_corrupt",
    );
  });

  it("shows a proxy-restore-failed event raised after startup", async () => {
    render(<StartupWarningsNotifier />);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    await act(async () => {
      emitTauriEvent("proxy-restore-failed", {
        appType: "codex",
        error: "bind failed",
      });
    });
    await waitFor(() =>
      expect(toastWarning).toHaveBeenCalledWith(
        "Proxy takeover could not be restored",
        expect.objectContaining({
          description: expect.stringContaining("bind failed"),
        }),
      ),
    );
  });

  it("skips warnings dismissed earlier in the session", async () => {
    sessionStorage.setItem(
      "startup_warnings_dismissed",
      JSON.stringify(["config_dir_unavailable||/missing"]),
    );
    server.use(
      http.post("http://tauri.local/get_startup_warnings", () =>
        HttpResponse.json([
          { kind: "config_dir_unavailable", path: "/missing", message: "x" },
        ]),
      ),
    );
    render(<StartupWarningsNotifier />);
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    expect(toastWarning).not.toHaveBeenCalled();
  });
});
