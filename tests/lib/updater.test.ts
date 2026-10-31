import { describe, expect, it, vi } from "vitest";

const checkMock = vi.fn();
vi.mock("@tauri-apps/plugin-updater", () => ({
  check: (...args: unknown[]) => checkMock(...args),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: async () => "1.0.0",
}));

import { checkForUpdate } from "@/lib/updater";

// FE-ARCH-32: the plugin result is typed; release notes come from `body`.
describe("checkForUpdate", () => {
  it("maps the plugin Update to UpdateInfo", async () => {
    checkMock.mockResolvedValue({
      version: "1.2.3",
      body: "Fixes",
      date: "2026-10-01",
    });
    await expect(checkForUpdate({ timeout: 5 })).resolves.toEqual({
      status: "available",
      info: {
        currentVersion: "1.0.0",
        availableVersion: "1.2.3",
        notes: "Fixes",
        pubDate: "2026-10-01",
      },
    });
    expect(checkMock).toHaveBeenCalledWith({ timeout: 5 });
  });

  it("reports up-to-date when the plugin returns null", async () => {
    checkMock.mockResolvedValue(null);
    await expect(checkForUpdate()).resolves.toEqual({ status: "up-to-date" });
  });
});
