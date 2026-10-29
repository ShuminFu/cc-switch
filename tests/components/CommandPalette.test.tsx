import { useState } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Provider, VisibleApps } from "@/types";
import { ALL_APP_IDS } from "@/entities/app/registry";
import { providerKeys } from "@/entities/provider/keys";
import {
  CommandPalette,
  useCommandPaletteShortcut,
  type CommandPaletteProps,
} from "@/components/app-shell/CommandPalette";

const provider = (id: string, name: string): Provider => ({
  id,
  name,
  settingsConfig: {},
});

const allVisible = Object.fromEntries(
  ALL_APP_IDS.map((id) => [id, true]),
) as unknown as VisibleApps;

const seed = (client: QueryClient) => {
  client.setQueryData(providerKeys.list("claude"), {
    providers: {
      "c-1": provider("c-1", "Anthropic Official"),
      "3f2a9c1e-uuid": provider("3f2a9c1e-uuid", "Relay Mirror"),
    },
    currentProviderId: "c-1",
  });
  client.setQueryData(providerKeys.list("codex"), {
    providers: { "x-1": provider("x-1", "OpenAI Official") },
    currentProviderId: "",
  });
  client.setQueryData(providerKeys.list("opencode"), {
    providers: { "o-1": provider("o-1", "OpenCode Zen") },
    currentProviderId: "",
  });
};

function Harness(
  props: Partial<CommandPaletteProps> & { initialOpen?: boolean },
) {
  const { initialOpen = false, ...rest } = props;
  const [open, setOpen] = useState(initialOpen);
  useCommandPaletteShortcut(open, setOpen);
  return (
    <CommandPalette
      open={open}
      onOpenChange={setOpen}
      visibleApps={allVisible}
      activeApp="claude"
      onSwitchProvider={vi.fn()}
      onSelectApp={vi.fn()}
      onOpenSettings={vi.fn()}
      onAddProvider={vi.fn()}
      isTakeoverActive={false}
      {...rest}
    />
  );
}

const renderPalette = (
  props: Partial<CommandPaletteProps> & { initialOpen?: boolean } = {},
) => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  seed(client);
  return render(
    <QueryClientProvider client={client}>
      <Harness {...props} />
    </QueryClientProvider>,
  );
};

const input = () => screen.getByPlaceholderText("commandPalette.placeholder");

describe("CommandPalette", () => {
  it("toggles with Cmd/Ctrl-K", async () => {
    renderPalette();
    expect(
      screen.queryByPlaceholderText("commandPalette.placeholder"),
    ).not.toBeInTheDocument();

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(await screen.findByText("OpenAI Official")).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "K", ctrlKey: true });
    await waitFor(() =>
      expect(
        screen.queryByPlaceholderText("commandPalette.placeholder"),
      ).not.toBeInTheDocument(),
    );

    // 带 Shift 的组合键不触发
    fireEvent.keyDown(window, { key: "k", ctrlKey: true, shiftKey: true });
    expect(
      screen.queryByPlaceholderText("commandPalette.placeholder"),
    ).not.toBeInTheDocument();
  });

  it("does not open over another dialog", () => {
    render(<div role="dialog">other</div>);
    renderPalette();
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(
      screen.queryByPlaceholderText("commandPalette.placeholder"),
    ).not.toBeInTheDocument();
  });

  it("lists providers of visible apps only", async () => {
    renderPalette({
      initialOpen: true,
      visibleApps: { ...allVisible, codex: false },
    });
    expect(await screen.findByText("Relay Mirror")).toBeInTheDocument();
    expect(screen.getByText("OpenCode Zen")).toBeInTheDocument();
    expect(screen.queryByText("OpenAI Official")).not.toBeInTheDocument();
  });

  it("filters by provider and app name, not by provider id", async () => {
    renderPalette({ initialOpen: true });
    await screen.findByText("Relay Mirror");

    fireEvent.change(input(), { target: { value: "3f2a9c1e" } });
    await waitFor(() =>
      expect(screen.queryByText("Relay Mirror")).not.toBeInTheDocument(),
    );
    expect(screen.getByText("commandPalette.empty")).toBeInTheDocument();

    fireEvent.change(input(), { target: { value: "codex" } });
    expect(await screen.findByText("OpenAI Official")).toBeInTheDocument();
    expect(screen.queryByText("Anthropic Official")).not.toBeInTheDocument();
  });

  it("switches a non-current provider on Enter and closes", async () => {
    const onSwitchProvider = vi.fn();
    renderPalette({ initialOpen: true, onSwitchProvider });
    await screen.findByText("Relay Mirror");

    fireEvent.change(input(), { target: { value: "relay" } });
    await waitFor(() =>
      expect(screen.queryByText("OpenAI Official")).not.toBeInTheDocument(),
    );
    fireEvent.keyDown(input(), { key: "Enter" });

    expect(onSwitchProvider).toHaveBeenCalledWith(
      "claude",
      expect.objectContaining({ id: "3f2a9c1e-uuid" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByPlaceholderText("commandPalette.placeholder"),
      ).not.toBeInTheDocument(),
    );
  });

  it("only navigates for the current provider and additive apps", async () => {
    const onSwitchProvider = vi.fn();
    const onSelectApp = vi.fn();
    renderPalette({ initialOpen: true, onSwitchProvider, onSelectApp });

    fireEvent.click(await screen.findByText("Anthropic Official"));
    expect(onSelectApp).toHaveBeenLastCalledWith("claude");

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByText("OpenCode Zen"));
    expect(onSelectApp).toHaveBeenLastCalledWith("opencode");

    expect(onSwitchProvider).not.toHaveBeenCalled();
  });

  it("runs common actions and hides takeover when unavailable", async () => {
    const onOpenSettings = vi.fn();
    const { unmount } = renderPalette({ initialOpen: true, onOpenSettings });
    await screen.findByText("Relay Mirror");
    expect(
      screen.queryByText("commandPalette.enableTakeover"),
    ).not.toBeInTheDocument();

    fireEvent.change(input(), { target: { value: "settings" } });
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
    unmount();

    const onToggleTakeover = vi.fn();
    renderPalette({
      initialOpen: true,
      onToggleTakeover,
      isTakeoverActive: true,
    });
    fireEvent.click(await screen.findByText("commandPalette.disableTakeover"));
    expect(onToggleTakeover).toHaveBeenCalledTimes(1);
  });
});
