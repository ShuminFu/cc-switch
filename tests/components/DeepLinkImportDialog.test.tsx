import { render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { emitTauriEvent } from "../msw/tauriMocks";
import { server } from "../msw/server";

const TAURI_ENDPOINT = "http://tauri.local";

const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: (...args: unknown[]) => toastErrorMock(...args),
    warning: vi.fn(),
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
});
