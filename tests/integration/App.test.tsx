import { Suspense, type ComponentType } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { providersApi } from "@/lib/api/providers";
import {
  resetProviderState,
  setCurrentProviderId,
  setLiveProviderIds,
  setProviders,
} from "../msw/state";
import { emitTauriEvent } from "../msw/tauriMocks";
import { server } from "../msw/server";
import { http, HttpResponse } from "msw";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/components/providers/ProviderList", () => ({
  ProviderList: ({
    providers,
    currentProviderId,
    onSwitch,
    onEdit,
    onDuplicate,
    onConfigureUsage,
    onOpenWebsite,
    onCreate,
    onDelete,
    onRemoveFromConfig,
  }: any) => (
    <div>
      <div data-testid="provider-list">{JSON.stringify(providers)}</div>
      <div data-testid="current-provider">{currentProviderId}</div>
      <button onClick={() => onSwitch(providers[currentProviderId])}>
        switch
      </button>
      <button onClick={() => onEdit(providers[currentProviderId])}>edit</button>
      <button onClick={() => onDuplicate(providers[currentProviderId])}>
        duplicate
      </button>
      <button onClick={() => onConfigureUsage(providers[currentProviderId])}>
        usage
      </button>
      <button onClick={() => onOpenWebsite("https://example.com")}>
        open-website
      </button>
      <button onClick={() => onCreate?.()}>create</button>
      <button onClick={() => onDelete?.(providers[currentProviderId])}>
        delete
      </button>
      <button
        onClick={() => onRemoveFromConfig?.(providers[currentProviderId])}
      >
        remove-from-config
      </button>
    </div>
  ),
}));

vi.mock("@/components/providers/AddProviderDialog", () => ({
  AddProviderDialog: ({ open, onOpenChange, onSubmit, appId }: any) =>
    open ? (
      <div data-testid="add-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              name: `New ${appId} Provider`,
              settingsConfig: {},
              category: "custom",
              sortIndex: 99,
            })
          }
        >
          confirm-add
        </button>
        <button onClick={() => onOpenChange(false)}>close-add</button>
      </div>
    ) : null,
}));

vi.mock("@/components/providers/EditProviderDialog", () => ({
  EditProviderDialog: ({ open, provider, onSubmit, onOpenChange }: any) =>
    open ? (
      <div data-testid="edit-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              provider: {
                ...provider,
                name: `${provider.name}-edited`,
              },
              originalId: provider.id,
            })
          }
        >
          confirm-edit
        </button>
        <button onClick={() => onOpenChange(false)}>close-edit</button>
      </div>
    ) : null,
}));

vi.mock("@/components/UsageScriptModal", () => ({
  default: ({ isOpen, provider, onSave, onClose }: any) =>
    isOpen ? (
      <div data-testid="usage-modal">
        <span data-testid="usage-provider">{provider?.id}</span>
        <button onClick={() => onSave("script-code")}>save-script</button>
        <button onClick={() => onClose()}>close-usage</button>
      </div>
    ) : null,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({ isOpen, onConfirm, onCancel }: any) =>
    isOpen ? (
      <div data-testid="confirm-dialog">
        <button onClick={() => onConfirm()}>confirm-delete</button>
        <button onClick={() => onCancel()}>cancel-delete</button>
      </div>
    ) : null,
}));

vi.mock("@/components/AppSwitcher", () => ({
  AppSwitcher: ({ activeApp, onSwitch }: any) => (
    <div data-testid="app-switcher">
      <span>{activeApp}</span>
      <button onClick={() => onSwitch("claude")}>switch-claude</button>
      <button onClick={() => onSwitch("codex")}>switch-codex</button>
      <button onClick={() => onSwitch("openclaw")}>switch-openclaw</button>
    </div>
  ),
}));

vi.mock("@/components/UpdateBadge", () => ({
  UpdateBadge: ({ onClick }: any) => (
    <button onClick={onClick}>update-badge</button>
  ),
}));

vi.mock("@/components/mcp/McpPanel", () => ({
  default: ({ open, onOpenChange }: any) =>
    open ? (
      <div data-testid="mcp-panel">
        <button onClick={() => onOpenChange(false)}>close-mcp</button>
      </div>
    ) : (
      <button onClick={() => onOpenChange(true)}>open-mcp</button>
    ),
}));

const mcpOpenAddMock = vi.fn();
const mcpOpenImportMock = vi.fn();

vi.mock("@/components/mcp/UnifiedMcpPanel", async () => {
  const React = await import("react");
  return {
    default: React.forwardRef((_props: unknown, ref) => {
      React.useImperativeHandle(ref, () => ({
        openAdd: mcpOpenAddMock,
        openImport: mcpOpenImportMock,
      }));
      return <div data-testid="mcp-page" />;
    }),
  };
});

vi.mock("@/components/settings/SettingsPage", () => ({
  SettingsPage: ({ defaultTab, onOpenChange }: any) => (
    <div data-testid="settings-page">
      <span data-testid="settings-tab">{defaultTab}</span>
      <button onClick={() => onOpenChange(false)}>close-settings</button>
    </div>
  ),
}));

const renderApp = (
  AppComponent: ComponentType,
  client: QueryClient = new QueryClient(),
) => {
  return render(
    <QueryClientProvider client={client}>
      <Suspense fallback={<div data-testid="loading">loading</div>}>
        <AppComponent />
      </Suspense>
    </QueryClientProvider>,
  );
};

// The full App mounts every page and query; under a busy full-suite run it can
// exceed the 5 s default even though it passes comfortably when run alone.
describe("App integration with MSW", { timeout: 20_000 }, () => {
  beforeEach(() => {
    // The shell persists the active app / page; start every test fresh.
    localStorage.clear();
    resetProviderState();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
  });

  it("covers basic provider flows via real hooks", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );

    fireEvent.click(screen.getByText("usage"));
    expect(await screen.findByTestId("usage-modal")).toBeInTheDocument();
    fireEvent.click(screen.getByText("save-script"));
    fireEvent.click(screen.getByText("close-usage"));

    fireEvent.click(screen.getByText("create"));
    expect(
      await screen.findByTestId("add-provider-dialog"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-add"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(
        /New codex Provider/,
      ),
    );

    fireEvent.click(screen.getByText("edit"));
    expect(
      await screen.findByTestId("edit-provider-dialog"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-edit"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(
        /-edited/,
      ),
    );

    fireEvent.click(screen.getByText("switch"));
    fireEvent.click(screen.getByText("duplicate"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(/copy/),
    );

    fireEvent.click(screen.getByText("open-website"));

    emitTauriEvent("provider-switched", {
      appType: "codex",
      providerId: "codex-2",
    });

    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(toastSuccessMock).toHaveBeenCalled();
  });

  it("shows toast when auto sync fails in background", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    expect(() => {
      emitTauriEvent("webdav-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("webdav-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "network timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });

    toastErrorMock.mockReset();
    expect(() => {
      emitTauriEvent("s3-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("s3-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "s3 timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });
  });

  it("duplicates openclaw providers with a generated key that avoids live-only ids", async () => {
    setProviders("openclaw", {
      deepseek: {
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: {
          baseUrl: "https://api.deepseek.com",
          apiKey: "test-key",
          api: "openai-completions",
          models: [],
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("openclaw", "deepseek");
    setLiveProviderIds("openclaw", ["deepseek-copy"]);

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-openclaw"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "deepseek",
      ),
    );

    fireEvent.click(screen.getByText("duplicate"));

    await waitFor(() => {
      const providerList = screen.getByTestId("provider-list").textContent;
      expect(providerList).toContain("deepseek-copy-2");
      expect(providerList).toContain("DeepSeek copy");
    });

    expect(toastErrorMock).not.toHaveBeenCalledWith(
      expect.stringContaining("Provider key is required for openclaw"),
    );
  });

  it("shows toast when duplicate cannot load live provider ids", async () => {
    setProviders("openclaw", {
      deepseek: {
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: {
          baseUrl: "https://api.deepseek.com",
          apiKey: "test-key",
          api: "openai-completions",
          models: [],
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("openclaw", "deepseek");

    const liveIdsSpy = vi
      .spyOn(providersApi, "getOpenClawLiveProviderIds")
      .mockRejectedValueOnce(new Error("broken config"));

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-openclaw"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "deepseek",
      ),
    );

    fireEvent.click(screen.getByText("duplicate"));

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        expect.stringContaining("Failed to read provider keys from the config"),
      );
    });

    expect(screen.getByTestId("provider-list").textContent).not.toContain(
      "deepseek-copy",
    );

    liveIdsSpy.mockRestore();
  });

  it("closes the confirm dialog and toasts when remove-from-config fails (PRODUCT-73)", async () => {
    setProviders("openclaw", {
      deepseek: {
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: { baseUrl: "https://api.deepseek.com", models: [] },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("openclaw", "deepseek");
    const removeSpy = vi
      .spyOn(providersApi, "removeFromLiveConfig")
      .mockRejectedValueOnce(new Error("unparsable openclaw.json"));

    const { default: App } = await import("@/App");
    renderApp(App);
    fireEvent.click(screen.getByText("switch-openclaw"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "deepseek",
      ),
    );

    fireEvent.click(screen.getByText("remove-from-config"));
    expect(await screen.findByTestId("confirm-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-delete"));
    // A second click while the first call is pending is ignored.
    fireEvent.click(screen.getByText("confirm-delete"));

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        expect.stringContaining("unparsable openclaw.json"),
      ),
    );
    await waitFor(() =>
      expect(screen.queryByTestId("confirm-dialog")).not.toBeInTheDocument(),
    );
    expect(removeSpy).toHaveBeenCalledTimes(1);
    removeSpy.mockRestore();
  });

  it("closes the confirm dialog when deleting a provider fails (PRODUCT-73)", async () => {
    server.use(
      http.post("http://tauri.local/delete_provider", () =>
        HttpResponse.json("cannot delete current provider", { status: 400 }),
      ),
    );
    const { default: App } = await import("@/App");
    renderApp(App);
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    fireEvent.click(screen.getByText("delete"));
    expect(await screen.findByTestId("confirm-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-delete"));

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.queryByTestId("confirm-dialog")).not.toBeInTheDocument(),
    );
    expect(screen.getByTestId("provider-list").textContent).toContain(
      "claude-1",
    );
  });

  it("shows an error state with retry, not the empty list, when get_providers fails", async () => {
    let fail = true;
    server.use(
      http.post("http://tauri.local/get_providers", () =>
        fail
          ? HttpResponse.text("database is locked", { status: 500 })
          : undefined,
      ),
    );

    const { default: App } = await import("@/App");
    renderApp(
      App,
      new QueryClient({ defaultOptions: { queries: { retry: false } } }),
    );

    expect(await screen.findByTestId("providers-error")).toHaveTextContent(
      "database is locked",
    );
    expect(screen.queryByTestId("provider-list")).not.toBeInTheDocument();

    fail = false;
    fireEvent.click(screen.getByText("Retry"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );
  });

  it("navigates to pages that own their header actions, and back", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    // Toolbar shortcut from the app registry opens the MCP page.
    fireEvent.click(screen.getByTitle("MCP Management"));
    expect(await screen.findByTestId("mcp-page")).toBeInTheDocument();
    expect(screen.getByText("MCP Server Management").tagName).toBe("H1");
    // Sub-page headers link to their manual page (DOCS-16).
    expect(
      screen.getByRole("button", { name: "Open the manual", hidden: true }),
    ).toBeInTheDocument();
    expect(localStorage.getItem("cc-switch-last-view")).toBe("mcp");

    // Header actions are rendered by the page and reach its panel handle.
    fireEvent.click(await screen.findByText("Add MCP"));
    expect(mcpOpenAddMock).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByText("Import Existing"));
    expect(mcpOpenImportMock).toHaveBeenCalledTimes(1);

    // Escape returns to the providers page.
    fireEvent.keyDown(window, { key: "Escape" });
    expect(await screen.findByTestId("provider-list")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByText("Add MCP")).not.toBeInTheDocument(),
    );

    // The update badge deep-links into the settings "about" tab.
    fireEvent.click(screen.getByText("update-badge"));
    expect(await screen.findByTestId("settings-tab")).toHaveTextContent(
      "about",
    );
    fireEvent.click(screen.getByText("close-settings"));
    expect(await screen.findByTestId("provider-list")).toBeInTheDocument();

    // PRODUCT-55: with the routing toggle off, the header still offers routing.
    fireEvent.click(
      screen.getByRole("button", {
        name: "Local routing and failover",
        hidden: true,
      }),
    );
    expect(await screen.findByTestId("settings-tab")).toHaveTextContent(
      "proxy",
    );
    fireEvent.click(screen.getByText("close-settings"));
    expect(await screen.findByTestId("provider-list")).toBeInTheDocument();

    // Cmd/Ctrl+, opens settings on the general tab.
    fireEvent.keyDown(window, { key: ",", ctrlKey: true });
    expect(await screen.findByTestId("settings-tab")).toHaveTextContent(
      "general",
    );
  });

  it("restores the last page and app from storage", async () => {
    localStorage.setItem("cc-switch-last-app", "codex");
    localStorage.setItem("cc-switch-last-view", "mcp");

    const { default: App } = await import("@/App");
    renderApp(App);

    expect(await screen.findByTestId("mcp-page")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Back"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );
  });
});
