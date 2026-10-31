import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import { SkillCard } from "@/components/skills/SkillCard";
import UnifiedSkillsPanel from "@/components/skills/UnifiedSkillsPanel";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({
  error: vi.fn(),
  success: vi.fn(),
  info: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: toastMock }));
describe("SkillCard (FE-UI-33)", () => {
  it("shows installed skills as installed instead of an Uninstall that only toasts", () => {
    render(
      <SkillCard
        skill={{
          key: "k",
          name: "Writer",
          description: "",
          directory: "writer",
          repoOwner: "o",
          repoName: "r",
          repoBranch: "main",
          installed: true,
        }}
        onInstall={vi.fn()}
      />,
    );
    expect(screen.queryByText("Uninstall")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Installed/ })).toBeDisabled();
  });
});

describe("Update all skills (FE-UI-33)", () => {
  it("reports failures in one summary toast", async () => {
    const skill = (id: string) => ({
      id,
      name: id,
      directory: id,
      apps: { claude: true },
      installedAt: 0,
      updatedAt: 0,
    });
    mockInvoke({
      get_installed_skills: [skill("a"), skill("b"), skill("c")],
      get_skill_backups: [],
      scan_unmanaged_skills: [],
      check_skill_updates: [
        { id: "a", name: "a" },
        { id: "b", name: "b" },
        { id: "c", name: "c" },
      ],
      update_skill: ({ id }: Record<string, unknown>) => {
        if (id !== "a") throw new Error(`fail ${id}`);
        return skill("a");
      },
    });
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} currentApp="claude" />
      </QueryClientProvider>,
    );

    await screen.findByText("a");
    fireEvent.click(screen.getByText("Check Updates"));
    const updateAll = await screen.findByText("Update All (3)");
    await waitFor(() => expect(updateAll.closest("button")).toBeEnabled());
    fireEvent.click(updateAll);

    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith(
        "Failed to update",
        expect.objectContaining({
          description: expect.stringContaining("fail b"),
        }),
      ),
    );
    const failureToasts = toastMock.error.mock.calls.filter(
      ([title]) => title === "Failed to update",
    );
    expect(failureToasts).toHaveLength(1);
    expect(failureToasts[0][1].description).toContain("fail c");
  });
});
