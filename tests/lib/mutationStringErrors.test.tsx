import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import i18n from "i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useUpdateAppProxyConfig } from "@/lib/query/proxy";
import { useScanProxies, useTestProxy } from "@/hooks/useGlobalProxy";
import { createTestQueryClient } from "../utils/testQueryClient";

const toastErrorMock = vi.fn();
const updateProxyConfigForAppMock = vi.fn();
const testProxyUrlMock = vi.fn();
const scanLocalProxiesMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    success: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    updateProxyConfigForApp: (...args: unknown[]) =>
      updateProxyConfigForAppMock(...args),
  },
}));

vi.mock("@/lib/api/globalProxy", () => ({
  getGlobalProxyUrl: vi.fn(),
  setGlobalProxyUrl: vi.fn(),
  getUpstreamProxyStatus: vi.fn(),
  testProxyUrl: (...args: unknown[]) => testProxyUrlMock(...args),
  scanLocalProxies: (...args: unknown[]) => scanLocalProxiesMock(...args),
}));

function setup<T>(hook: () => T) {
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return renderHook(hook, { wrapper });
}

/** Text of the first toast.error call, with i18n interpolation params joined. */
function lastToastText(): string {
  const [message] = toastErrorMock.mock.calls.at(-1) ?? [];
  return String(message);
}

// FE-ARCH-39: Tauri `Result<_, String>` commands reject with a bare string, so
// reading `error.message` showed "undefined" or an empty toast.
describe("mutation onError with string rejections", () => {
  beforeEach(() => {
    toastErrorMock.mockReset();
    i18n.addResourceBundle(
      "en",
      "translation",
      {
        proxy: {
          settings: { toast: { saveFailed: "Save failed: {{error}}" } },
        },
        settings: {
          globalProxy: {
            testFailed: "Connection failed: {{error}}",
            scanFailed: "Scan failed: {{error}}",
          },
        },
      },
      true,
      true,
    );
  });

  it("useUpdateAppProxyConfig shows the backend string", async () => {
    updateProxyConfigForAppMock.mockRejectedValue("port in use");
    const { result } = setup(() => useUpdateAppProxyConfig());
    act(() => {
      result.current.mutate({ appType: "claude" } as never);
    });
    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(JSON.stringify(toastErrorMock.mock.calls)).toContain("port in use");
  });

  it("useTestProxy does not toast an empty message", async () => {
    testProxyUrlMock.mockRejectedValue("Proxy URL is empty");
    const { result } = setup(() => useTestProxy());
    act(() => {
      result.current.mutate("http://127.0.0.1:1");
    });
    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(lastToastText()).not.toBe("undefined");
    expect(JSON.stringify(toastErrorMock.mock.calls)).toContain(
      "Proxy URL is empty",
    );
  });

  it("useScanProxies renders the string detail", async () => {
    scanLocalProxiesMock.mockRejectedValue("scan blocked");
    const { result } = setup(() => useScanProxies());
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(JSON.stringify(toastErrorMock.mock.calls)).toContain("scan blocked");
  });
});
