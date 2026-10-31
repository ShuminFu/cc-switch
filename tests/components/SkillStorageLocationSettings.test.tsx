import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { SkillStorageLocationSettings } from "@/components/settings/SkillStorageLocationSettings";
import { queryKeys } from "@/lib/query/keys";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: toastMock }));

describe("SkillStorageLocationSettings (FE-UI-58)", () => {
  it("shows which skills failed to migrate and refetches installed skills", async () => {
    mockInvoke({
      migrate_skill_storage: {
        migratedCount: 2,
        skippedCount: 0,
        errors: ["foo: permission denied"],
      },
    });
    const client = createTestQueryClient();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const onMigrated = vi.fn();
    render(
      <QueryClientProvider client={client}>
        <SkillStorageLocationSettings
          value="cc_switch"
          installedCount={0}
          onMigrated={onMigrated}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("radio", { name: /~\/.agents\/skills/ }));

    const list = await screen.findByTestId("skill-migration-errors");
    expect(list).toHaveTextContent("foo: permission denied");
    expect(toastMock.warning).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ description: "foo: permission denied" }),
    );
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.skills.all,
      }),
    );
    expect(onMigrated).toHaveBeenCalledWith("unified");
  });
});
