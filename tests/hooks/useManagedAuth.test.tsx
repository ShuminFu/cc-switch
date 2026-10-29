import type { ReactNode } from "react";
import { renderHook, act } from "@testing-library/react";
import {
  QueryClient,
  QueryClientProvider,
  notifyManager,
} from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ManagedAuthDeviceCodeResponse } from "@/lib/api";
import { useManagedAuth } from "@/components/providers/forms/hooks/useManagedAuth";

const authStartLoginMock = vi.fn();
const authPollForAccountMock = vi.fn();
const authGetStatusMock = vi.fn();

vi.mock("@/lib/api", () => ({
  authApi: {
    authStartLogin: (...args: unknown[]) => authStartLoginMock(...args),
    authPollForAccount: (...args: unknown[]) => authPollForAccountMock(...args),
    authGetStatus: (...args: unknown[]) => authGetStatusMock(...args),
    authLogout: vi.fn(),
    authRemoveAccount: vi.fn(),
    authSetDefaultAccount: vi.fn(),
  },
  settingsApi: {
    openExternal: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock("@/lib/clipboard", () => ({
  copyText: vi.fn().mockResolvedValue(undefined),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function deviceCode(
  overrides: Partial<ManagedAuthDeviceCodeResponse> = {},
): ManagedAuthDeviceCodeResponse {
  return {
    provider: "github_copilot",
    device_code: "device-code",
    user_code: "ABCD-1234",
    verification_uri: "https://github.com/login/device",
    expires_in: 60,
    interval: 5,
    ...overrides,
  };
}

function createWrapper() {
  // gcTime: Infinity keeps React Query from arming its own GC timers so
  // vi.getTimerCount() only reflects the hook's polling timers.
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Infinity },
      mutations: { gcTime: Infinity },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return wrapper;
}

const flush = () =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });

const advance = (ms: number) =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });

describe("useManagedAuth polling timers", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    notifyManager.setScheduler(queueMicrotask);
    authStartLoginMock.mockReset();
    authPollForAccountMock.mockReset();
    authGetStatusMock.mockReset();
    authGetStatusMock.mockResolvedValue({
      provider: "github_copilot",
      authenticated: false,
      default_account_id: null,
      accounts: [],
    });
    authPollForAccountMock.mockRejectedValue(
      new Error("authorization_pending"),
    );
  });

  afterEach(() => {
    notifyManager.setScheduler((cb) => setTimeout(cb, 0));
    vi.useRealTimers();
  });

  it("a double start keeps only the latest attempt and leaves no timers after expiry", async () => {
    const first = deferred<ManagedAuthDeviceCodeResponse>();
    const second = deferred<ManagedAuthDeviceCodeResponse>();
    authStartLoginMock
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const { result } = renderHook(() => useManagedAuth("github_copilot"), {
      wrapper: createWrapper(),
    });
    await flush();

    act(() => result.current.startAuth());
    act(() => result.current.startAuth());
    await flush();

    first.resolve(deviceCode({ device_code: "first", expires_in: 20 }));
    second.resolve(deviceCode({ device_code: "second", expires_in: 60 }));
    await flush();

    // The stale first attempt must never poll or arm timers.
    expect(
      authPollForAccountMock.mock.calls.some((call) => call[1] === "first"),
    ).toBe(false);

    // The first code's expiry must not kill the valid second session.
    await advance(25_000);
    expect(result.current.pollingState).toBe("polling");
    expect(result.current.error).toBeNull();

    await advance(40_000);
    expect(result.current.pollingState).toBe("error");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not poll when unmounted before the start request resolves", async () => {
    const pending = deferred<ManagedAuthDeviceCodeResponse>();
    authStartLoginMock.mockReturnValueOnce(pending.promise);

    const { result, unmount } = renderHook(
      () => useManagedAuth("github_copilot"),
      { wrapper: createWrapper() },
    );
    await flush();

    act(() => result.current.startAuth());
    await flush();
    unmount();

    pending.resolve(deviceCode());
    await flush();
    await advance(120_000);

    expect(authPollForAccountMock).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not poll when cancelled before the start request resolves", async () => {
    const pending = deferred<ManagedAuthDeviceCodeResponse>();
    authStartLoginMock.mockReturnValueOnce(pending.promise);

    const { result } = renderHook(() => useManagedAuth("github_copilot"), {
      wrapper: createWrapper(),
    });
    await flush();

    act(() => result.current.startAuth());
    await flush();
    act(() => result.current.cancelAuth());

    pending.resolve(deviceCode());
    await flush();
    await advance(120_000);

    expect(authPollForAccountMock).not.toHaveBeenCalled();
    expect(result.current.pollingState).toBe("idle");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps at most one poll in flight", async () => {
    authStartLoginMock.mockResolvedValueOnce(deviceCode({ expires_in: 300 }));
    const slowPoll = deferred<null>();
    authPollForAccountMock.mockReturnValueOnce(slowPoll.promise);

    const { result } = renderHook(() => useManagedAuth("github_copilot"), {
      wrapper: createWrapper(),
    });
    await flush();

    act(() => result.current.startAuth());
    await flush();
    expect(authPollForAccountMock).toHaveBeenCalledTimes(1);

    // Several intervals pass while the first poll is still in flight.
    await advance(40_000);
    expect(authPollForAccountMock).toHaveBeenCalledTimes(1);

    // Once it settles, the next poll is scheduled one interval later.
    slowPoll.reject(new Error("authorization_pending"));
    await flush();
    expect(authPollForAccountMock).toHaveBeenCalledTimes(1);
    await advance(8_000);
    expect(authPollForAccountMock).toHaveBeenCalledTimes(2);

    act(() => result.current.cancelAuth());
    expect(vi.getTimerCount()).toBe(0);
  });
});
