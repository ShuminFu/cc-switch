import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import WorkspaceFileEditor from "@/components/workspace/WorkspaceFileEditor";
import { workspaceApi } from "@/lib/api/workspace";

// `t` must be referentially stable (as in react-i18next): the read effect
// depends on it.
vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

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

const disk: Record<string, string> = {};

beforeEach(() => {
  vi.restoreAllMocks();
  disk["AGENTS.md"] = "v1";
  vi.spyOn(workspaceApi, "readFile").mockImplementation(async (name) =>
    name in disk ? disk[name] : Promise.reject(new Error("unreadable")),
  );
  vi.spyOn(workspaceApi, "writeFile").mockImplementation(
    async (name, content) => {
      disk[name] = content;
    },
  );
});

const renderEditor = (onClose = vi.fn(), filename = "AGENTS.md") =>
  render(<WorkspaceFileEditor filename={filename} isOpen onClose={onClose} />);

const focusWindow = async () => {
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
};

describe("WorkspaceFileEditor", () => {
  it("silently picks up on-disk changes on focus when there are no edits", async () => {
    renderEditor();
    const editor = await screen.findByTestId("md-editor");
    expect(editor).toHaveValue("v1");

    disk["AGENTS.md"] = "v2";
    await focusWindow();
    await waitFor(() => expect(editor).toHaveValue("v2"));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows Reload / Keep mine when the file changes under unsaved edits", async () => {
    renderEditor();
    const editor = await screen.findByTestId("md-editor");
    fireEvent.change(editor, { target: { value: "mine" } });

    disk["AGENTS.md"] = "agent wrote this";
    await focusWindow();

    await screen.findByRole("alert");
    expect(editor).toHaveValue("mine");

    fireEvent.click(screen.getByText("common.externalChangeReload"));
    expect(editor).toHaveValue("agent wrote this");
  });

  it("asks before discarding unsaved edits on close", async () => {
    const onClose = vi.fn();
    renderEditor(onClose);
    const editor = await screen.findByTestId("md-editor");
    fireEvent.change(editor, { target: { value: "mine" } });

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(
      await screen.findByRole("button", { name: "common.discardChanges" }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("saves and stays clean afterwards", async () => {
    const onClose = vi.fn();
    renderEditor(onClose);
    const editor = await screen.findByTestId("md-editor");
    fireEvent.change(editor, { target: { value: "saved text" } });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(disk["AGENTS.md"]).toBe("saved text"));
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("disables Save when the newly opened file could not be read", async () => {
    const { rerender } = renderEditor();
    await screen.findByTestId("md-editor");

    rerender(
      <WorkspaceFileEditor filename="MISSING.md" isOpen onClose={vi.fn()} />,
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "common.save" }),
      ).toBeDisabled(),
    );
  });
});
