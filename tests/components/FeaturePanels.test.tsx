import type { ReactElement } from "react";
import {
  act,
  render,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { FailoverQueueManager } from "@/components/proxy/FailoverQueueManager";
import PromptPanel from "@/components/prompts/PromptPanel";
import HermesMemoryPanel from "@/components/hermes/HermesMemoryPanel";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));

// Stable `t` that honours both `t(key, "default")` and `{ defaultValue }`.
const stableT = vi.hoisted(
  () => (key: string, options?: unknown) =>
    typeof options === "string"
      ? key
      : ((options as { defaultValue?: string } | undefined)?.defaultValue ??
        key),
);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: stableT }),
}));

vi.mock("@/components/MarkdownEditor", () => ({
  default: ({ value, onChange }: any) => (
    <textarea
      data-testid="editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const renderWithQuery = (ui: ReactElement) =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );

beforeEach(() => {
  toastMock.error.mockReset();
  toastMock.success.mockReset();
});

describe("FailoverQueueManager", () => {
  const queue = [
    { providerId: "p1", providerName: "Primary", sortIndex: 0 },
    {
      providerId: "p2",
      providerName: "Backup",
      providerNotes: "eu",
      sortIndex: 1,
    },
  ];

  it("renders the queue in order with its notes", async () => {
    mockInvoke({
      get_auto_failover_enabled: false,
      get_failover_queue: queue,
      get_available_providers_for_failover: [],
    });
    renderWithQuery(<FailoverQueueManager appType="claude" />);

    expect(await screen.findByText("Primary")).toBeInTheDocument();
    expect(screen.getByText("Backup")).toBeInTheDocument();
    expect(screen.getByText("(eu)")).toBeInTheDocument();
    expect(screen.getByText("1")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
  });

  it("reorders by swapping sort indices and shows health, last error, test and reset (FE-UI-24)", async () => {
    const api = mockInvoke({
      get_auto_failover_enabled: true,
      get_failover_queue: [
        { providerId: "p1", providerName: "Primary", sortIndex: 3 },
        { providerId: "p2", providerName: "Backup", sortIndex: 7 },
      ],
      get_available_providers_for_failover: [],
      get_provider_health: ({ providerId }: Record<string, unknown>) => ({
        provider_id: providerId,
        app_type: "claude",
        is_healthy: providerId !== "p2",
        consecutive_failures: providerId === "p2" ? 4 : 0,
        last_success_at: null,
        last_failure_at: providerId === "p2" ? "2026-01-01T00:00:00Z" : null,
        last_error: providerId === "p2" ? "HTTP 529 overloaded" : null,
        updated_at: "2026-01-01T00:00:00Z",
      }),
      update_providers_sort_order: true,
      reset_circuit_breaker: null,
      stream_check_provider: {
        status: "operational",
        success: true,
        message: "ok",
        responseTimeMs: 12,
        testedAt: 0,
        retryCount: 0,
      },
    });
    renderWithQuery(<FailoverQueueManager appType="claude" />);

    expect(await screen.findByTestId("queue-last-error-p2")).toHaveAttribute(
      "title",
      "HTTP 529 overloaded",
    );

    const moveUps = screen.getAllByRole("button", { name: "Move up" });
    const moveDowns = screen.getAllByRole("button", { name: "Move down" });
    expect(moveUps[0]).toBeDisabled();
    expect(moveDowns[1]).toBeDisabled();
    fireEvent.click(moveDowns[0]);
    await waitFor(() =>
      expect(api.callsTo("update_providers_sort_order")).toHaveLength(1),
    );
    expect(api.callsTo("update_providers_sort_order")[0].args).toEqual({
      app: "claude",
      updates: [
        { id: "p1", sortIndex: 7 },
        { id: "p2", sortIndex: 3 },
      ],
    });

    fireEvent.click(
      screen.getAllByRole("button", { name: "Test connectivity" })[1],
    );
    await waitFor(() =>
      expect(api.callsTo("stream_check_provider")).toHaveLength(1),
    );
    expect(api.callsTo("stream_check_provider")[0].args).toMatchObject({
      providerId: "p2",
    });

    // Only the failing provider offers a circuit reset.
    const resets = screen.getAllByRole("button", {
      name: "Reset circuit breaker",
    });
    expect(resets).toHaveLength(1);
    fireEvent.click(resets[0]);
    await waitFor(() =>
      expect(api.callsTo("reset_circuit_breaker")).toHaveLength(1),
    );
  });

  it("shows the empty state when the queue is empty", async () => {
    mockInvoke({
      get_auto_failover_enabled: false,
      get_failover_queue: [],
      get_available_providers_for_failover: [],
    });
    renderWithQuery(<FailoverQueueManager appType="claude" />);
    expect(
      await screen.findByText("proxy.failoverQueue.empty"),
    ).toBeInTheDocument();
  });

  it("surfaces a backend error instead of an empty queue", async () => {
    mockInvoke({
      get_auto_failover_enabled: false,
      get_failover_queue: () => {
        throw new Error("database locked");
      },
      get_available_providers_for_failover: [],
    });
    renderWithQuery(<FailoverQueueManager appType="claude" />);
    expect(await screen.findByText(/database locked/)).toBeInTheDocument();
    expect(screen.queryByText("proxy.failoverQueue.empty")).toBeNull();
  });

  it("removes a provider with the IPC argument names the backend expects", async () => {
    const api = mockInvoke({
      get_auto_failover_enabled: true,
      get_failover_queue: queue,
      get_available_providers_for_failover: [],
      remove_from_failover_queue: null,
    });
    renderWithQuery(<FailoverQueueManager appType="codex" />);

    await screen.findByText("Primary");
    fireEvent.click(
      screen.getAllByRole("button", { name: "common.delete" })[1],
    );

    await waitFor(() =>
      expect(api.callsTo("remove_from_failover_queue")).toHaveLength(1),
    );
    expect(api.callsTo("remove_from_failover_queue")[0].args).toEqual({
      appType: "codex",
      providerId: "p2",
    });
    await waitFor(() => expect(toastMock.success).toHaveBeenCalled());
  });

  it("toggles auto failover for this app only", async () => {
    const api = mockInvoke({
      get_auto_failover_enabled: false,
      get_failover_queue: queue,
      get_available_providers_for_failover: [],
      set_auto_failover_enabled: null,
    });
    renderWithQuery(<FailoverQueueManager appType="gemini" />);

    await screen.findByText("Primary");
    fireEvent.click(screen.getByRole("switch"));

    await waitFor(() =>
      expect(api.callsTo("set_auto_failover_enabled")).toHaveLength(1),
    );
    expect(api.callsTo("set_auto_failover_enabled")[0].args).toEqual({
      appType: "gemini",
      enabled: true,
    });
  });

  it("disables every control when the panel is disabled", async () => {
    mockInvoke({
      get_auto_failover_enabled: false,
      get_failover_queue: queue,
      get_available_providers_for_failover: [],
    });
    renderWithQuery(<FailoverQueueManager appType="claude" disabled />);

    await screen.findByText("Primary");
    expect(screen.getByRole("switch")).toBeDisabled();
    for (const button of screen.getAllByRole("button", {
      name: "common.delete",
    })) {
      expect(button).toBeDisabled();
    }
  });
});

describe("PromptPanel", () => {
  const prompts = {
    a: { id: "a", name: "Alpha", content: "A", enabled: true },
    b: { id: "b", name: "Beta", content: "B", enabled: false },
  };

  it("loads prompts when opened and summarises the enabled one", async () => {
    const api = mockInvoke({
      get_prompts: prompts,
      get_current_prompt_file_content: "A",
    });
    renderWithQuery(<PromptPanel open onOpenChange={vi.fn()} appId="claude" />);

    expect(await screen.findByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();
    expect(api.callsTo("get_prompts")[0].args).toEqual({ app: "claude" });
    const [alpha, beta] = screen.getAllByRole("switch");
    expect(alpha).toHaveAttribute("aria-checked", "true");
    expect(beta).toHaveAttribute("aria-checked", "false");
  });

  it("shows the empty state when there are no prompts", async () => {
    mockInvoke({ get_prompts: {}, get_current_prompt_file_content: null });
    renderWithQuery(<PromptPanel open onOpenChange={vi.fn()} appId="codex" />);
    expect(await screen.findByText("prompts.empty")).toBeInTheDocument();
  });

  it("enabling a prompt is exclusive and calls enable_prompt", async () => {
    let current = prompts;
    const api = mockInvoke({
      // Stateful backend: the post-toggle refetch sees the new state.
      get_prompts: () => current,
      enable_prompt: ({ id }: Record<string, unknown>) => {
        current = Object.fromEntries(
          Object.entries(current).map(([key, p]) => [
            key,
            { ...p, enabled: key === id },
          ]),
        ) as typeof prompts;
      },
    });
    renderWithQuery(<PromptPanel open onOpenChange={vi.fn()} appId="claude" />);

    await screen.findByText("Beta");
    fireEvent.click(screen.getAllByRole("switch")[1]);

    // Optimistic: Beta on, Alpha off.
    await waitFor(() => {
      const [alpha, beta] = screen.getAllByRole("switch");
      expect(beta).toHaveAttribute("aria-checked", "true");
      expect(alpha).toHaveAttribute("aria-checked", "false");
    });
    await waitFor(() => expect(api.callsTo("enable_prompt")).toHaveLength(1));
    expect(api.callsTo("enable_prompt")[0].args).toEqual({
      app: "claude",
      id: "b",
    });
  });

  it("deletes a prompt only after confirmation", async () => {
    const api = mockInvoke({
      get_prompts: prompts,
      get_current_prompt_file_content: "A",
      delete_prompt: null,
    });
    renderWithQuery(<PromptPanel open onOpenChange={vi.fn()} appId="claude" />);

    await screen.findByText("Beta");
    fireEvent.click(screen.getAllByTitle("common.delete")[1]);
    expect(api.callsTo("delete_prompt")).toHaveLength(0);

    fireEvent.click(
      await screen.findByRole("button", { name: "common.confirm" }),
    );
    await waitFor(() => expect(api.callsTo("delete_prompt")).toHaveLength(1));
    expect(api.callsTo("delete_prompt")[0].args).toEqual({
      app: "claude",
      id: "b",
    });
  });

  it("keeps in-progress edits when the prompt list refetches (FE-UI-51)", async () => {
    let version = 0;
    const api = mockInvoke({
      get_prompts: () => {
        version += 1;
        return {
          a: { ...prompts.a, description: `v${version}` },
          b: prompts.b,
        };
      },
      upsert_prompt: null,
    });
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <PromptPanel open onOpenChange={vi.fn()} appId="claude" />
      </QueryClientProvider>,
    );

    await screen.findByText("Alpha");
    fireEvent.click(screen.getAllByTitle("common.edit")[0]);
    const editor = await screen.findByTestId("editor");
    fireEvent.change(editor, { target: { value: "# A\n" } });

    // A refetch (e.g. `profile-applied`) delivers new object identities.
    await act(async () => {
      await client.invalidateQueries({ queryKey: ["prompts"] });
    });
    await waitFor(() => expect(version).toBeGreaterThan(1));
    expect(screen.getByTestId("editor")).toHaveValue("# A\n");

    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(api.callsTo("upsert_prompt")).toHaveLength(1));
    const saved = api.callsTo("upsert_prompt")[0].args.prompt as {
      content: string;
    };
    // Content is stored untrimmed.
    expect(saved.content).toBe("# A\n");
  });

  it("rolls back a failed toggle without an unhandled rejection and disables toggles while pending (FE-UI-52)", async () => {
    let rejectEnable: (error: Error) => void = () => undefined;
    mockInvoke({
      get_prompts: prompts,
      enable_prompt: () =>
        new Promise((_, reject) => {
          rejectEnable = reject;
        }),
    });
    const unhandled = vi.fn();
    window.addEventListener("unhandledrejection", unhandled);
    renderWithQuery(<PromptPanel open onOpenChange={vi.fn()} appId="claude" />);

    await screen.findByText("Beta");
    fireEvent.click(screen.getAllByRole("switch")[1]);

    await waitFor(() =>
      expect(screen.getAllByRole("switch")[1]).toHaveAttribute(
        "aria-checked",
        "true",
      ),
    );
    for (const toggle of screen.getAllByRole("switch")) {
      expect(toggle).toBeDisabled();
    }

    await act(async () => {
      rejectEnable(new Error("nope"));
    });

    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith("prompts.enableFailed"),
    );
    const [alpha, beta] = screen.getAllByRole("switch");
    expect(alpha).toHaveAttribute("aria-checked", "true");
    expect(beta).toHaveAttribute("aria-checked", "false");
    // The list never fell back to the first-load placeholder.
    expect(screen.queryByText("prompts.loading")).not.toBeInTheDocument();
    window.removeEventListener("unhandledrejection", unhandled);
    expect(unhandled).not.toHaveBeenCalled();
  });

  it("shows a load error toast when prompts cannot be read", async () => {
    mockInvoke({
      get_prompts: () => {
        throw new Error("boom");
      },
    });
    renderWithQuery(<PromptPanel open onOpenChange={vi.fn()} appId="claude" />);
    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith("prompts.loadFailed"),
    );
  });
});

describe("HermesMemoryPanel", () => {
  it("edits and saves the agent memory with the right kind", async () => {
    const api = mockInvoke({
      get_hermes_memory_limits: {
        memory: 20,
        user: 10,
        memoryEnabled: true,
        userEnabled: true,
      },
      get_hermes_memory: ({ kind }: Record<string, unknown>) =>
        kind === "memory" ? "hello" : "me",
      set_hermes_memory: null,
    });
    renderWithQuery(<HermesMemoryPanel />);

    const editor = await screen.findByTestId("editor");
    await waitFor(() => expect(editor).toHaveValue("hello"));

    fireEvent.change(editor, { target: { value: "hello world" } });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() =>
      expect(api.callsTo("set_hermes_memory")).toHaveLength(1),
    );
    expect(api.callsTo("set_hermes_memory")[0].args).toEqual({
      kind: "memory",
      content: "hello world",
    });
    await waitFor(() =>
      expect(toastMock.success).toHaveBeenCalledWith(
        "hermes.memory.saveSuccess",
      ),
    );
  });

  it("flags content over the configured limit", async () => {
    mockInvoke({
      get_hermes_memory_limits: {
        memory: 5,
        user: 10,
        memoryEnabled: true,
        userEnabled: true,
      },
      get_hermes_memory: "123456",
    });
    renderWithQuery(<HermesMemoryPanel />);

    await waitFor(() =>
      expect(screen.getByTestId("editor")).toHaveValue("123456"),
    );
    expect(screen.getByText(/hermes\.memory\.overLimit/)).toBeInTheDocument();
  });

  it("disables Save while the content is over the limit (FE-UI-50)", async () => {
    mockInvoke({
      get_hermes_memory_limits: {
        memory: 5,
        user: 10,
        memoryEnabled: true,
        userEnabled: true,
      },
      get_hermes_memory: "123456",
    });
    renderWithQuery(<HermesMemoryPanel />);
    await waitFor(() =>
      expect(screen.getByTestId("editor")).toHaveValue("123456"),
    );
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
  });

  it("keeps unsaved edits across Agent/User tab switches (FE-UI-50)", async () => {
    const user = userEvent.setup();
    mockInvoke({
      get_hermes_memory_limits: {
        memory: 50,
        user: 50,
        memoryEnabled: true,
        userEnabled: true,
      },
      get_hermes_memory: ({ kind }: Record<string, unknown>) =>
        kind === "memory" ? "agent" : "user",
    });
    renderWithQuery(<HermesMemoryPanel />);

    await waitFor(() =>
      expect(screen.getByTestId("editor")).toHaveValue("agent"),
    );
    fireEvent.change(screen.getByTestId("editor"), {
      target: { value: "agent edited" },
    });

    await user.click(
      screen.getByRole("tab", { name: /hermes.memory.userTab/ }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("editor")).toHaveValue("user"),
    );
    await user.click(
      screen.getByRole("tab", { name: /hermes.memory.agentTab/ }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("editor")).toHaveValue("agent edited"),
    );
    // The dirty tab is marked.
    expect(
      screen.getByRole("tab", { name: /hermes.memory.agentTab/ }),
    ).toHaveTextContent("•");
  });

  it("shows the disabled hint and toggles memory on", async () => {
    const api = mockInvoke({
      get_hermes_memory_limits: {
        memory: 20,
        user: 10,
        memoryEnabled: false,
        userEnabled: true,
      },
      get_hermes_memory: "",
      set_hermes_memory_enabled: null,
    });
    renderWithQuery(<HermesMemoryPanel />);

    expect(
      await screen.findByText("hermes.memory.disabledHint"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(api.callsTo("set_hermes_memory_enabled")).toHaveLength(1),
    );
    expect(api.callsTo("set_hermes_memory_enabled")[0].args).toEqual({
      kind: "memory",
      enabled: true,
    });
  });
});
