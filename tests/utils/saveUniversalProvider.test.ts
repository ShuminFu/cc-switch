import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TFunction } from "i18next";
import { toast } from "sonner";
import { saveUniversalProvider } from "@/components/universal/saveUniversalProvider";
import type { UniversalProvider } from "@/types";
import { mockInvoke } from "../msw/invokeRecorder";

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

const t = ((key: string) => key) as unknown as TFunction;
const provider = { id: "u1", name: "Relay" } as UniversalProvider;

beforeEach(() => {
  vi.mocked(toast.error).mockClear();
  vi.mocked(toast.success).mockClear();
  vi.mocked(toast.warning).mockClear();
});

describe("saveUniversalProvider (FE-PROV-47)", () => {
  it("reports a sync failure after a successful upsert as such", async () => {
    mockInvoke({
      upsert_universal_provider: true,
      sync_universal_provider: () => {
        throw new Error("codex config locked");
      },
    });
    await expect(
      saveUniversalProvider(provider, { sync: true, isEdit: false, t }),
    ).resolves.toBe("syncFailed");
    expect(toast.warning).toHaveBeenCalledWith(
      "universalProvider.addedButSyncFailed",
    );
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("reports an upsert failure without syncing", async () => {
    const { callsTo } = mockInvoke({
      upsert_universal_provider: () => {
        throw new Error("db locked");
      },
      sync_universal_provider: true,
    });
    await expect(
      saveUniversalProvider(provider, { sync: true, isEdit: true, t }),
    ).resolves.toBe("saveFailed");
    expect(toast.error).toHaveBeenCalledWith("universalProvider.saveError");
    expect(callsTo("sync_universal_provider")).toHaveLength(0);
  });

  it("saves and syncs", async () => {
    mockInvoke({
      upsert_universal_provider: true,
      sync_universal_provider: true,
    });
    await expect(
      saveUniversalProvider(provider, { sync: true, isEdit: false, t }),
    ).resolves.toBe("saved");
    expect(toast.success).toHaveBeenCalledWith(
      "universalProvider.addedAndSynced",
    );
  });
});
