import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import HermesMemoryPanel from "@/components/hermes/HermesMemoryPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/hooks/useDarkMode", () => ({ useDarkMode: () => false }));

vi.mock("@/components/MarkdownEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (v: string) => void;
  }) => (
    <textarea
      data-testid="md-editor"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  ),
}));

vi.mock("@/hooks/useHermes", () => ({
  useHermesMemory: (kind: string) => ({
    data: kind === "memory" ? "agent memory" : "user profile",
    isLoading: false,
  }),
  useHermesMemoryLimits: () => ({ data: undefined }),
  useOpenHermesWebUI: () => vi.fn(),
  useSaveHermesMemory: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useToggleHermesMemoryEnabled: () => ({ mutate: vi.fn(), isPending: false }),
}));

const visibleEditor = () =>
  screen
    .getAllByTestId("md-editor")
    .find((el) => !el.closest("[data-state=inactive]")) as HTMLTextAreaElement;

describe("HermesMemoryPanel", () => {
  it("keeps unsaved edits when switching between tabs", () => {
    render(<HermesMemoryPanel />);

    expect(visibleEditor().value).toBe("agent memory");
    fireEvent.change(visibleEditor(), {
      target: { value: "agent memory (edited)" },
    });

    fireEvent.mouseDown(
      screen.getByRole("tab", { name: "hermes.memory.userTab" }),
    );
    expect(visibleEditor().value).toBe("user profile");

    fireEvent.mouseDown(
      screen.getByRole("tab", { name: "hermes.memory.agentTab" }),
    );
    expect(visibleEditor().value).toBe("agent memory (edited)");
  });
});
