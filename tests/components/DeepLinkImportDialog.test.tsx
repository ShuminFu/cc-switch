import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { emitTauriEvent } from "../msw/tauriMocks";
import { server } from "../msw/server";

const TAURI_ENDPOINT = "http://tauri.local";

const toastErrorMock = vi.fn();
const toastWarningMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: (...args: unknown[]) => toastErrorMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
  },
}));

describe("DeepLinkImportDialog", () => {
  it("never logs the deeplink URL (which may carry secrets) on deeplink-error", async () => {
    const consoleErrorSpy = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    const queryClient = new QueryClient();

    render(
      <QueryClientProvider client={queryClient}>
        <DeepLinkImportDialog />
      </QueryClientProvider>,
    );

    // Wait for listeners to register (listen() is async)
    await Promise.resolve();
    await Promise.resolve();

    emitTauriEvent("deeplink-error", {
      url: "ccswitch://v1/import?resource=bogus&apiKey=sk-secret",
      error: "Unsupported resource type: bogus",
    });

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());

    expect(consoleErrorSpy).toHaveBeenCalled();
    const logged = JSON.stringify(consoleErrorSpy.mock.calls);
    expect(logged).not.toContain("sk-secret");
    expect(logged).toContain("Unsupported resource type: bogus");

    consoleErrorSpy.mockRestore();
  });

  const renderWithClaudeConfig = async (
    env: Record<string, string>,
    extra: Record<string, unknown> = {},
  ) => {
    // Echo the request back, like the backend merge does for these fields
    server.use(
      http.post(
        `${TAURI_ENDPOINT}/merge_deeplink_config`,
        async ({ request }) => {
          const body = (await request.json()) as {
            request: Record<string, unknown>;
          };
          return HttpResponse.json(body.request);
        },
      ),
    );
    const queryClient = new QueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <DeepLinkImportDialog />
      </QueryClientProvider>,
    );
    await Promise.resolve();
    await Promise.resolve();

    emitTauriEvent("deeplink-import", {
      version: "v1",
      resource: "provider",
      app: "claude",
      name: "Env Provider",
      endpoint: "https://api.example.com",
      apiKey: "sk-test",
      config: btoa(JSON.stringify({ env })),
      configFormat: "json",
      ...extra,
    });
    await screen.findByText("deeplink.configDetails");
  };

  it("highlights non-allow-listed env keys with their full values", async () => {
    const longValue = `--some-flag=${"x".repeat(200)}`;
    await renderWithClaudeConfig(
      {
        ANTHROPIC_BASE_URL: "https://api.example.com",
        API_TIMEOUT_MS: "600000",
        CUSTOM_RUNTIME_FLAGS: longValue,
        NODE_OPTIONS: "--require /tmp/evil.js",
        LD_PRELOAD: "/tmp/evil.so",
      },
      { enabled: true },
    );

    const unlisted = screen.getByTestId("deeplink-unlisted-env");
    expect(unlisted).toHaveTextContent("deeplink.unlistedEnvTitle");
    expect(unlisted).toHaveTextContent("CUSTOM_RUNTIME_FLAGS");
    // Full, untruncated value
    expect(unlisted).toHaveTextContent(longValue);
    expect(unlisted).toHaveTextContent("deeplink.unlistedEnvNoAutoSwitch");
    expect(unlisted).not.toHaveTextContent("API_TIMEOUT_MS");

    const denied = screen.getByTestId("deeplink-denied-env");
    expect(denied).toHaveTextContent("NODE_OPTIONS");
    expect(denied).toHaveTextContent("LD_PRELOAD");
    expect(denied).not.toHaveTextContent("--require");
  });

  it("shows no env warning when every key is allow-listed", async () => {
    await renderWithClaudeConfig({
      ANTHROPIC_BASE_URL: "https://api.example.com",
      ANTHROPIC_CUSTOM_HEADERS: "X-Org: 1",
      API_TIMEOUT_MS: "600000",
      CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
      HTTPS_PROXY: "http://127.0.0.1:7890",
    });

    expect(screen.getByText("API_TIMEOUT_MS")).toBeInTheDocument();
    expect(screen.queryByTestId("deeplink-unlisted-env")).toBeNull();
    expect(screen.queryByTestId("deeplink-denied-env")).toBeNull();
  });

  it("masks short and long env secrets to at most 4 characters", async () => {
    await renderWithClaudeConfig({
      ANTHROPIC_AUTH_TOKEN: "sk-abcdefghijk",
      ANTHROPIC_API_KEY: "abcdefgh",
      ANTHROPIC_BASE_URL: "https://api.example.com",
    });

    expect(screen.getByText(`sk-a${"*".repeat(20)}`)).toBeInTheDocument();
    expect(screen.queryByText(/sk-abcd/)).toBeNull();
    expect(screen.queryByText(/abcdefgh/)).toBeNull();
    expect(screen.getAllByText("****").length).toBeGreaterThan(0);
  });

  const providerRequest = (name: string, apiKey: string) => ({
    version: "v1",
    resource: "provider",
    app: "claude",
    name,
    endpoint: `https://${name.toLowerCase().replace(/\s+/g, "-")}.example.com`,
    apiKey,
  });

  const renderDialog = async () => {
    const queryClient = new QueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <DeepLinkImportDialog />
      </QueryClientProvider>,
    );
    await Promise.resolve();
    await Promise.resolve();
  };

  const captureImports = () => {
    const imported: Array<Record<string, unknown>> = [];
    server.use(
      http.post(
        `${TAURI_ENDPOINT}/import_from_deeplink_unified`,
        async ({ request }) => {
          const body = (await request.json()) as {
            request: Record<string, unknown>;
          };
          imported.push(body.request);
          return HttpResponse.json({ type: "provider", id: "imported-id" });
        },
      ),
    );
    return imported;
  };

  it("keeps the reviewed request when another deeplink arrives while the dialog is open", async () => {
    toastWarningMock.mockClear();
    const imported = captureImports();
    await renderDialog();

    act(() => {
      emitTauriEvent(
        "deeplink-import",
        providerRequest("Provider A", "sk-aaaa"),
      );
    });
    await screen.findByText("Provider A");

    act(() => {
      emitTauriEvent(
        "deeplink-import",
        providerRequest("Provider B", "sk-bbbb"),
      );
    });

    await waitFor(() =>
      expect(toastWarningMock).toHaveBeenCalledWith(
        "deeplink.requestIgnoredBusy",
        expect.anything(),
      ),
    );
    expect(screen.getByText("Provider A")).toBeInTheDocument();
    expect(screen.queryByText("Provider B")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "deeplink.import" }));

    await waitFor(() => expect(imported).toHaveLength(1));
    expect(imported[0]).toMatchObject({
      name: "Provider A",
      apiKey: "sk-aaaa",
    });
  });

  it("ignores a deeplink that arrives while the previous config is still merging", async () => {
    toastWarningMock.mockClear();
    const imported = captureImports();
    let releaseMerge!: () => void;
    const mergeGate = new Promise<void>((resolve) => {
      releaseMerge = resolve;
    });
    server.use(
      http.post(
        `${TAURI_ENDPOINT}/merge_deeplink_config`,
        async ({ request }) => {
          const body = (await request.json()) as {
            request: Record<string, unknown>;
          };
          await mergeGate;
          return HttpResponse.json(body.request);
        },
      ),
    );
    await renderDialog();

    act(() => {
      emitTauriEvent("deeplink-import", {
        ...providerRequest("Provider A", "sk-aaaa"),
        configUrl: "https://config.example.com/a.json",
      });
    });
    emitTauriEvent("deeplink-import", providerRequest("Provider B", "sk-bbbb"));

    await waitFor(() => expect(toastWarningMock).toHaveBeenCalledTimes(1));
    await act(async () => {
      releaseMerge();
    });

    await screen.findByText("Provider A");
    expect(screen.queryByText("Provider B")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "deeplink.import" }));

    await waitFor(() => expect(imported).toHaveLength(1));
    expect(imported[0]).toMatchObject({ name: "Provider A" });
  });

  it("accepts a new deeplink once the previous dialog was cancelled", async () => {
    toastWarningMock.mockClear();
    await renderDialog();

    act(() => {
      emitTauriEvent(
        "deeplink-import",
        providerRequest("Provider A", "sk-aaaa"),
      );
    });
    await screen.findByText("Provider A");

    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await waitFor(() => expect(screen.queryByText("Provider A")).toBeNull());

    act(() => {
      emitTauriEvent(
        "deeplink-import",
        providerRequest("Provider B", "sk-bbbb"),
      );
    });
    await screen.findByText("Provider B");
    expect(toastWarningMock).not.toHaveBeenCalled();
  });
});
