import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import type { ProviderFormValues } from "@/components/providers/forms/ProviderForm";
import type { AppId } from "@/lib/api";

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogHeader: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogTitle: ({ children }: { children: React.ReactNode }) => (
    <h1>{children}</h1>
  ),
  DialogDescription: ({ children }: { children: React.ReactNode }) => (
    <p>{children}</p>
  ),
  DialogFooter: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
}));

let mockFormValues: ProviderFormValues;

vi.mock("@/components/providers/forms/ProviderForm", () => ({
  ProviderForm: ({
    onSubmit,
  }: {
    onSubmit: (values: ProviderFormValues) => void;
  }) => (
    <form
      id="provider-form"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(mockFormValues);
      }}
    />
  ),
}));

describe("AddProviderDialog", () => {
  beforeEach(() => {
    mockFormValues = {
      name: "Test Provider",
      websiteUrl: "https://provider.example.com",
      settingsConfig: JSON.stringify({ env: {}, config: {} }),
      meta: {
        custom_endpoints: {
          "https://api.new-endpoint.com": {
            url: "https://api.new-endpoint.com",
            addedAt: 1,
          },
        },
      },
    };
  });

  it("使用 ProviderForm 返回的自定义端点", async () => {
    const handleSubmit = vi.fn().mockResolvedValue(undefined);
    const handleOpenChange = vi.fn();

    render(
      <AddProviderDialog
        open
        onOpenChange={handleOpenChange}
        appId="claude"
        onSubmit={handleSubmit}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", {
        name: "common.add",
      }),
    );

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));

    const submitted = handleSubmit.mock.calls[0][0];
    expect(submitted.meta?.custom_endpoints).toEqual(
      mockFormValues.meta?.custom_endpoints,
    );
    expect(handleOpenChange).toHaveBeenCalledWith(false);
  });

  it("在缺少自定义端点时回退到配置中的 baseUrl", async () => {
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    mockFormValues = {
      name: "Base URL Provider",
      websiteUrl: "",
      settingsConfig: JSON.stringify({
        env: { ANTHROPIC_BASE_URL: "https://claude.base" },
        config: {},
      }),
    };

    render(
      <AddProviderDialog
        open
        onOpenChange={vi.fn()}
        appId="claude"
        onSubmit={handleSubmit}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", {
        name: "common.add",
      }),
    );

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));

    const submitted = handleSubmit.mock.calls[0][0];
    expect(submitted.meta?.custom_endpoints).toEqual({
      "https://claude.base": {
        url: "https://claude.base",
        addedAt: expect.any(Number),
        lastUsed: undefined,
      },
    });
  });

  it("新建 Grok Build 自定义供应商时不补默认 Grok 图标", async () => {
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    mockFormValues = {
      name: "tes 1",
      websiteUrl: "",
      icon: "",
      iconColor: "",
      settingsConfig: JSON.stringify({
        config: `[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "https://grok.example.com/v1"
name = "tes 1"
api_key = "secret"
api_backend = "responses"
context_window = 500000
`,
      }),
    };

    render(
      <AddProviderDialog
        open
        onOpenChange={vi.fn()}
        appId="grokbuild"
        onSubmit={handleSubmit}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "common.add" }));

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));

    const submitted = handleSubmit.mock.calls[0][0];
    expect(submitted.icon).toBeUndefined();
    expect(submitted.iconColor).toBeUndefined();
  });

  const submitFor = async (appId: AppId) => {
    const handleSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <AddProviderDialog
        open
        onOpenChange={vi.fn()}
        appId={appId}
        onSubmit={handleSubmit}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "common.add" }));
    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    return handleSubmit.mock.calls[0][0];
  };

  it.each([
    ["opencode", { options: { baseURL: "https://opencode.base/" } }],
    ["openclaw", { baseUrl: "https://openclaw.base" }],
    ["hermes", { base_url: "https://hermes.base" }],
    ["gemini", { env: { GOOGLE_GEMINI_BASE_URL: "https://gemini.base" } }],
  ] as const)(
    "falls back to the %s config base URL for custom endpoints",
    async (appId, config) => {
      mockFormValues = {
        name: "Base URL Provider",
        websiteUrl: "",
        settingsConfig: JSON.stringify(config),
      };

      const submitted = await submitFor(appId);
      const [url] = Object.keys(submitted.meta?.custom_endpoints ?? {});
      expect(url).toMatch(/^https:\/\/[a-z]+\.base$/);
    },
  );

  it.each([
    ["claude", true],
    ["codex", true],
    ["gemini", true],
    ["claude-desktop", false],
    ["grokbuild", false],
    ["opencode", false],
    ["openclaw", false],
    ["hermes", false],
  ] as const)("shows the universal tab for %s: %s", (appId, shown) => {
    render(
      <AddProviderDialog
        open
        onOpenChange={vi.fn()}
        appId={appId}
        onSubmit={vi.fn()}
      />,
    );
    expect(screen.queryByText("provider.tabUniversal") !== null).toBe(shown);
  });

  it("passes suggested defaults through only for OpenClaw", async () => {
    const suggestedDefaults = { model: { primary: "p/model" } };
    mockFormValues = {
      name: "Defaults",
      websiteUrl: "",
      settingsConfig: JSON.stringify({ baseUrl: "" }),
      suggestedDefaults,
    } as unknown as ProviderFormValues;

    expect((await submitFor("openclaw")).suggestedDefaults).toEqual(
      suggestedDefaults,
    );
  });

  it("does not pass suggested defaults for other apps", async () => {
    mockFormValues = {
      name: "Defaults",
      websiteUrl: "",
      settingsConfig: JSON.stringify({ base_url: "" }),
      suggestedDefaults: { model: { primary: "p/model" } },
    } as unknown as ProviderFormValues;

    expect((await submitFor("hermes")).suggestedDefaults).toBeUndefined();
  });
});
