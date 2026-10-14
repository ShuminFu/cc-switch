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

const memory = vi.hoisted(() => ({
  data: { memory: "agent memory", user: "user profile" } as Record<
    string,
    string
  >,
}));

vi.mock("@/hooks/useHermes", () => ({
  useHermesMemory: (kind: string) => ({
    data: memory.data[kind],
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

  it("re-seeds a clean pane when the memory file changes on disk", () => {
    memory.data = { memory: "agent memory", user: "user profile" };
    const { rerender } = render(<HermesMemoryPanel />);

    memory.data = { memory: "agent memory v2", user: "user profile" };
    rerender(<HermesMemoryPanel />);
    expect(visibleEditor().value).toBe("agent memory v2");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("offers Reload / Keep mine when the file changes under unsaved edits", () => {
    memory.data = { memory: "agent memory", user: "user profile" };
    const { rerender } = render(<HermesMemoryPanel />);
    fireEvent.change(visibleEditor(), { target: { value: "mine" } });

    memory.data = { memory: "from disk", user: "user profile" };
    rerender(<HermesMemoryPanel />);

    expect(visibleEditor().value).toBe("mine");
    expect(screen.getByRole("alert")).toBeInTheDocument();

    fireEvent.click(screen.getByText("common.externalChangeReload"));
    expect(visibleEditor().value).toBe("from disk");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
