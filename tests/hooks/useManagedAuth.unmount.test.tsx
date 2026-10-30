import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

const deferred = vi.hoisted(() => {
  let resolve: (value: unknown) => void = () => {};
  const promise = new Promise((r) => {
    resolve = r;
  });
  return { promise, resolve: (value: unknown) => resolve(value) };
});

const mocks = vi.hoisted(() => ({
  openExternal: vi.fn(async () => {}),
  copyText: vi.fn(async () => {}),
  authPollForAccount: vi.fn(async () => null),
}));

vi.mock("@/lib/api", () => ({
  authApi: {
    authGetStatus: vi.fn(async () => ({ authenticated: false, accounts: [] })),
    authStartLogin: vi.fn(() => deferred.promise),
    authPollForAccount: mocks.authPollForAccount,
    authLogout: vi.fn(),
    authRemoveAccount: vi.fn(),
    authSetDefaultAccount: vi.fn(),
  },
  settingsApi: { openExternal: mocks.openExternal },
}));

vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copyText }));

import { useManagedAuth } from "@/components/providers/forms/hooks/useManagedAuth";

describe("useManagedAuth after unmount (FE-PROV-46)", () => {
  it("does not open the browser, copy the code or poll once unmounted", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const setIntervalSpy = vi.spyOn(globalThis, "setInterval");

    const { result, unmount } = renderHook(
      () => useManagedAuth("github_copilot"),
      { wrapper },
    );
    act(() => {
      result.current.addAccount();
    });
    unmount();
    const intervalsBefore = setIntervalSpy.mock.calls.length;

    await act(async () => {
      deferred.resolve({
        device_code: "dc",
        user_code: "UC-123",
        verification_uri: "https://github.com/login/device",
        expires_in: 900,
        interval: 5,
      });
      await new Promise((resolve) => setTimeout(resolve, 20));
    });

    expect(mocks.copyText).not.toHaveBeenCalled();
    expect(mocks.openExternal).not.toHaveBeenCalled();
    expect(mocks.authPollForAccount).not.toHaveBeenCalled();
    expect(setIntervalSpy.mock.calls.length).toBe(intervalsBefore);
    setIntervalSpy.mockRestore();
  });
});
