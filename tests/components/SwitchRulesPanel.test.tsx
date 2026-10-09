import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SwitchRulesPanel } from "@/components/settings/SwitchRulesPanel";
import type { SwitchRule, SwitchRuleState } from "@/lib/api/switchRules";

const apiMock = vi.hoisted(() => ({
  list: vi.fn(),
  upsert: vi.fn(),
  delete: vi.fn(),
  setEnabled: vi.fn(),
  getStates: vi.fn(),
  cancelRevert: vi.fn(),
  evaluateNow: vi.fn(),
}));
const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
}));

vi.mock("@/lib/api/switchRules", () => ({ switchRulesApi: apiMock }));
vi.mock("sonner", () => ({ toast: toastMock }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown> | string) => {
      if (typeof options === "string") return options;
      let text = (options?.defaultValue as string | undefined) ?? key;
      for (const [name, value] of Object.entries(options ?? {})) {
        text = text.split(`{{${name}}}`).join(String(value));
      }
      return text;
    },
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/queries", () => ({
  useProvidersQuery: () => ({
    data: {
      providers: {
        official: { id: "official", name: "Anthropic", category: "official" },
        backup: { id: "backup", name: "Kimi Relay" },
        kimi: {
          id: "kimi",
          name: "Kimi Plan",
          meta: { usage_script: { templateType: "token_plan" } },
        },
      },
      currentProviderId: "official",
    },
  }),
}));

// Radix Select 依赖指针事件；用透传组件代替，每个选项渲染为按钮
vi.mock("@/components/ui/select", async () => {
  const React = await import("react");
  const Ctx = React.createContext<(value: string) => void>(() => {});
  return {
    Select: ({ value, onValueChange, children }: any) => (
      <Ctx.Provider value={onValueChange}>
        <div data-testid="select" data-value={value}>
          {children}
        </div>
      </Ctx.Provider>
    ),
    SelectTrigger: ({ children, ...props }: any) => (
      <button type="button" {...props}>
        {children}
      </button>
    ),
    SelectValue: ({ placeholder }: any) => <span>{placeholder ?? null}</span>,
    SelectContent: ({ children }: any) => <div>{children}</div>,
    SelectItem: ({ value, children }: any) => {
      const set = React.useContext(Ctx);
      return (
        <button type="button" onClick={() => set(value)}>
          {children}
        </button>
      );
    },
  };
});

const rule: SwitchRule = {
  id: "r1",
  appType: "claude",
  source: "subscription",
  tierName: "five_hour",
  thresholdPct: 90,
  targetProviderId: "backup",
  revertOnReset: true,
  enabled: true,
  sortIndex: 0,
  lastFiredAt: 1_800_000_000,
  createdAt: 1,
};

const armed: SwitchRuleState = {
  ruleId: "r1",
  firedAt: 1_800_000_000,
  resetsAt: 1_800_010_000,
  switchedFrom: "official",
  switchedTo: "backup",
  lastUtilization: 92,
  reason: "five_hour 92%",
};

const renderPanel = (disabled = false) => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <SwitchRulesPanel appType="claude" disabled={disabled} />
    </QueryClientProvider>,
  );
};

describe("SwitchRulesPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.list.mockResolvedValue([]);
    apiMock.getStates.mockResolvedValue([]);
    apiMock.upsert.mockImplementation(async (draft) => ({
      ...rule,
      ...draft,
      id: "new",
    }));
    apiMock.delete.mockResolvedValue(true);
    apiMock.setEnabled.mockResolvedValue(true);
    apiMock.cancelRevert.mockResolvedValue(true);
    apiMock.evaluateNow.mockResolvedValue(0);
  });

  it("shows the empty state and saves a subscription rule from the editor", async () => {
    renderPanel();
    await screen.findByText(/还没有规则/);

    fireEvent.click(screen.getByRole("button", { name: /添加规则/ }));
    expect(screen.getByTestId("switch-rule-editor")).toBeInTheDocument();

    // official providers are never offered as the fallback target
    expect(screen.queryByRole("button", { name: "Anthropic" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "five_hour" }));
    fireEvent.click(screen.getByRole("button", { name: "Kimi Relay" }));
    fireEvent.change(screen.getByLabelText(/触发阈值/), {
      target: { value: "85" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(apiMock.upsert).toHaveBeenCalledWith({
        appType: "claude",
        source: "subscription",
        watchedProviderId: undefined,
        tierName: "five_hour",
        thresholdPct: 85,
        targetProviderId: "backup",
        revertOnReset: true,
        enabled: true,
      }),
    );
    expect(toastMock.success).toHaveBeenCalledWith("规则已保存");
    await waitFor(() =>
      expect(screen.queryByTestId("switch-rule-editor")).toBeNull(),
    );
  });

  it("rejects an out-of-range threshold before calling the backend", async () => {
    renderPanel();
    await screen.findByText(/还没有规则/);
    fireEvent.click(screen.getByRole("button", { name: /添加规则/ }));
    fireEvent.click(screen.getByRole("button", { name: "five_hour" }));
    fireEvent.click(screen.getByRole("button", { name: "Kimi Relay" }));
    fireEvent.change(screen.getByLabelText(/触发阈值/), {
      target: { value: "250" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith("阈值需在 1 到 100 之间"),
    );
    expect(apiMock.upsert).not.toHaveBeenCalled();
  });

  it("switches to coding-plan tiers when a plan provider is watched", async () => {
    renderPanel();
    await screen.findByText(/还没有规则/);
    fireEvent.click(screen.getByRole("button", { name: /添加规则/ }));
    fireEvent.click(
      screen.getByRole("button", { name: /Kimi Plan（Coding Plan）/ }),
    );

    expect(
      screen.getByRole("button", { name: "weekly_limit" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "seven_day_opus" })).toBeNull();
    // the watched provider cannot also be the target
    expect(screen.queryByRole("button", { name: "Kimi Plan" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "weekly_limit" }));
    fireEvent.click(screen.getByRole("button", { name: "Kimi Relay" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(apiMock.upsert).toHaveBeenCalledWith(
        expect.objectContaining({
          source: "coding_plan",
          watchedProviderId: "kimi",
          tierName: "weekly_limit",
        }),
      ),
    );
  });

  it("lists rules with their armed state and wires toggle, cancel and delete", async () => {
    apiMock.list.mockResolvedValue([rule]);
    apiMock.getStates.mockResolvedValue([armed]);
    renderPanel();

    const row = await screen.findByTestId("switch-rule-r1");
    expect(row).toHaveTextContent(
      "官方订阅 的 five_hour ≥ 90% 时切换到 Kimi Relay",
    );
    expect(screen.getByTestId("switch-rule-state-r1")).toHaveTextContent(
      "five_hour 92%",
    );
    expect(screen.getByTestId("switch-rule-state-r1")).toHaveTextContent(
      "Anthropic → Kimi Relay",
    );

    fireEvent.click(screen.getByRole("button", { name: "保持当前" }));
    await waitFor(() =>
      expect(apiMock.cancelRevert).toHaveBeenCalledWith("r1"),
    );

    fireEvent.click(screen.getByRole("switch", { name: "启用规则" }));
    await waitFor(() =>
      expect(apiMock.setEnabled).toHaveBeenCalledWith("r1", false),
    );

    fireEvent.click(screen.getByRole("button", { name: "删除规则" }));
    await waitFor(() => expect(apiMock.delete).toHaveBeenCalledWith("r1"));
  });

  it("evaluates rules on demand and reports the outcome", async () => {
    apiMock.list.mockResolvedValue([rule]);
    apiMock.evaluateNow.mockResolvedValue(1);
    renderPanel();
    await screen.findByTestId("switch-rule-r1");

    fireEvent.click(screen.getByRole("button", { name: /立即评估/ }));
    await waitFor(() =>
      expect(apiMock.evaluateNow).toHaveBeenCalledWith("claude"),
    );
    expect(toastMock.success).toHaveBeenCalledWith("已评估，触发了 1 条规则");
  });

  it("disables actions while the master toggle is off", async () => {
    apiMock.list.mockResolvedValue([rule]);
    renderPanel(true);
    await screen.findByTestId("switch-rule-r1");
    expect(screen.getByRole("button", { name: /添加规则/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /立即评估/ })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "启用规则" })).toBeDisabled();
  });
});
