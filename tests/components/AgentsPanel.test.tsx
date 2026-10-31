import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { AgentsPanel } from "@/components/agents/AgentsPanel";
import { mockInvoke } from "../msw/invokeRecorder";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const renderPanel = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <AgentsPanel onOpenChange={vi.fn()} />
    </QueryClientProvider>,
  );

describe("AgentsPanel (PRODUCT-42)", () => {
  it("lists agents and flags files with invalid frontmatter", async () => {
    mockInvoke({
      list_claude_agents: [
        {
          fileName: "reviewer.md",
          path: "/h/.claude/agents/reviewer.md",
          name: "reviewer",
          description: "Reviews code",
          model: "sonnet",
          prompt: "You review.",
        },
        {
          fileName: "notes.md",
          path: "/h/.claude/agents/notes.md",
          name: "notes",
          prompt: "plain",
          invalid: true,
        },
      ],
    });
    renderPanel();
    expect(await screen.findByText("reviewer")).toBeInTheDocument();
    expect(screen.getByText("sonnet")).toBeInTheDocument();
    expect(screen.getAllByText("Invalid frontmatter")).toHaveLength(1);

    fireEvent.click(screen.getByText("reviewer"));
    expect(screen.getByText("You review.")).toBeInTheDocument();
  });

  it("creates an agent only with a valid name and description", async () => {
    const api = mockInvoke({
      list_claude_agents: [],
      create_claude_agent: {
        fileName: "code-reviewer.md",
        path: "/h/.claude/agents/code-reviewer.md",
        name: "code-reviewer",
        prompt: "Be strict.",
      },
    });
    renderPanel();
    await screen.findByText(/No subagents yet/);
    fireEvent.click(screen.getByRole("button", { name: /New agent/ }));

    const create = await screen.findByRole("button", { name: "Create agent" });
    expect(create).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "Bad Name" },
    });
    expect(
      screen.getByText("Use lowercase letters, digits and hyphens."),
    ).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "code-reviewer" },
    });
    fireEvent.change(screen.getByLabelText("When to use this agent"), {
      target: { value: "Reviews diffs" },
    });
    fireEvent.change(screen.getByLabelText("System prompt"), {
      target: { value: "Be strict." },
    });
    expect(create).toBeEnabled();
    fireEvent.click(create);

    await waitFor(() =>
      expect(api.callsTo("create_claude_agent")).toHaveLength(1),
    );
    expect(api.callsTo("create_claude_agent")[0].args).toEqual({
      agent: {
        name: "code-reviewer",
        description: "Reviews diffs",
        model: "",
        tools: "",
        prompt: "Be strict.",
      },
    });
    // The list is refreshed after creating.
    await waitFor(() =>
      expect(api.callsTo("list_claude_agents").length).toBeGreaterThan(1),
    );
  });
});
