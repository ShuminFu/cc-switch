import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import WorkspaceFileEditor from "@/components/workspace/WorkspaceFileEditor";
import DailyMemoryPanel from "@/components/workspace/DailyMemoryPanel";

const api = vi.hoisted(() => ({
  readFile: vi.fn(),
  writeFile: vi.fn(),
  listDailyMemoryFiles: vi.fn(),
  readDailyMemoryFile: vi.fn(),
  writeDailyMemoryFile: vi.fn(),
  deleteDailyMemoryFile: vi.fn(),
  searchDailyMemoryFiles: vi.fn(),
}));

vi.mock("@/lib/api/workspace", () => ({ workspaceApi: api }));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

// A stable `t` like the real i18next one, so effects keyed on it run once.
const stableT = vi.hoisted(
  () => (key: string, options?: Record<string, unknown>) =>
    (options?.defaultValue as string) ?? key,
);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: stableT }),
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({ isOpen, children, footer }: any) =>
    isOpen ? (
      <div>
        {children}
        <div>{footer}</div>
      </div>
    ) : null,
}));

vi.mock("@/components/MarkdownEditor", () => ({
  default: ({ value, onChange }: any) => (
    <textarea
      data-testid="editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const todayFilename = () => {
  const now = new Date();
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}.md`;
};

describe("WorkspaceFileEditor", () => {
  beforeEach(() => {
    Object.values(api).forEach((fn) => fn.mockReset());
  });

  it("disables save and never writes after a failed read", async () => {
    api.readFile.mockRejectedValue(new Error("permission denied"));

    render(
      <WorkspaceFileEditor filename="AGENTS.md" isOpen onClose={vi.fn()} />,
    );

    const save = await screen.findByRole("button", { name: "common.save" });
    await waitFor(() => expect(api.readFile).toHaveBeenCalled());
    await screen.findByText("workspace.loadFailed");
    expect(save).toBeDisabled();
    expect(screen.queryByTestId("editor")).not.toBeInTheDocument();

    fireEvent.click(save);
    expect(api.writeFile).not.toHaveBeenCalled();
  });

  it("re-enables save once a retry succeeds", async () => {
    api.readFile
      .mockRejectedValueOnce(new Error("locked"))
      .mockResolvedValueOnce("hello");
    api.writeFile.mockResolvedValue(undefined);

    render(
      <WorkspaceFileEditor filename="AGENTS.md" isOpen onClose={vi.fn()} />,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Retry" }));

    const editor = (await screen.findByTestId("editor")) as HTMLTextAreaElement;
    expect(editor.value).toBe("hello");
    const save = screen.getByRole("button", { name: "common.save" });
    expect(save).not.toBeDisabled();
    fireEvent.click(save);
    await waitFor(() =>
      expect(api.writeFile).toHaveBeenCalledWith("AGENTS.md", "hello"),
    );
  });
});

describe("DailyMemoryPanel create today", () => {
  beforeEach(() => {
    Object.values(api).forEach((fn) => fn.mockReset());
  });

  it("does not clobber today's existing note when the list failed to load", async () => {
    api.listDailyMemoryFiles.mockRejectedValue(new Error("ipc timeout"));
    api.readDailyMemoryFile.mockResolvedValue("existing note");
    api.writeDailyMemoryFile.mockResolvedValue(undefined);

    render(<DailyMemoryPanel isOpen onClose={vi.fn()} />);
    await waitFor(() => expect(api.listDailyMemoryFiles).toHaveBeenCalled());

    fireEvent.click(
      screen.getByRole("button", { name: /workspace.dailyMemory.createToday/ }),
    );

    const editor = (await screen.findByTestId("editor")) as HTMLTextAreaElement;
    await waitFor(() => expect(editor.value).toBe("existing note"));
    expect(api.readDailyMemoryFile).toHaveBeenCalledWith(todayFilename());

    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(api.writeDailyMemoryFile).toHaveBeenCalledWith(
        todayFilename(),
        "existing note",
      ),
    );
    expect(api.writeDailyMemoryFile).not.toHaveBeenCalledWith(
      todayFilename(),
      "",
    );
  });

  it("opens an empty editor when today's note does not exist yet", async () => {
    api.listDailyMemoryFiles.mockResolvedValue([]);
    api.readDailyMemoryFile.mockResolvedValue(null);

    render(<DailyMemoryPanel isOpen onClose={vi.fn()} />);
    await waitFor(() => expect(api.listDailyMemoryFiles).toHaveBeenCalled());

    fireEvent.click(
      screen.getByRole("button", { name: /workspace.dailyMemory.createToday/ }),
    );

    const editor = (await screen.findByTestId("editor")) as HTMLTextAreaElement;
    expect(editor.value).toBe("");
  });
});
