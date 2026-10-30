import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { settingsApi } from "@/lib/api/settings";

describe("sync download api (CCS-237)", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({ status: "downloaded", signed: true });
  });

  it.each([
    ["webdavSyncDownload", "webdav_sync_download"],
    ["s3SyncDownload", "s3_sync_download"],
  ] as const)(
    "%s refuses unsigned snapshots by default",
    async (method, command) => {
      await expect(settingsApi[method]()).resolves.toEqual({
        status: "downloaded",
        signed: true,
      });
      expect(invokeMock).toHaveBeenCalledWith(command, {
        allowUnsigned: false,
      });
    },
  );

  it.each([
    ["webdavSyncDownload", "webdav_sync_download"],
    ["s3SyncDownload", "s3_sync_download"],
  ] as const)("%s forwards allowUnsigned", async (method, command) => {
    await settingsApi[method]({ allowUnsigned: true });
    expect(invokeMock).toHaveBeenCalledWith(command, { allowUnsigned: true });
  });
});
