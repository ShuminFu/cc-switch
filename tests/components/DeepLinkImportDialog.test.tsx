import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
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

const emptyPreview = {
  unlistedEnvKeys: [],
  deniedEnvKeys: [],
  mcpServers: [],
  liveFiles: [],
  writesLiveOnImport: false,
  risks: [],
};

// Echo the request back as the merged request, like the backend preview does
// for these fields, with `extra` preview fields on top.
const echoPreview = (
  extra: Record<string, unknown> = {},
  gate?: Promise<void>,
) =>
  http.post(
    `${TAURI_ENDPOINT}/preview_deeplink_import`,
    async ({ request }) => {
      const body = (await request.json()) as {
        request: Record<string, unknown>;
      };
      if (gate) await gate;
      return HttpResponse.json({
        ...emptyPreview,
        ...extra,
        request: body.request,
      });
    },
  );

describe("DeepLinkImportDialog", () => {
  beforeEach(() => {
    server.use(echoPreview());
  });

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
    server.use(echoPreview({}, mergeGate));
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

  const importButton = () =>
    screen.getByRole("button", { name: "deeplink.import" });

  it("renders a link's MCP command in full and gates Import on the risk box", async () => {
    const imported = captureImports();
    const longArg = `--script=${"a".repeat(180)}`;
    const spec = {
      command: "npx",
      args: ["-y", "evil-mcp@9.9.9", longArg],
      env: { API_TOKEN: "tok-abcdefghijkl", MODE: "fast" },
    };
    server.use(
      echoPreview({
        mcpServers: [
          {
            id: "runner",
            command: spec.command,
            args: spec.args,
            spec,
            exists: false,
          },
        ],
        liveFiles: ["/home/u/.claude.json"],
        writesLiveOnImport: true,
        risks: ["mcpCommand"],
      }),
    );
    await renderDialog();

    act(() => {
      emitTauriEvent("deeplink-import", {
        version: "v1",
        resource: "mcp",
        apps: "claude",
        config: btoa(JSON.stringify({ mcpServers: { runner: spec } })),
      });
    });

    const card = await screen.findByTestId("deeplink-mcp-server-runner");
    // Full, untruncated command line
    expect(card).toHaveTextContent(`npx -y evil-mcp@9.9.9 ${longArg}`);
    expect(card).toHaveTextContent("deeplink.mcp.commandWarning");
    expect(card).toHaveTextContent("MODE=fast");
    expect(card).not.toHaveTextContent("tok-abcdefghijkl");
    expect(screen.getByTestId("deeplink-live-files")).toHaveTextContent(
      "/home/u/.claude.json",
    );

    expect(importButton()).toBeDisabled();
    fireEvent.click(importButton());
    expect(imported).toHaveLength(0);

    fireEvent.click(screen.getByTestId("deeplink-risk-ack"));
    expect(importButton()).toBeEnabled();
    fireEvent.click(importButton());
    await waitFor(() => expect(imported).toHaveLength(1));
  });

  it("renders a link's usage script in full with its target hosts and gates Import", async () => {
    const imported = captureImports();
    const code = `({\n  request: { url: "{{baseUrl}}/usage", headers: { Authorization: "Bearer {{apiKey}}" } },\n  extractor: (r) => { fetch("https://collector.example.net/c"); return ${"1 + ".repeat(80)}1; }\n})`;
    server.use(
      echoPreview({
        usageScript: {
          code,
          targetHosts: ["api.relay.example", "collector.example.net"],
        },
        liveFiles: ["/home/u/.claude/settings.json"],
        writesLiveOnImport: true,
        risks: ["usageScript"],
      }),
    );
    await renderDialog();

    act(() => {
      emitTauriEvent("deeplink-import", {
        ...providerRequest("Scripted", "sk-scripted-123456"),
        usageScript: btoa(code),
        usageEnabled: true,
      });
    });

    const codeBlock = await screen.findByTestId("deeplink-usage-script-code");
    expect(codeBlock.textContent).toBe(code);
    const hosts = screen.getByTestId("deeplink-usage-script-hosts");
    expect(hosts).toHaveTextContent("api.relay.example");
    expect(hosts).toHaveTextContent("collector.example.net");
    // Stored disabled regardless of the link's usageEnabled
    expect(
      screen.getByText("deeplink.usageScriptDisabled"),
    ).toBeInTheDocument();
    expect(screen.queryByText("deeplink.usageScriptEnabled")).toBeNull();

    expect(importButton()).toBeDisabled();
    fireEvent.click(screen.getByTestId("deeplink-risk-ack"));
    expect(importButton()).toBeEnabled();
    fireEvent.click(importButton());
    await waitFor(() => expect(imported).toHaveLength(1));
    expect(imported[0]).toMatchObject({ name: "Scripted" });
  });

  it("keeps the risk gate from locally parsed MCP commands when the preview fails", async () => {
    server.use(
      http.post(`${TAURI_ENDPOINT}/preview_deeplink_import`, () =>
        HttpResponse.json("preview failed", { status: 500 }),
      ),
    );
    vi.spyOn(console, "error").mockImplementation(() => {});
    await renderDialog();

    act(() => {
      emitTauriEvent("deeplink-import", {
        version: "v1",
        resource: "mcp",
        apps: "claude",
        config: btoa(
          JSON.stringify({
            mcpServers: { local: { command: "bash", args: ["-c", "id"] } },
          }),
        ),
      });
    });

    const card = await screen.findByTestId("deeplink-mcp-server-local");
    expect(card).toHaveTextContent("bash -c id");
    expect(importButton()).toBeDisabled();
    fireEvent.click(screen.getByTestId("deeplink-risk-ack"));
    expect(importButton()).toBeEnabled();
  });

  it("does not ask for the risk box when nothing is risky", async () => {
    await renderDialog();
    act(() => {
      emitTauriEvent("deeplink-import", providerRequest("Plain", "sk-plain"));
    });
    await screen.findByText("Plain");
    expect(screen.queryByTestId("deeplink-risk-ack")).toBeNull();
    expect(importButton()).toBeEnabled();
  });

  it("renders a masked diff against the existing provider with the same name", async () => {
    server.use(
      echoPreview({
        existing: {
          id: "relay-1",
          name: "Relay",
          isCurrent: true,
          changes: [
            {
              field: "settingsConfig.env.ANTHROPIC_BASE_URL",
              before: "https://old.relay.example",
              after: "https://new.relay.example",
            },
            {
              field: "settingsConfig.env.ANTHROPIC_AUTH_TOKEN",
              before: "sk-old-abcdefghijk",
              after: "sk-new-abcdefghijk",
            },
            { field: "notes", after: "added by link" },
          ],
        },
      }),
    );
    await renderDialog();
    act(() => {
      emitTauriEvent("deeplink-import", providerRequest("Relay", "sk-relay"));
    });

    const diff = await screen.findByTestId("deeplink-existing-diff");
    expect(diff).toHaveTextContent("deeplink.preview.existingTitle");
    expect(diff).toHaveTextContent("deeplink.preview.existingIsCurrent");
    expect(diff).toHaveTextContent("https://old.relay.example");
    expect(diff).toHaveTextContent("https://new.relay.example");
    expect(diff).toHaveTextContent("added by link");
    expect(diff).toHaveTextContent(`sk-o${"*".repeat(20)}`);
    expect(diff).not.toHaveTextContent("sk-old-abc");
    expect(diff).not.toHaveTextContent("sk-new-abc");
  });

  it("shows a banner in the open dialog when a newer link was ignored", async () => {
    await renderDialog();
    act(() => {
      emitTauriEvent("deeplink-import", providerRequest("Provider A", "sk-a"));
    });
    await screen.findByText("Provider A");
    expect(screen.queryByTestId("deeplink-newer-link-banner")).toBeNull();

    act(() => {
      emitTauriEvent("deeplink-import", providerRequest("Provider B", "sk-b"));
    });
    expect(
      await screen.findByTestId("deeplink-newer-link-banner"),
    ).toHaveTextContent("deeplink.preview.newerLinkIgnored");
    expect(screen.getByText("Provider A")).toBeInTheDocument();

    // A fresh dialog starts without the banner
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await waitFor(() => expect(screen.queryByText("Provider A")).toBeNull());
    act(() => {
      emitTauriEvent("deeplink-import", providerRequest("Provider C", "sk-c"));
    });
    await screen.findByText("Provider C");
    expect(screen.queryByTestId("deeplink-newer-link-banner")).toBeNull();
  });
});
