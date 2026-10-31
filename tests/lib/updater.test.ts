import { describe, expect, it, vi } from "vitest";

const checkAppUpdate = vi.fn();
vi.mock("@/lib/bindings", () => ({
  commands: {
    checkAppUpdate: (...args: unknown[]) => checkAppUpdate(...args),
  },
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: async () => "1.0.0",
}));

import { checkForUpdate } from "@/lib/updater";

// CMD-39: the check goes through the backend command, not the plugin JS API.
describe("checkForUpdate", () => {
  it("maps the backend update to UpdateInfo", async () => {
    checkAppUpdate.mockResolvedValue({
      status: "ok",
      data: {
        currentVersion: "1.0.0",
        availableVersion: "1.2.3",
        notes: "Fixes",
        pubDate: "2026-10-01",
      },
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
    expect(checkAppUpdate).toHaveBeenCalledWith(5);
  });

  it("reports up-to-date when no update is available", async () => {
    checkAppUpdate.mockResolvedValue({ status: "ok", data: null });
    await expect(checkForUpdate()).resolves.toEqual({ status: "up-to-date" });
    expect(checkAppUpdate).toHaveBeenLastCalledWith(30000);
  });

  it("does not import the updater plugin", async () => {
    const source = await import("node:fs").then((fs) =>
      fs.readFileSync("src/lib/updater.ts", "utf8"),
    );
    expect(source).not.toContain("plugin-updater");
  });
});
