import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import { SessionTrashDialog } from "@/components/sessions/SessionTrashDialog";
import { TooltipProvider } from "@/components/ui/tooltip";
import { server } from "../msw/server";

const toastSuccess = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: vi.fn(),
  },
}));

const renderDialog = () =>
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <TooltipProvider>
        <SessionTrashDialog />
      </TooltipProvider>
    </QueryClientProvider>,
  );

describe("SessionTrashDialog (SESS-41)", () => {
  it("lists recently deleted sessions and restores one", async () => {
    let entries = [
      {
        id: "1700000000000-claude-abc-1",
        providerId: "claude",
        sessionId: "abc",
        deletedAt: 1_700_000_000_000,
        items: [
          {
            originalPath: "/home/u/.claude/projects/p/abc.jsonl",
            storedName: "0-abc.jsonl",
          },
        ],
      },
    ];
    const restored: string[] = [];
    server.use(
      http.post("http://tauri.local/list_session_trash", () =>
        HttpResponse.json(entries),
      ),
      http.post(
        "http://tauri.local/restore_session_from_trash",
        async ({ request }) => {
          const { id } = (await request.json()) as { id: string };
          restored.push(id);
          const entry = entries.find((e) => e.id === id);
          entries = entries.filter((e) => e.id !== id);
          return HttpResponse.json(entry);
        },
      ),
    );

    renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Recently deleted" }));

    expect(await screen.findByText("abc.jsonl")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Restore/ }));

    await waitFor(() =>
      expect(restored).toEqual(["1700000000000-claude-abc-1"]),
    );
    expect(
      await screen.findByText("No recently deleted sessions"),
    ).toBeInTheDocument();
    expect(toastSuccess).toHaveBeenCalledWith("Session restored");
  });

  it("shows an empty state when nothing was deleted", async () => {
    renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Recently deleted" }));
    expect(
      await screen.findByText("No recently deleted sessions"),
    ).toBeInTheDocument();
  });
});
