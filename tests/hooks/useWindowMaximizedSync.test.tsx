import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const isMaximizedMock = vi.fn<() => Promise<boolean>>();
const onResizedMock = vi.fn<(handler: () => void) => Promise<() => void>>();

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: () => isMaximizedMock(),
    onResized: (handler: () => void) => onResizedMock(handler),
  }),
}));

import { useWindowMaximizedSync } from "@/hooks/useWindowMaximizedSync";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

describe("useWindowMaximizedSync", () => {
  beforeEach(() => {
    isMaximizedMock.mockReset();
    onResizedMock.mockReset();
  });

  it("syncs the maximized state on mount and on resize, and unlistens on unmount", async () => {
    const unlisten = vi.fn();
    let resizeHandler: (() => void) | undefined;
    isMaximizedMock.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    onResizedMock.mockImplementation(async (handler) => {
      resizeHandler = handler;
      return unlisten;
    });
    const onChange = vi.fn();

    const { unmount } = renderHook(() => useWindowMaximizedSync(onChange));

    await waitFor(() => expect(onResizedMock).toHaveBeenCalledTimes(1));
    expect(onChange).toHaveBeenLastCalledWith(false);

    resizeHandler?.();
    await waitFor(() => expect(onChange).toHaveBeenLastCalledWith(true));

    unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("unlistens once when unmounted before onResized resolves", async () => {
    const unlisten = vi.fn();
    const registration = deferred<() => void>();
    isMaximizedMock.mockResolvedValue(false);
    onResizedMock.mockReturnValue(registration.promise);
    const onChange = vi.fn();

    const { unmount } = renderHook(() => useWindowMaximizedSync(onChange));
    await waitFor(() => expect(onResizedMock).toHaveBeenCalledTimes(1));

    unmount();
    expect(unlisten).not.toHaveBeenCalled();

    registration.resolve(unlisten);
    await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
    // Give any stray continuation a chance to run; still exactly once.
    await new Promise((r) => setTimeout(r, 0));
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("does not register a resize listener when unmounted before the initial sync resolves", async () => {
    const initial = deferred<boolean>();
    isMaximizedMock.mockReturnValue(initial.promise);
    const onChange = vi.fn();

    const { unmount } = renderHook(() => useWindowMaximizedSync(onChange));
    unmount();

    initial.resolve(true);
    await new Promise((r) => setTimeout(r, 0));
    expect(onChange).not.toHaveBeenCalled();
    expect(onResizedMock).not.toHaveBeenCalled();
  });
});
