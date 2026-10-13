import type { ReactNode } from "react";
import { renderHook, act, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useManagedAuth } from "@/components/providers/forms/hooks/useManagedAuth";
import { createTestQueryClient } from "../utils/testQueryClient";

const authGetStatusMock = vi.fn();
const authStartLoginMock = vi.fn();
const authPollForAccountMock = vi.fn();
const openExternalMock = vi.fn();

vi.mock("@/lib/api", () => ({
  authApi: {
    authGetStatus: (...args: unknown[]) => authGetStatusMock(...args),
    authStartLogin: (...args: unknown[]) => authStartLoginMock(...args),
    authPollForAccount: (...args: unknown[]) => authPollForAccountMock(...args),
    authLogout: vi.fn(),
    authRemoveAccount: vi.fn(),
    authSetDefaultAccount: vi.fn(),
  },
  settingsApi: {
    openExternal: (...args: unknown[]) => openExternalMock(...args),
  },
}));

vi.mock("@/lib/clipboard", () => ({
  copyText: vi.fn().mockResolvedValue(undefined),
}));

const deviceCode = {
  device_code: "dev-123",
  user_code: "ABCD-EFGH",
  verification_uri: "https://example.com/device",
  expires_in: 900,
  interval: 5,
};

function createWrapper() {
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { wrapper };
}

describe("useManagedAuth", () => {
  beforeEach(() => {
    authGetStatusMock.mockReset();
    authStartLoginMock.mockReset();
    authPollForAccountMock.mockReset();
    openExternalMock.mockReset();

    authGetStatusMock.mockResolvedValue({
      provider: "github_copilot",
      authenticated: false,
      default_account_id: null,
      accounts: [],
    });
    authPollForAccountMock.mockRejectedValue(new Error("pending"));
    openExternalMock.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not start polling when cancelled while the login request is in flight", async () => {
    let resolveStart: (value: typeof deviceCode) => void = () => {};
    authStartLoginMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveStart = resolve;
        }),
    );

    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useManagedAuth("github_copilot"), {
      wrapper,
    });

    act(() => {
      result.current.startAuth();
    });
    await waitFor(() => expect(authStartLoginMock).toHaveBeenCalledTimes(1));

    act(() => {
      result.current.cancelAuth();
    });

    await act(async () => {
      resolveStart(deviceCode);
    });

    // Give the (stale) onSuccess a chance to run fully.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });

    expect(result.current.pollingState).toBe("idle");
    expect(result.current.deviceCode).toBeNull();
    expect(openExternalMock).not.toHaveBeenCalled();
    expect(authPollForAccountMock).not.toHaveBeenCalled();
  });

  it("does not keep polling after unmount when the login response arrives late", async () => {
    let resolveStart: (value: typeof deviceCode) => void = () => {};
    authStartLoginMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveStart = resolve;
        }),
    );

    const { wrapper } = createWrapper();
    const { result, unmount } = renderHook(
      () => useManagedAuth("github_copilot"),
      { wrapper },
    );

    act(() => {
      result.current.startAuth();
    });
    await waitFor(() => expect(authStartLoginMock).toHaveBeenCalledTimes(1));

    unmount();

    const setIntervalSpy = vi.spyOn(globalThis, "setInterval");
    await act(async () => {
      resolveStart(deviceCode);
      await new Promise((r) => setTimeout(r, 0));
    });

    expect(setIntervalSpy).not.toHaveBeenCalled();
    expect(authPollForAccountMock).not.toHaveBeenCalled();
    setIntervalSpy.mockRestore();
  });

  it("starts polling for a live login attempt", async () => {
    authStartLoginMock.mockResolvedValue(deviceCode);

    const { wrapper } = createWrapper();
    const { result, unmount } = renderHook(
      () => useManagedAuth("github_copilot"),
      { wrapper },
    );

    act(() => {
      result.current.startAuth();
    });

    await waitFor(() => expect(result.current.pollingState).toBe("polling"));
    await waitFor(() =>
      expect(authPollForAccountMock).toHaveBeenCalledWith(
        "github_copilot",
        "dev-123",
        undefined,
      ),
    );
    expect(result.current.deviceCode).toEqual(deviceCode);

    unmount();
  });
});
