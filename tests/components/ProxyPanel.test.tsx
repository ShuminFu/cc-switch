import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProxyPanel } from "@/components/proxy/ProxyPanel";
import type { GlobalProxyConfig, ProxyStatus } from "@/types/proxy";

const mocks = vi.hoisted(() => ({
  proxyStatus: {
    status: undefined as ProxyStatus | undefined,
    isRunning: false,
  },
  takeoverStatus: {} as Record<string, boolean>,
  globalConfig: undefined as GlobalProxyConfig | undefined,
  queues: {} as Record<
    string,
    Array<{ providerId: string; providerName: string }>
  >,
  setTakeover: vi.fn(),
  updateGlobalConfig: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    success: mocks.toastSuccess,
    error: mocks.toastError,
  },
}));

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => mocks.proxyStatus,
}));

vi.mock("@/lib/query/proxy", () => ({
  useProxyTakeoverStatus: () => ({ data: mocks.takeoverStatus }),
  useSetProxyTakeoverForApp: () => ({
    mutateAsync: mocks.setTakeover,
    isPending: false,
  }),
  useGlobalProxyConfig: () => ({ data: mocks.globalConfig }),
  useUpdateGlobalProxyConfig: () => ({
    mutateAsync: mocks.updateGlobalConfig,
    isPending: false,
  }),
}));

vi.mock("@/lib/query/failover", () => ({
  useFailoverQueue: (appType: string) => ({
    data: mocks.queues[appType] ?? [],
  }),
  useProviderHealth: () => ({
    data: { consecutive_failures: 0, is_healthy: true },
  }),
}));

const baseConfig: GlobalProxyConfig = {
  proxyEnabled: true,
  listenAddress: "127.0.0.1",
  listenPort: 15721,
  enableLogging: true,
};

const runningStatus: ProxyStatus = {
  running: true,
  address: "127.0.0.1",
  port: 15721,
  active_connections: 2,
  total_requests: 40,
  success_requests: 38,
  failed_requests: 2,
  success_rate: 95,
  uptime_seconds: 3725,
  current_provider: null,
  current_provider_id: null,
  last_request_at: null,
  last_error: null,
  failover_count: 0,
  active_targets: [
    { app_type: "claude", provider_name: "Primary", provider_id: "p1" },
  ],
};

type ProxyPanelProps = ComponentProps<typeof ProxyPanel>;

const renderPanel = (overrides: Partial<ProxyPanelProps> = {}) => {
  const props: ProxyPanelProps = {
    enableLocalProxy: true,
    onEnableLocalProxyChange: vi.fn(),
    onToggleProxy: vi.fn().mockResolvedValue(undefined),
    isProxyPending: false,
    ...overrides,
  };
  render(<ProxyPanel {...props} />);
  return props;
};

describe("ProxyPanel", () => {
  beforeEach(() => {
    mocks.proxyStatus = { status: undefined, isRunning: false };
    mocks.takeoverStatus = {};
    mocks.globalConfig = { ...baseConfig };
    mocks.queues = {};
    mocks.setTakeover.mockResolvedValue(undefined);
    mocks.updateGlobalConfig.mockResolvedValue(undefined);
  });

  describe("when the proxy is stopped", () => {
    it("shows the listen settings seeded from the global config", () => {
      mocks.globalConfig = { ...baseConfig, listenPort: 18080 };
      renderPanel();

      expect(
        screen.getByLabelText("proxy.settings.fields.listenAddress.label"),
      ).toHaveValue("127.0.0.1");
      expect(
        screen.getByLabelText("proxy.settings.fields.listenPort.label"),
      ).toHaveValue(18080);
      expect(screen.getByText("proxy.panel.stoppedTitle")).toBeInTheDocument();
      expect(screen.queryByText("proxyConfig.appTakeover")).toBeNull();
    });

    it("warns when the listen address is not loopback", () => {
      renderPanel();
      expect(screen.queryByRole("alert")).toBeNull();

      fireEvent.change(
        screen.getByLabelText("proxy.settings.fields.listenAddress.label"),
        { target: { value: "0.0.0.0" } },
      );

      expect(screen.getByRole("alert")).toHaveTextContent(
        "proxy.settings.fields.listenAddress.lanWarning",
      );
    });

    it("rejects an invalid address without saving", () => {
      renderPanel();
      fireEvent.change(
        screen.getByLabelText("proxy.settings.fields.listenAddress.label"),
        { target: { value: "not-an-ip" } },
      );
      fireEvent.click(screen.getByRole("button", { name: "common.save" }));

      expect(mocks.toastError).toHaveBeenCalledWith(
        "proxy.settings.invalidAddress",
      );
      expect(mocks.updateGlobalConfig).not.toHaveBeenCalled();
    });

    it.each(["80", "70000", "12ab"])(
      "rejects port %s without saving",
      (port) => {
        renderPanel();
        fireEvent.change(
          screen.getByLabelText("proxy.settings.fields.listenPort.label"),
          { target: { value: port } },
        );
        fireEvent.click(screen.getByRole("button", { name: "common.save" }));

        expect(mocks.toastError).toHaveBeenCalledWith(
          "proxy.settings.invalidPort",
        );
        expect(mocks.updateGlobalConfig).not.toHaveBeenCalled();
      },
    );

    it("normalizes localhost and saves a valid address and port", async () => {
      renderPanel();
      fireEvent.change(
        screen.getByLabelText("proxy.settings.fields.listenAddress.label"),
        { target: { value: " localhost " } },
      );
      fireEvent.change(
        screen.getByLabelText("proxy.settings.fields.listenPort.label"),
        { target: { value: "20000" } },
      );
      fireEvent.click(screen.getByRole("button", { name: "common.save" }));

      await waitFor(() => {
        expect(mocks.updateGlobalConfig).toHaveBeenCalledWith({
          ...baseConfig,
          listenAddress: "127.0.0.1",
          listenPort: 20000,
        });
      });
      expect(mocks.toastSuccess).toHaveBeenCalledWith(
        "proxy.settings.configSaved",
        { closeButton: true },
      );
    });

    it("accepts an IPv6 literal", async () => {
      renderPanel();
      fireEvent.change(
        screen.getByLabelText("proxy.settings.fields.listenAddress.label"),
        { target: { value: "::1" } },
      );
      fireEvent.click(screen.getByRole("button", { name: "common.save" }));

      await waitFor(() => {
        expect(mocks.updateGlobalConfig).toHaveBeenCalledWith(
          expect.objectContaining({ listenAddress: "::1" }),
        );
      });
    });

    it("forwards the service switch to onToggleProxy", () => {
      const props = renderPanel();
      // [0] = enable feature row, [1] = proxy service switch
      fireEvent.click(screen.getAllByRole("switch")[1]);

      expect(props.onToggleProxy).toHaveBeenCalledWith(true);
    });
  });

  describe("when the proxy is running", () => {
    beforeEach(() => {
      mocks.proxyStatus = { status: runningStatus, isRunning: true };
    });

    it("shows the service address, active targets and stats", () => {
      renderPanel();

      expect(screen.getByText("http://127.0.0.1:15721")).toBeInTheDocument();
      expect(screen.getByText("Primary")).toBeInTheDocument();
      expect(screen.getByText("95.0%")).toBeInTheDocument();
      expect(screen.getByText("1h 2m 5s")).toBeInTheDocument();
      // The listen settings are only editable while stopped.
      expect(
        screen.queryByLabelText("proxy.settings.fields.listenAddress.label"),
      ).toBeNull();
    });

    it("brackets IPv6 service addresses", () => {
      mocks.proxyStatus = {
        status: { ...runningStatus, address: "::1" },
        isRunning: true,
      };
      renderPanel();

      expect(screen.getByText("http://[::1]:15721")).toBeInTheDocument();
    });

    it("toggles takeover for an app and reports failures", async () => {
      renderPanel();
      expect(screen.getByText("proxyConfig.appTakeover")).toBeInTheDocument();

      const codexRow = screen.getByText("codex").parentElement as HTMLElement;
      fireEvent.click(codexRow.querySelector('[role="switch"]')!);

      await waitFor(() => {
        expect(mocks.setTakeover).toHaveBeenCalledWith({
          appType: "codex",
          enabled: true,
        });
      });
      expect(mocks.toastSuccess).toHaveBeenCalledWith(
        "proxy.takeover.enabled",
        { closeButton: true },
      );

      mocks.setTakeover.mockRejectedValueOnce(new Error("port busy"));
      fireEvent.click(codexRow.querySelector('[role="switch"]')!);

      await waitFor(() => {
        expect(mocks.toastError).toHaveBeenCalledWith("proxy.takeover.failed");
      });
    });

    it("lists the failover queue and marks the active provider", () => {
      mocks.queues = {
        claude: [
          { providerId: "p1", providerName: "Primary" },
          { providerId: "p2", providerName: "Backup" },
        ],
      };
      renderPanel();

      expect(screen.getByText("proxy.failoverQueue.title")).toBeInTheDocument();
      expect(screen.getByText("Backup")).toBeInTheDocument();
      // Active target row + queue row both show "Primary"; only the queue row carries the badge.
      expect(screen.getAllByText("provider.inUse").length).toBeGreaterThan(1);
    });

    it("saves the logging toggle into the global config", async () => {
      renderPanel();
      const loggingRow = screen
        .getByText("proxy.settings.fields.enableLogging.label")
        .closest("div.flex") as HTMLElement;
      fireEvent.click(loggingRow.querySelector('[role="switch"]')!);

      await waitFor(() => {
        expect(mocks.updateGlobalConfig).toHaveBeenCalledWith({
          ...baseConfig,
          enableLogging: false,
        });
      });
    });
  });
});
