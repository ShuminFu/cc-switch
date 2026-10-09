import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RepoManagerPanel } from "@/components/skills/RepoManagerPanel";
import type { SkillRepo } from "@/lib/api/skills";

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
}));

const repos: SkillRepo[] = [
  { owner: "anthropics", name: "skills", branch: "main", enabled: true },
  { owner: "acme", name: "private-skills", branch: "dev", enabled: false },
];

describe("RepoManagerPanel", () => {
  it("renders a toggle per repository and reports the new enabled state", () => {
    const onToggle = vi.fn().mockResolvedValue(undefined);
    render(
      <RepoManagerPanel
        repos={repos}
        skills={[]}
        onAdd={vi.fn()}
        onRemove={vi.fn()}
        onToggle={onToggle}
        onClose={() => {}}
      />,
    );

    const switches = screen.getAllByRole("switch", { name: "参与技能发现" });
    expect(switches).toHaveLength(2);
    expect(switches[0]).toHaveAttribute("aria-checked", "true");
    expect(switches[1]).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText("已停用")).toBeInTheDocument();

    fireEvent.click(switches[0]);
    expect(onToggle).toHaveBeenCalledWith(repos[0], false);

    fireEvent.click(switches[1]);
    expect(onToggle).toHaveBeenCalledWith(repos[1], true);
  });

  it("omits the toggle when no handler is provided", () => {
    render(
      <RepoManagerPanel
        repos={repos}
        skills={[]}
        onAdd={vi.fn()}
        onRemove={vi.fn()}
        onClose={() => {}}
      />,
    );

    expect(screen.queryByRole("switch")).toBeNull();
  });
});
