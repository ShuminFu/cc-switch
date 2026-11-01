import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsSearch } from "@/components/settings/SettingsSearch";
import { WindowSettings } from "@/components/settings/WindowSettings";
import { focusSettingByTitle } from "@/components/settings/focusSetting";
import { KeyboardShortcutsDialog } from "@/components/common/KeyboardShortcutsDialog";
import { useShortcut } from "@/lib/shortcuts";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

describe("SettingsSearch (FE-UI-38)", () => {
  it("lists matches and selects the highlighted one with Enter", () => {
    const onSelect = vi.fn();
    render(<SettingsSearch onSelect={onSelect} />);
    const box = screen.getByRole("combobox", { name: "Search settings" });

    fireEvent.change(box, { target: { value: "onboarding" } });
    const options = screen.getAllByRole("option");
    expect(options[0]).toHaveTextContent("Skip Claude Code onboarding");
    expect(options[0]).toHaveTextContent("General");

    fireEvent.keyDown(box, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith(
      expect.objectContaining({ id: "skipClaudeOnboarding", tab: "general" }),
    );
    expect(box).toHaveValue("");
  });

  it("says when nothing matches", () => {
    render(<SettingsSearch onSelect={vi.fn()} />);
    fireEvent.change(screen.getByRole("combobox"), {
      target: { value: "zzzz-nothing" },
    });
    expect(screen.getByText("No matching settings")).toBeInTheDocument();
  });

  it("Cmd/Ctrl+F focuses the search box", () => {
    render(<SettingsSearch onSelect={vi.fn()} />);
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    expect(screen.getByRole("combobox")).toHaveFocus();
  });
});

describe("focusSettingByTitle", () => {
  afterEach(() => vi.useRealTimers());

  it("focuses the toggle whose label is the setting title", () => {
    vi.useFakeTimers();
    const { container } = render(
      <WindowSettings
        settings={{ minimizeToTrayOnClose: false } as never}
        onChange={vi.fn()}
      />,
    );
    const focused = focusSettingByTitle(
      container,
      "settings.skipClaudeOnboarding",
    );
    expect(focused).toHaveAttribute("role", "switch");
    expect(focused).toHaveFocus();
    expect(focused?.closest("label")).toHaveClass("ring-2");
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(focused?.closest("label")).not.toHaveClass("ring-2");
  });

  it("returns null when the title is not on the page", () => {
    const { container } = render(<div>nothing</div>);
    expect(focusSettingByTitle(container, "Missing")).toBeNull();
  });
});

function ExtraShortcut() {
  useShortcut(
    { id: "test.extra", keys: "mod+k", label: "Open the command palette" },
    () => {},
  );
  return null;
}

describe("KeyboardShortcutsDialog", () => {
  it("opens with ? and lists the registered shortcuts", () => {
    render(
      <>
        <ExtraShortcut />
        <KeyboardShortcutsDialog />
      </>,
    );
    expect(screen.queryByText("Keyboard shortcuts")).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "?", shiftKey: true });
    expect(screen.getByText("Keyboard shortcuts")).toBeInTheDocument();
    expect(screen.getByText("Open the command palette")).toBeInTheDocument();
    expect(screen.getByText("Show keyboard shortcuts")).toBeInTheDocument();
  });
});
