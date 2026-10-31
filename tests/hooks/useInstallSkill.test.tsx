import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { useInstallSkill, useUninstallSkill } from "@/hooks/useSkills";
import type { DiscoverableSkill, InstalledSkill } from "@/lib/api/skills";
import { queryKeys } from "@/lib/query/keys";
import { createTestQueryClient } from "../utils/testQueryClient";

const installed: InstalledSkill = {
  id: "owner/repo:skill-a",
  name: "Skill A",
  directory: "skill-a",
  apps: {
    claude: true,
    codex: false,
    gemini: false,
    opencode: false,
    openclaw: false,
    hermes: false,
  },
  installedAt: 0,
  updatedAt: 0,
};

vi.mock("@/lib/api/skills", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/skills")>();
  return {
    ...actual,
    skillsApi: {
      ...actual.skillsApi,
      installUnified: vi.fn(async () => installed),
      uninstallUnified: vi.fn(async () => ({ backupPath: null })),
    },
  };
});

// FE-ARCH-44
describe("useInstallSkill cache patch", () => {
  it("keeps exactly one entry per id when the same skill installs twice", async () => {
    const queryClient = createTestQueryClient();
    queryClient.setQueryData<InstalledSkill[]>(queryKeys.skills.installed, []);
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(
      () => ({ install: useInstallSkill(), uninstall: useUninstallSkill() }),
      { wrapper },
    );
    const skill = {
      key: "owner/repo:skill-a",
      directory: "skill-a",
      repoOwner: "owner",
      repoName: "repo",
    } as DiscoverableSkill;

    await act(async () => {
      await result.current.install.mutateAsync({ skill, currentApp: "claude" });
      await result.current.install.mutateAsync({ skill, currentApp: "claude" });
    });
    const after = queryClient.getQueryData<InstalledSkill[]>(
      queryKeys.skills.installed,
    );
    expect(after?.filter((s) => s.id === installed.id)).toHaveLength(1);

    await act(async () => {
      await result.current.uninstall.mutateAsync({ id: installed.id });
    });
    expect(
      queryClient.getQueryData<InstalledSkill[]>(queryKeys.skills.installed),
    ).toEqual([]);
  });
});
