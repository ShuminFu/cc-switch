import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { appsApi } from "@/lib/api/apps";
import { ALL_APP_IDS } from "@/entities/app/registry";

describe("appsApi.getRegistry", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("invokes get_app_registry without arguments and returns the descriptors", async () => {
    const descriptors = ALL_APP_IDS.map((id) => ({ id }));
    invokeMock.mockResolvedValue(descriptors);

    await expect(appsApi.getRegistry()).resolves.toEqual(descriptors);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("get_app_registry");
  });
});
