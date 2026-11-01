import type { ReactElement } from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
} from "@testing-library/react";
import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import EnvPanel from "@/components/openclaw/EnvPanel";
import AgentsDefaultsPanel from "@/components/openclaw/AgentsDefaultsPanel";
import { queryKeys } from "@/lib/query/keys";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
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
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));
vi.mock("@/components/openclaw/hooks/useOpenClawModelOptions", () => ({
  useOpenClawModelOptions: () => ({
    options: [
      { value: "p/a", label: "P / A" },
      { value: "p/b", label: "P / B" },
    ],
    isLoading: false,
  }),
}));

const renderWith = (client: QueryClient, ui: ReactElement) =>
  render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>);

describe("EnvPanel (FE-UI-34)", () => {
  it("keeps unsaved edits when the env changes on disk and offers a reload", async () => {
    let env: Record<string, unknown> = { A: "1" };
    mockInvoke({ get_openclaw_env: () => env });
    const client = createTestQueryClient();
    renderWith(client, <EnvPanel />);

    const editor = await screen.findByTestId("json-editor");
    await waitFor(() => expect(editor).toHaveValue('{\n  "A": "1"\n}'));
    fireEvent.change(editor, { target: { value: '{"A":"mine"}' } });

    env = { A: "2" };
    await act(async () => {
      await client.invalidateQueries({ queryKey: queryKeys.openclaw.env });
    });

    expect(await screen.findByText(/changed on disk/)).toBeInTheDocument();
    expect(screen.getByTestId("json-editor")).toHaveValue('{"A":"mine"}');

    fireEvent.click(screen.getByRole("button", { name: /reload/i }));
    expect(screen.getByTestId("json-editor")).toHaveValue('{\n  "A": "2"\n}');
  });

  it("adopts refetched data silently while the buffer is clean", async () => {
    let env: Record<string, unknown> = { A: "1" };
    mockInvoke({ get_openclaw_env: () => env });
    const client = createTestQueryClient();
    renderWith(client, <EnvPanel />);
    const editor = await screen.findByTestId("json-editor");
    await waitFor(() => expect(editor).toHaveValue('{\n  "A": "1"\n}'));

    env = { B: "2" };
    await act(async () => {
      await client.invalidateQueries({ queryKey: queryKeys.openclaw.env });
    });
    await waitFor(() =>
      expect(screen.getByTestId("json-editor")).toHaveValue('{\n  "B": "2"\n}'),
    );
    expect(screen.queryByText(/changed on disk/)).not.toBeInTheDocument();
  });
});

describe("AgentsDefaultsPanel (FE-UI-36)", () => {
  it("refuses to save fallbacks without a primary model", async () => {
    const api = mockInvoke({
      get_openclaw_agents_defaults: { model: { fallbacks: ["p/b"] } },
      set_openclaw_agents_defaults: null,
    });
    renderWith(createTestQueryClient(), <AgentsDefaultsPanel />);

    fireEvent.click(await screen.findByRole("button", { name: /Save/ }));
    expect(
      await screen.findByText(/Choose a primary model/),
    ).toBeInTheDocument();
    expect(api.callsTo("set_openclaw_agents_defaults")).toHaveLength(0);
  });

  it("rejects non-integer numeric fields instead of dropping them", async () => {
    const api = mockInvoke({
      get_openclaw_agents_defaults: { model: { primary: "p/a" } },
      set_openclaw_agents_defaults: null,
    });
    renderWith(createTestQueryClient(), <AgentsDefaultsPanel />);

    const ctx = await screen.findByLabelText("Context Tokens");
    fireEvent.change(ctx, { target: { value: "-5" } });
    fireEvent.click(screen.getByRole("button", { name: /Save/ }));
    expect(await screen.findByText(/whole number/)).toBeInTheDocument();
    expect(api.callsTo("set_openclaw_agents_defaults")).toHaveLength(0);

    fireEvent.change(ctx, { target: { value: "200000" } });
    fireEvent.click(screen.getByRole("button", { name: /Save/ }));
    await waitFor(() =>
      expect(api.callsTo("set_openclaw_agents_defaults")).toHaveLength(1),
    );
    expect(api.callsTo("set_openclaw_agents_defaults")[0].args).toEqual({
      defaults: { model: { primary: "p/a" }, contextTokens: 200000 },
    });
  });
});
