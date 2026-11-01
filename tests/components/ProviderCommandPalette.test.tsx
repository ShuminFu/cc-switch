import { act, fireEvent, render, screen } from "@testing-library/react";
import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { Provider } from "@/types";
import {
  formatShortcut,
  matchesShortcut,
  useRegisteredShortcuts,
  useShortcut,
} from "@/lib/shortcuts";

// cmdk scrolls the selected item into view; jsdom has no layout.
Element.prototype.scrollIntoView ??= () => undefined;

const switchProviderMock = vi.fn();
const providersByApp: Record<
  string,
  { providers: Record<string, Provider>; currentProviderId: string }
> = {
  claude: {
    providers: {
      c1: { id: "c1", name: "Claude Relay", settingsConfig: {} },
      c2: { id: "c2", name: "Claude Backup", settingsConfig: {} },
    },
    currentProviderId: "c1",
  },
  codex: {
    providers: { x1: { id: "x1", name: "Codex Relay", settingsConfig: {} } },
    currentProviderId: "",
  },
};

vi.mock("@/lib/query", () => ({
  useProvidersQuery: (appId: string) => ({ data: providersByApp[appId] }),
  useSettingsQuery: () => ({
    data: { visibleApps: { claude: true, codex: true } },
  }),
}));
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({ isRunning: false, takeoverStatus: {} }),
}));
vi.mock("@/hooks/useProviderActions", () => ({
  useProviderActions: (appId: string) => ({
    switchProvider: (provider: Provider) => switchProviderMock(appId, provider),
  }),
}));

import { ProviderCommandPalette } from "@/components/providers/ProviderCommandPalette";

const key = (init: KeyboardEventInit) =>
  new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });

describe("shortcut registry (FE-ARCH-16)", () => {
  it("matches mod combinations on Cmd or Ctrl", () => {
    expect(matchesShortcut(key({ key: "k", metaKey: true }), "mod+k")).toBe(
      true,
    );
    expect(matchesShortcut(key({ key: "K", ctrlKey: true }), "mod+k")).toBe(
      true,
    );
    expect(matchesShortcut(key({ key: "k" }), "mod+k")).toBe(false);
    expect(formatShortcut("mod+k", true)).toBe("⌘K");
    expect(formatShortcut("mod+,", false)).toBe("Ctrl+,");
  });

  it("dispatches registered shortcuts and lists them", () => {
    const handler = vi.fn();
    const { unmount } = renderHook(() => {
      useShortcut({ id: "test.x", keys: "mod+j", label: "Test" }, handler);
      return useRegisteredShortcuts();
    });
    act(() => {
      window.dispatchEvent(key({ key: "j", ctrlKey: true }));
    });
    expect(handler).toHaveBeenCalledTimes(1);
    unmount();
    act(() => {
      window.dispatchEvent(key({ key: "j", ctrlKey: true }));
    });
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it("skips plain-key shortcuts while typing in a text field", () => {
    const handler = vi.fn();
    renderHook(() =>
      useShortcut({ id: "test.slash", keys: "/", label: "Slash" }, handler),
    );
    const input = document.createElement("input");
    document.body.appendChild(input);
    act(() => {
      input.dispatchEvent(key({ key: "/" }));
    });
    expect(handler).not.toHaveBeenCalled();
    input.remove();
  });
});

describe("ProviderCommandPalette (FE-PROV-35)", () => {
  beforeEach(() => switchProviderMock.mockReset());

  it("opens with Cmd+K and switches the provider of the right app", async () => {
    const navigation = {
      activeApp: "claude" as const,
      setActiveApp: vi.fn(),
      openPage: vi.fn(),
    };
    render(<ProviderCommandPalette navigation={navigation} />);
    expect(screen.queryByText("Codex Relay")).toBeNull();

    act(() => {
      window.dispatchEvent(key({ key: "k", metaKey: true }));
    });
    fireEvent.click(await screen.findByText("Codex Relay"));
    expect(switchProviderMock).toHaveBeenCalledWith(
      "codex",
      providersByApp.codex.providers.x1,
    );
  });

  it("does not re-switch to the current provider and can open a page", async () => {
    const navigation = {
      activeApp: "claude" as const,
      setActiveApp: vi.fn(),
      openPage: vi.fn(),
    };
    render(<ProviderCommandPalette navigation={navigation} />);
    act(() => {
      window.dispatchEvent(key({ key: "k", ctrlKey: true }));
    });
    fireEvent.click(await screen.findByText("Claude Relay"));
    expect(switchProviderMock).not.toHaveBeenCalled();

    act(() => {
      window.dispatchEvent(key({ key: "k", ctrlKey: true }));
    });
    fireEvent.click(await screen.findByText("Settings"));
    expect(navigation.openPage).toHaveBeenCalledWith("settings");
  });
});
