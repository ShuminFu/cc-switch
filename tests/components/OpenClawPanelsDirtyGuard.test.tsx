import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import EnvPanel from "@/components/openclaw/EnvPanel";
import ToolsPanel from "@/components/openclaw/ToolsPanel";
import AgentsDefaultsPanel from "@/components/openclaw/AgentsDefaultsPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (v: string) => void;
  }) => (
    <textarea
      data-testid="json-editor"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  ),
}));

vi.mock("@/components/openclaw/hooks/useOpenClawModelOptions", () => ({
  useOpenClawModelOptions: () => ({ options: [], isLoading: false }),
}));

const state = vi.hoisted(() => ({
  env: {} as Record<string, unknown>,
  tools: {} as Record<string, unknown>,
  agents: {} as Record<string, unknown> | null,
  save: vi.fn(),
}));

vi.mock("@/hooks/useOpenClaw", () => ({
  useOpenClawEnv: () => ({ data: state.env, isLoading: false }),
  useOpenClawTools: () => ({ data: state.tools, isLoading: false }),
  useOpenClawAgentsDefaults: () => ({ data: state.agents, isLoading: false }),
  useSaveOpenClawEnv: () => ({ mutateAsync: state.save, isPending: false }),
  useSaveOpenClawTools: () => ({ mutateAsync: state.save, isPending: false }),
  useSaveOpenClawAgentsDefaults: () => ({
    mutateAsync: state.save,
    isPending: false,
  }),
}));

const pretty = (v: unknown) => JSON.stringify(v, null, 2);

beforeEach(() => {
  state.save.mockReset();
  state.save.mockResolvedValue(undefined);
});

describe("OpenClaw EnvPanel", () => {
  it("re-seeds from refetched data while there are no edits", () => {
    state.env = { vars: { A: "1" } };
    const { rerender } = render(<EnvPanel />);
    expect(screen.getByTestId("json-editor")).toHaveValue(
      pretty({ vars: { A: "1" } }),
    );

    state.env = { vars: { A: "2" } };
    rerender(<EnvPanel />);
    expect(screen.getByTestId("json-editor")).toHaveValue(
      pretty({ vars: { A: "2" } }),
    );
  });

  it("keeps unsaved edits when the query refetches, re-seeds after save", async () => {
    state.env = { vars: { A: "1" } };
    const { rerender } = render(<EnvPanel />);
    const editor = screen.getByTestId("json-editor");

    fireEvent.change(editor, {
      target: { value: '{ "vars": { "A": "mine" } }' },
    });

    state.env = { vars: { A: "external" } };
    rerender(<EnvPanel />);
    expect(editor).toHaveValue('{ "vars": { "A": "mine" } }');

    fireEvent.click(screen.getByRole("button", { name: /common.save/ }));
    await waitFor(() =>
      expect(state.save).toHaveBeenCalledWith({ vars: { A: "mine" } }),
    );

    // Post-save refetch is applied again.
    state.env = { vars: { A: "mine" }, extra: true };
    rerender(<EnvPanel />);
    await waitFor(() =>
      expect(editor).toHaveValue(pretty({ vars: { A: "mine" }, extra: true })),
    );
  });
});

describe("OpenClaw ToolsPanel", () => {
  it("keeps unsaved allow-list edits when the query refetches", () => {
    state.tools = { allow: ["read"] };
    const { rerender } = render(<ToolsPanel />);

    const input = screen.getByDisplayValue("read");
    fireEvent.change(input, { target: { value: "read,write" } });

    state.tools = { allow: ["external"] };
    rerender(<ToolsPanel />);

    expect(screen.getByDisplayValue("read,write")).toBeInTheDocument();
    expect(screen.queryByDisplayValue("external")).not.toBeInTheDocument();
  });

  it("re-seeds from refetched data while there are no edits", () => {
    state.tools = { allow: ["read"] };
    const { rerender } = render(<ToolsPanel />);

    state.tools = { allow: ["external"] };
    rerender(<ToolsPanel />);
    expect(screen.getByDisplayValue("external")).toBeInTheDocument();
  });
});

describe("OpenClaw AgentsDefaultsPanel", () => {
  it("keeps unsaved runtime edits when the query refetches", () => {
    state.agents = { workspace: "~/a" };
    const { rerender } = render(<AgentsDefaultsPanel />);

    fireEvent.change(screen.getByPlaceholderText("~/projects"), {
      target: { value: "~/mine" },
    });

    state.agents = { workspace: "~/external" };
    rerender(<AgentsDefaultsPanel />);

    expect(screen.getByPlaceholderText("~/projects")).toHaveValue("~/mine");
  });

  it("re-seeds from refetched data while there are no edits", () => {
    state.agents = { workspace: "~/a" };
    const { rerender } = render(<AgentsDefaultsPanel />);

    state.agents = { workspace: "~/external" };
    rerender(<AgentsDefaultsPanel />);
    expect(screen.getByPlaceholderText("~/projects")).toHaveValue("~/external");
  });
});
