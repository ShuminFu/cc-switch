import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";

const apiMocks = vi.hoisted(() => ({
  getCurrent: vi.fn(),
  getLiveProviderSettings: vi.fn(),
  getOpenClawLiveProvider: vi.fn(),
}));

// 记录 ProviderForm 每次挂载时拿到的 initialData，用于断言表单只以最终配置挂载一次
const formMounts = vi.hoisted(() => [] as Array<Record<string, unknown>>);

vi.mock("@/lib/api", () => ({
  providersApi: {
    getCurrent: apiMocks.getCurrent,
  },
  vscodeApi: {
    getLiveProviderSettings: apiMocks.getLiveProviderSettings,
  },
  openclawApi: {
    getLiveProvider: apiMocks.getOpenClawLiveProvider,
  },
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
    footer,
  }: {
    isOpen: boolean;
    children: React.ReactNode;
    footer?: React.ReactNode;
  }) =>
    isOpen ? (
      <div>
        <div>{children}</div>
        <div>{footer}</div>
      </div>
    ) : null,
}));

vi.mock("@/components/providers/forms/ProviderForm", async () => {
  const { useEffect } = await import("react");
  return {
    ProviderForm: ({
      initialData,
      onSubmit,
      isProxyTakeover,
    }: {
      initialData: {
        name?: string;
        websiteUrl?: string;
        notes?: string;
        settingsConfig?: Record<string, unknown>;
        meta?: Record<string, unknown>;
        icon?: string;
        iconColor?: string;
      };
      onSubmit: (values: {
        name: string;
        websiteUrl: string;
        notes?: string;
        settingsConfig: string;
        meta?: Record<string, unknown>;
        icon?: string;
        iconColor?: string;
      }) => void;
      isProxyTakeover?: boolean;
    }) => {
      useEffect(() => {
        formMounts.push(initialData.settingsConfig ?? {});
        // eslint-disable-next-line react-hooks/exhaustive-deps
      }, []);
      return (
        <form
          id="provider-form"
          onSubmit={(event) => {
            event.preventDefault();
            onSubmit({
              name: initialData.name ?? "",
              websiteUrl: initialData.websiteUrl ?? "",
              notes: initialData.notes,
              settingsConfig: JSON.stringify(initialData.settingsConfig ?? {}),
              meta: initialData.meta,
              icon: initialData.icon,
              iconColor: initialData.iconColor,
            });
          }}
        >
          <output data-testid="settings-config">
            {JSON.stringify(initialData.settingsConfig ?? {})}
          </output>
          <output data-testid="is-proxy-takeover">
            {isProxyTakeover ? "true" : "false"}
          </output>
        </form>
      );
    },
  };
});

import { EditProviderDialog } from "@/components/providers/EditProviderDialog";

describe("EditProviderDialog", () => {
  beforeEach(() => {
    apiMocks.getCurrent.mockReset();
    apiMocks.getLiveProviderSettings.mockReset();
    apiMocks.getOpenClawLiveProvider.mockReset();
    formMounts.length = 0;
  });

  it("保留 Codex 数据库中的 modelCatalog，避免 live 配置缺字段时清空模型映射", async () => {
    const dbModelCatalog = {
      models: [
        {
          model: "deepseek-v4-flash",
          displayName: "DeepSeek V4 Flash",
          contextWindow: 1000000,
        },
      ],
    };
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "aggregator",
      settingsConfig: {
        auth: {
          OPENAI_API_KEY: "db-key",
        },
        config: 'model_provider = "custom"\nmodel = "deepseek-v4-flash"\n',
        modelCatalog: dbModelCatalog,
      },
    };
    const liveSettings = {
      auth: {
        OPENAI_API_KEY: "live-key",
      },
      config: 'model_provider = "custom"\nmodel = "deepseek-v4-pro"\n',
    };
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockResolvedValue(liveSettings);

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={handleSubmit}
        appId="codex"
      />,
    );

    await waitFor(() => {
      expect(
        JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
      ).toEqual({
        ...liveSettings,
        modelCatalog: dbModelCatalog,
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    expect(handleSubmit.mock.calls[0][0].provider.settingsConfig).toEqual({
      ...liveSettings,
      modelCatalog: dbModelCatalog,
    });
  });

  it("代理接管中编辑 Codex 供应商时展示数据库配置而不是读取 live 代理配置", async () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "custom",
      settingsConfig: {
        auth: {
          OPENAI_API_KEY: "db-key",
        },
        config:
          'model_provider = "custom"\n[model_providers.custom]\nbase_url = "https://api.deepseek.com/v1"\n',
      },
    };

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockResolvedValue({
      auth: {
        OPENAI_API_KEY: "PROXY_MANAGED",
      },
      config:
        'model_provider = "custom"\n[model_providers.custom]\nbase_url = "http://127.0.0.1:15721/v1"\nexperimental_bearer_token = "PROXY_MANAGED"\n',
    });

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
        isProxyTakeover
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("is-proxy-takeover").textContent).toBe("true");
    });

    expect(apiMocks.getLiveProviderSettings).not.toHaveBeenCalled();
    expect(
      JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
    ).toEqual(provider.settingsConfig);
  });

  it("当前供应商：等待 live 读取完成后才渲染表单，并以 live 值初始化（不会先渲染数据库值再重置）", async () => {
    const provider: Provider = {
      id: "claude-main",
      name: "Claude Main",
      settingsConfig: { env: { ANTHROPIC_BASE_URL: "https://db.example" } },
    };
    const liveSettings = {
      env: { ANTHROPIC_BASE_URL: "https://live.example" },
    };
    let resolveLive: (value: unknown) => void = () => {};
    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockReturnValue(
      new Promise((resolve) => {
        resolveLive = resolve;
      }),
    );

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="claude"
      />,
    );

    await waitFor(() =>
      expect(apiMocks.getLiveProviderSettings).toHaveBeenCalledTimes(1),
    );
    expect(screen.getByTestId("edit-provider-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("settings-config")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
    expect(formMounts).toHaveLength(0);

    await act(async () => {
      resolveLive(liveSettings);
    });

    expect(
      JSON.parse(
        (await screen.findByTestId("settings-config")).textContent ?? "{}",
      ),
    ).toEqual(liveSettings);
    expect(screen.queryByTestId("edit-provider-loading")).toBeNull();
    expect(formMounts).toEqual([liveSettings]);
  });

  it("非当前供应商：不读取 live，直接以数据库配置渲染表单", async () => {
    const provider: Provider = {
      id: "backup",
      name: "Backup",
      settingsConfig: { env: { ANTHROPIC_BASE_URL: "https://db.example" } },
    };
    apiMocks.getCurrent.mockResolvedValue("someone-else");

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="claude"
      />,
    );

    expect(
      JSON.parse(
        (await screen.findByTestId("settings-config")).textContent ?? "{}",
      ),
    ).toEqual(provider.settingsConfig);
    expect(apiMocks.getLiveProviderSettings).not.toHaveBeenCalled();
    expect(formMounts).toEqual([provider.settingsConfig]);
  });

  it("live 返回空值或读取失败时仍结束加载并回退到数据库配置", async () => {
    const provider: Provider = {
      id: "claude-main",
      name: "Claude Main",
      settingsConfig: { env: { ANTHROPIC_BASE_URL: "https://db.example" } },
    };
    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockResolvedValue(null);

    const { unmount } = render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="claude"
      />,
    );

    expect(
      JSON.parse(
        (await screen.findByTestId("settings-config")).textContent ?? "{}",
      ),
    ).toEqual(provider.settingsConfig);
    unmount();

    apiMocks.getLiveProviderSettings.mockRejectedValue(new Error("boom"));
    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="claude"
      />,
    );

    expect(
      JSON.parse(
        (await screen.findByTestId("settings-config")).textContent ?? "{}",
      ),
    ).toEqual(provider.settingsConfig);
  });

  it("OpenClaw：等待 live 条目读取完成后才渲染表单", async () => {
    const provider: Provider = {
      id: "oc-provider",
      name: "OpenClaw Provider",
      settingsConfig: { baseUrl: "https://db.example" },
    };
    const liveEntry = { baseUrl: "https://live.example" };
    let resolveLive: (value: unknown) => void = () => {};
    apiMocks.getOpenClawLiveProvider.mockReturnValue(
      new Promise((resolve) => {
        resolveLive = resolve;
      }),
    );

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="openclaw"
      />,
    );

    expect(screen.getByTestId("edit-provider-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("settings-config")).not.toBeInTheDocument();

    await act(async () => {
      resolveLive(liveEntry);
    });

    expect(
      JSON.parse(
        (await screen.findByTestId("settings-config")).textContent ?? "{}",
      ),
    ).toEqual(liveEntry);
    expect(formMounts).toEqual([liveEntry]);
  });

  it("代理接管模式下不等待任何读取，首次渲染即显示表单", () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      settingsConfig: { auth: { OPENAI_API_KEY: "db-key" } },
    };
    apiMocks.getCurrent.mockReturnValue(new Promise(() => {}));

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
        isProxyTakeover
      />,
    );

    expect(screen.getByTestId("settings-config")).toBeInTheDocument();
    expect(screen.queryByTestId("edit-provider-loading")).toBeNull();
    expect(apiMocks.getCurrent).not.toHaveBeenCalled();
  });
});
