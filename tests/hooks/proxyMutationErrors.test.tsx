import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useSwitchProxyProvider,
  useUpdateAppProxyConfig,
  useUpdateGlobalProxyConfig,
} from "@/lib/query/proxy";
import { useScanProxies, useTestProxy } from "@/hooks/useGlobalProxy";
import type { AppProxyConfig, GlobalProxyConfig } from "@/types/proxy";
import { createTestQueryClient } from "../utils/testQueryClient";

const toastErrorMock = vi.fn();
const invokeMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options && "error" in options ? `${key}|${options.error}` : key,
  }),
}));

function createWrapper() {
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { wrapper };
}

// Tauri commands returning `Result<_, String>` reject with a plain string,
// not an Error instance. The toast must still carry that message.
describe("proxy mutation error toasts with Tauri string rejections", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    toastErrorMock.mockReset();
    invokeMock.mockRejectedValue("port 15721 already in use");
  });

  it("useUpdateGlobalProxyConfig surfaces the backend message", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useUpdateGlobalProxyConfig(), {
      wrapper,
    });

    await act(async () => {
      await result.current
        .mutateAsync({} as GlobalProxyConfig)
        .catch(() => undefined);
    });

    expect(toastErrorMock).toHaveBeenCalledWith(
      "proxy.settings.toast.saveFailed|port 15721 already in use",
    );
  });

  it("useUpdateAppProxyConfig surfaces the backend message", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useUpdateAppProxyConfig(), {
      wrapper,
    });

    await act(async () => {
      await result.current
        .mutateAsync({ appType: "claude" } as AppProxyConfig)
        .catch(() => undefined);
    });

    expect(toastErrorMock).toHaveBeenCalledWith(
      "proxy.settings.toast.saveFailed|port 15721 already in use",
    );
  });

  it("useSwitchProxyProvider surfaces the backend message", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useSwitchProxyProvider(), {
      wrapper,
    });

    await act(async () => {
      await result.current
        .mutateAsync({ appType: "claude", providerId: "p1" })
        .catch(() => undefined);
    });

    expect(toastErrorMock).toHaveBeenCalledWith(
      "proxy.switchFailed|port 15721 already in use",
    );
  });

  it("useTestProxy surfaces the backend message", async () => {
    invokeMock.mockRejectedValue("Proxy URL is empty");
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useTestProxy(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync("").catch(() => undefined);
    });

    expect(toastErrorMock).toHaveBeenCalledWith("Proxy URL is empty");
  });

  it("useScanProxies surfaces the backend message", async () => {
    invokeMock.mockRejectedValue("scan exploded");
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useScanProxies(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync().catch(() => undefined);
    });

    expect(toastErrorMock).toHaveBeenCalledWith(
      "settings.globalProxy.scanFailed|scan exploded",
    );
  });
});
