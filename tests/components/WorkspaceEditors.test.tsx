import {
  act,
  render,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
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

// Settings with an OpenClaw directory override (FE-UI-35).
vi.mock("@/lib/query/queries", () => ({
  useSettingsQuery: () => ({ data: { openclawConfigDir: "/custom/oc/" } }),
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({ isOpen, children, footer, onClose }: any) =>
    isOpen ? (
      <div>
        <button type="button" onClick={onClose}>
          panel-back
        </button>
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

    const save = await screen.findByRole("button", { name: "Save" });
    await waitFor(() => expect(api.readFile).toHaveBeenCalled());
    await screen.findByText("Failed to load");
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
    const save = screen.getByRole("button", { name: "Save" });
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

    fireEvent.click(screen.getByRole("button", { name: /Add Memory/ }));

    const editor = (await screen.findByTestId("editor")) as HTMLTextAreaElement;
    await waitFor(() => expect(editor.value).toBe("existing note"));
    expect(api.readDailyMemoryFile).toHaveBeenCalledWith(todayFilename());

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
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

    fireEvent.click(screen.getByRole("button", { name: /Add Memory/ }));

    const editor = (await screen.findByTestId("editor")) as HTMLTextAreaElement;
    expect(editor.value).toBe("");
  });
});

describe("DailyMemoryPanel search", () => {
  beforeEach(() => {
    Object.values(api).forEach((fn) => fn.mockReset());
    api.listDailyMemoryFiles.mockResolvedValue([
      {
        filename: "2026-01-01.md",
        date: "2026-01-01",
        sizeBytes: 10,
        preview: "x",
      },
    ]);
  });

  const result = (filename: string) => ({
    filename,
    date: filename.replace(".md", ""),
    sizeBytes: 1,
    snippet: filename,
    matchCount: 1,
  });

  it("ignores results that arrive for an older query (FE-UI-57)", async () => {
    const pending: Array<(value: unknown) => void> = [];
    api.searchDailyMemoryFiles.mockImplementation(
      () => new Promise((resolve) => pending.push(resolve)),
    );
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      render(<DailyMemoryPanel isOpen onClose={vi.fn()} />);
      await waitFor(() => expect(api.listDailyMemoryFiles).toHaveBeenCalled());
      fireEvent.click(
        screen.getByTitle("Full-text search across all daily memories. ⌘F"),
      );
      const input = await screen.findByPlaceholderText(
        "Search full content...",
      );

      fireEvent.change(input, { target: { value: "ab" } });
      await act(async () => {
        vi.advanceTimersByTime(350);
      });
      fireEvent.change(input, { target: { value: "abc" } });
      await act(async () => {
        vi.advanceTimersByTime(350);
      });
      expect(pending).toHaveLength(2);

      // Newer query resolves first, the stale one last.
      await act(async () => {
        pending[1]([result("new.md")]);
      });
      await act(async () => {
        pending[0]([result("old.md")]);
      });

      expect(await screen.findByText("new")).toBeInTheDocument();
      expect(screen.queryByText("old")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("delete is a separate, labelled button (FE-UI-11)", async () => {
    render(<DailyMemoryPanel isOpen onClose={vi.fn()} />);
    await screen.findByText("2026-01-01");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(await screen.findByText("Delete Daily Memory")).toBeInTheDocument();
    // Opening the delete confirm did not open the file.
    expect(api.readDailyMemoryFile).not.toHaveBeenCalled();
  });
});

describe("WorkspaceFileEditor unsaved-changes guard (FE-UI-34)", () => {
  beforeEach(() => {
    Object.values(api).forEach((fn) => fn.mockReset());
  });

  it("asks before discarding edits and closes directly when clean", async () => {
    api.readFile.mockResolvedValue("hello");
    const onClose = vi.fn();
    render(
      <WorkspaceFileEditor filename="AGENTS.md" isOpen onClose={onClose} />,
    );

    const editor = await screen.findByTestId("editor");
    await waitFor(() => expect(editor).toHaveValue("hello"));
    fireEvent.change(editor, { target: { value: "hello!" } });

    fireEvent.click(screen.getByText("panel-back"));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole("button", { name: "Discard" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes without asking when nothing changed", async () => {
    api.readFile.mockResolvedValue("hello");
    const onClose = vi.fn();
    render(
      <WorkspaceFileEditor filename="AGENTS.md" isOpen onClose={onClose} />,
    );
    await waitFor(() =>
      expect(screen.getByTestId("editor")).toHaveValue("hello"),
    );
    fireEvent.click(screen.getByText("panel-back"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("workspace paths (FE-UI-35)", () => {
  beforeEach(() => {
    Object.values(api).forEach((fn) => fn.mockReset());
    api.listDailyMemoryFiles.mockResolvedValue([]);
  });

  it("shows the overridden OpenClaw directory instead of ~/.openclaw", async () => {
    render(<DailyMemoryPanel isOpen onClose={vi.fn()} />);
    expect(
      await screen.findByText("/custom/oc/workspace/memory/"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/~\/\.openclaw/)).not.toBeInTheDocument();
  });
});
