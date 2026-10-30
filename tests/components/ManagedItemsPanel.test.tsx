import type { ReactElement } from "react";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ManagedItemsPanel } from "@/components/common/ManagedItemsPanel";
import { useConfirm } from "@/components/common/useConfirm";
import UnifiedMcpPanel from "@/components/mcp/UnifiedMcpPanel";
import UnifiedSkillsPanel from "@/components/skills/UnifiedSkillsPanel";
import PromptPanel from "@/components/prompts/PromptPanel";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({
  error: vi.fn(),
  success: vi.fn(),
  info: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: toastMock }));

const stableT = vi.hoisted(
  () => (key: string, options?: unknown) =>
    (options as { defaultValue?: string } | undefined)?.defaultValue ?? key,
);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: stableT, i18n: { language: "en" } }),
}));

vi.mock("@/components/MarkdownEditor", () => ({
  default: ({ value }: { value: string }) => (
    <textarea data-testid="editor" readOnly value={value} />
  ),
}));

// The MCP form pulls in CodeMirror; the generic panel only needs to open it.
vi.mock("@/components/mcp/McpFormModal", () => ({
  default: ({ editingId }: { editingId?: string }) => (
    <div data-testid="mcp-form">{editingId}</div>
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

const appsOff = {
  claude: false,
  codex: false,
  gemini: false,
  grokbuild: false,
  opencode: false,
  openclaw: false,
  hermes: false,
};

describe("ManagedItemsPanel (generic shell)", () => {
  type Item = { id: string; apps: Record<string, boolean> };
  const items: Item[] = [
    { id: "one", apps: { ...appsOff, claude: true } },
    { id: "two", apps: { ...appsOff, claude: true, codex: true } },
  ];

  function Harness({ onDelete }: { onDelete: (id: string) => void }) {
    const confirm = useConfirm();
    return (
      <ManagedItemsPanel
        items={items}
        isLoading={false}
        getId={(item) => item.id}
        totalLabel="2 items"
        appIds={["claude", "codex"]}
        getApps={(item) => item.apps}
        onToggleApp={() => undefined}
        renderPrimary={(item) => <span>{item.id}</span>}
        getActions={(item) => [
          {
            key: "delete",
            label: `delete ${item.id}`,
            icon: <span>x</span>,
            onClick: () =>
              confirm.confirm({
                title: "Delete?",
                message: item.id,
                onConfirm: () => onDelete(item.id),
              }),
          },
        ]}
        loadingLabel="loading"
        empty={{ icon: null, title: "empty", description: "" }}
        confirm={confirm}
      />
    );
  }

  it("derives per-app counts and runs actions through one confirm dialog", async () => {
    const onDelete = vi.fn();
    render(<Harness onDelete={onDelete} />);

    // Count bar: claude 2, codex 1.
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText("1")).toBeInTheDocument();

    // Action buttons have accessible names (keyboard reachable, FE-UI-11).
    fireEvent.click(screen.getByRole("button", { name: "delete two" }));
    expect(onDelete).not.toHaveBeenCalled();
    fireEvent.click(
      await screen.findByRole("button", { name: "common.confirm" }),
    );
    await waitFor(() => expect(onDelete).toHaveBeenCalledWith("two"));
    await waitFor(() =>
      expect(screen.queryByText("Delete?")).not.toBeInTheDocument(),
    );
  });

  it("app toggles expose their state to assistive tech", () => {
    render(<Harness onDelete={vi.fn()} />);
    const toggles = screen.getAllByRole("button", { pressed: true });
    // one: claude; two: claude + codex
    expect(toggles).toHaveLength(3);
  });
});

describe("ManagedItemsPanel adapters", () => {
  it("MCP: toggles an app, opens the editor and deletes after confirm", async () => {
    const api = mockInvoke({
      get_mcp_servers: {
        fs: {
          id: "fs",
          name: "Filesystem",
          server: { type: "stdio", command: "npx" },
          apps: { ...appsOff, claude: true },
        },
      },
      toggle_mcp_app: null,
      delete_mcp_server: true,
    });
    renderWithQuery(<UnifiedMcpPanel onOpenChange={vi.fn()} />);

    await screen.findByText("Filesystem");
    fireEvent.click(screen.getAllByRole("button", { name: "Codex" })[0]);
    await waitFor(() => expect(api.callsTo("toggle_mcp_app")).toHaveLength(1));
    expect(api.callsTo("toggle_mcp_app")[0].args).toEqual({
      serverId: "fs",
      app: "codex",
      enabled: true,
    });

    fireEvent.click(screen.getByRole("button", { name: "common.edit" }));
    expect(await screen.findByTestId("mcp-form")).toHaveTextContent("fs");

    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "common.confirm" }),
    );
    await waitFor(() =>
      expect(api.callsTo("delete_mcp_server")).toHaveLength(1),
    );
    expect(api.callsTo("delete_mcp_server")[0].args).toEqual({ id: "fs" });
  });

  it("Skills: toggles an app and uninstalls after confirm", async () => {
    const api = mockInvoke({
      get_installed_skills: [
        {
          id: "s1",
          name: "Writer",
          directory: "writer",
          apps: { ...appsOff, claude: true },
          installedAt: 0,
          updatedAt: 0,
        },
      ],
      get_skill_backups: [],
      scan_unmanaged_skills: [],
      toggle_skill_app: true,
      uninstall_skill_unified: {},
    });
    renderWithQuery(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} currentApp="claude" />,
    );

    await screen.findByText("Writer");
    fireEvent.click(screen.getAllByRole("button", { name: "Claude" })[0]);
    await waitFor(() =>
      expect(api.callsTo("toggle_skill_app")).toHaveLength(1),
    );
    expect(api.callsTo("toggle_skill_app")[0].args).toEqual({
      id: "s1",
      app: "claude",
      enabled: false,
    });

    fireEvent.click(screen.getByRole("button", { name: "skills.uninstall" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "common.confirm" }),
    );
    await waitFor(() =>
      expect(api.callsTo("uninstall_skill_unified")).toHaveLength(1),
    );
  });

  it("Prompts: toggles, opens the editor and deletes after confirm", async () => {
    let prompts = {
      a: { id: "a", name: "Alpha", content: "A", enabled: false },
    };
    const api = mockInvoke({
      get_prompts: () => prompts,
      enable_prompt: () => {
        prompts = { a: { ...prompts.a, enabled: true } };
      },
      delete_prompt: null,
    });
    renderWithQuery(<PromptPanel open onOpenChange={vi.fn()} appId="claude" />);

    await screen.findByText("Alpha");
    fireEvent.click(screen.getByRole("switch", { name: "Alpha" }));
    await waitFor(() => expect(api.callsTo("enable_prompt")).toHaveLength(1));

    fireEvent.click(screen.getByRole("button", { name: "common.edit" }));
    expect(await screen.findByTestId("editor")).toHaveValue("A");
  });

  it("Prompts: delete goes through the shared confirm dialog", async () => {
    const api = mockInvoke({
      get_prompts: {
        a: { id: "a", name: "Alpha", content: "A", enabled: false },
      },
      delete_prompt: null,
    });
    renderWithQuery(<PromptPanel open onOpenChange={vi.fn()} appId="claude" />);

    await screen.findByText("Alpha");
    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    expect(api.callsTo("delete_prompt")).toHaveLength(0);
    fireEvent.click(
      await screen.findByRole("button", { name: "common.confirm" }),
    );
    await waitFor(() => expect(api.callsTo("delete_prompt")).toHaveLength(1));
  });
});
