import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NavigationProvider, useNavigation } from "@/app/navigation";
import {
  CommandPalette,
  isPaletteShortcut,
} from "@/components/palette/CommandPalette";
import { getCurrentProviderId, resetProviderState } from "../msw/state";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

function ViewProbe() {
  const { view, params, activeApp } = useNavigation();
  return (
    <div data-testid="probe">
      {view}|{params?.tab ?? ""}|{activeApp}
    </div>
  );
}

const renderPalette = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <NavigationProvider>
        <ViewProbe />
        <CommandPalette />
      </NavigationProvider>
    </QueryClientProvider>,
  );

const openPalette = () =>
  fireEvent.keyDown(window, { key: "k", ctrlKey: true });

describe("CommandPalette (PRODUCT-19)", () => {
  beforeEach(() => {
    localStorage.clear();
    resetProviderState();
    // cmdk scrolls the selected item into view; jsdom lacks the API.
    Element.prototype.scrollIntoView = vi.fn();
  });

  it("recognises only Cmd/Ctrl+K", () => {
    const ev = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);
    expect(isPaletteShortcut(ev({ key: "k", metaKey: true }))).toBe(true);
    expect(isPaletteShortcut(ev({ key: "K", ctrlKey: true }))).toBe(true);
    expect(isPaletteShortcut(ev({ key: "k" }))).toBe(false);
    expect(
      isPaletteShortcut(ev({ key: "k", ctrlKey: true, shiftKey: true })),
    ).toBe(false);
  });

  it("switches the active app's provider from the keyboard", async () => {
    renderPalette();
    openPalette();
    const item = await screen.findByText("Claude Custom");
    fireEvent.click(item);
    await waitFor(() =>
      expect(getCurrentProviderId("claude")).toBe("claude-2"),
    );
    await waitFor(() =>
      expect(screen.queryByText("Claude Custom")).not.toBeInTheDocument(),
    );
  });

  it("opens a settings tab and jumps to another app", async () => {
    renderPalette();
    openPalette();
    fireEvent.click(await screen.findByText("usage.title"));
    await waitFor(() =>
      expect(screen.getByTestId("probe").textContent).toBe(
        "settings|usage|claude",
      ),
    );

    openPalette();
    const codex = await screen.findAllByText(/codex/i);
    fireEvent.click(codex[0]);
    await waitFor(() =>
      expect(screen.getByTestId("probe").textContent).toBe("providers||codex"),
    );
  });
});
