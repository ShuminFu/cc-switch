import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import PromptFormPanel from "@/components/prompts/PromptFormPanel";
import type { Prompt } from "@/lib/api";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

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

const makePrompt = (overrides: Partial<Prompt> = {}): Prompt => ({
  id: "p1",
  name: "Original",
  description: "desc",
  content: "body",
  enabled: false,
  createdAt: 1,
  updatedAt: 1,
  ...overrides,
});

describe("PromptFormPanel", () => {
  it("hydrates fields from initialData", () => {
    render(
      <PromptFormPanel
        appId="claude"
        editingId="p1"
        initialData={makePrompt()}
        onSave={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByLabelText("prompts.name")).toHaveValue("Original");
    expect(screen.getByTestId("md-editor")).toHaveValue("body");
  });

  it("does not clobber in-progress edits when the parent reloads prompts", () => {
    const onSave = vi.fn();
    const onClose = vi.fn();
    const { rerender } = render(
      <PromptFormPanel
        appId="claude"
        editingId="p1"
        initialData={makePrompt()}
        onSave={onSave}
        onClose={onClose}
      />,
    );

    fireEvent.change(screen.getByLabelText("prompts.name"), {
      target: { value: "Edited name" },
    });
    fireEvent.change(screen.getByTestId("md-editor"), {
      target: { value: "edited body" },
    });

    // A reload (e.g. profile-applied / toggle) hands down a fresh object.
    rerender(
      <PromptFormPanel
        appId="claude"
        editingId="p1"
        initialData={makePrompt({ enabled: true, updatedAt: 2 })}
        onSave={onSave}
        onClose={onClose}
      />,
    );

    expect(screen.getByLabelText("prompts.name")).toHaveValue("Edited name");
    expect(screen.getByTestId("md-editor")).toHaveValue("edited body");
    // Only metadata changed, so no external-change banner.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("offers Reload when the prompt content changes underneath edits", () => {
    const props = {
      appId: "claude" as const,
      editingId: "p1",
      onSave: vi.fn(),
      onClose: vi.fn(),
    };
    const { rerender } = render(
      <PromptFormPanel {...props} initialData={makePrompt()} />,
    );
    fireEvent.change(screen.getByTestId("md-editor"), {
      target: { value: "mine" },
    });

    rerender(
      <PromptFormPanel
        {...props}
        initialData={makePrompt({ content: "imported body" })}
      />,
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();
    fireEvent.click(screen.getByText("common.externalChangeReload"));
    expect(screen.getByTestId("md-editor")).toHaveValue("imported body");
  });

  it("closes immediately on ESC when there are no edits", () => {
    const onClose = vi.fn();
    render(
      <PromptFormPanel
        appId="claude"
        editingId="p1"
        initialData={makePrompt()}
        onSave={vi.fn()}
        onClose={onClose}
      />,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("asks before discarding unsaved edits on close", () => {
    const onClose = vi.fn();
    render(
      <PromptFormPanel
        appId="claude"
        editingId="p1"
        initialData={makePrompt()}
        onSave={vi.fn()}
        onClose={onClose}
      />,
    );
    fireEvent.change(screen.getByLabelText("prompts.name"), {
      target: { value: "Edited" },
    });

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByText("common.discardChangesTitle")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(
      screen.getByRole("button", { name: "common.discardChanges" }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
