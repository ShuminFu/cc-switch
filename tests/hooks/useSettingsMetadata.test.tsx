// TEST-12: mocked at the IPC boundary (MSW) instead of `vi.mock("@/lib/api")`,
// so the real settingsApi wrapper and its command name are exercised.
import { renderHook, act, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { useSettingsMetadata } from "@/hooks/useSettingsMetadata";
import { mockInvoke } from "../msw/invokeRecorder";
import { tauriError } from "../msw/tauriError";
import { server } from "../msw/server";
import { http } from "msw";

describe("useSettingsMetadata", () => {
  it("loads portable flag and handles success path", async () => {
    const recorder = mockInvoke({ is_portable_mode: true });

    const { result } = renderHook(() => useSettingsMetadata());

    expect(result.current.isLoading).toBe(true);
    expect(result.current.isPortable).toBe(false);

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.isPortable).toBe(true);
    expect(recorder.callsTo("is_portable_mode")).toEqual([
      { command: "is_portable_mode", args: {} },
    ]);
  });

  it("handles a backend rejection and proceeds", async () => {
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    server.use(
      http.post("http://tauri.local/is_portable_mode", () =>
        tauriError("network failure"),
      ),
    );

    const { result } = renderHook(() => useSettingsMetadata());

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.isPortable).toBe(false);
    expect(consoleError).toHaveBeenCalledWith(
      "[useSettingsMetadata] Failed to load metadata",
      "network failure",
    );
    consoleError.mockRestore();
  });

  it("allows updating restart flag via setters", async () => {
    const { result } = renderHook(() => useSettingsMetadata());
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    act(() => result.current.setRequiresRestart(true));
    expect(result.current.requiresRestart).toBe(true);

    act(() => result.current.acknowledgeRestart());
    expect(result.current.requiresRestart).toBe(false);
  });
});
