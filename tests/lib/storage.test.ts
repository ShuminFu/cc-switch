import { afterEach, describe, expect, it, vi } from "vitest";
import {
  safeStorageGet,
  safeStorageRemove,
  safeStorageSet,
} from "@/lib/storage";

describe("safe storage helpers", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    window.localStorage.clear();
  });

  it("reads, writes and removes values when storage works", () => {
    expect(safeStorageGet("k")).toBeNull();
    expect(safeStorageSet("k", "v")).toBe(true);
    expect(safeStorageGet("k")).toBe("v");
    expect(safeStorageRemove("k")).toBe(true);
    expect(safeStorageGet("k")).toBeNull();
  });

  it("swallows errors thrown by storage", () => {
    const fail = () => {
      throw new DOMException("quota", "QuotaExceededError");
    };
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(fail);
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(fail);
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(fail);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});

    expect(safeStorageGet("k")).toBeNull();
    expect(safeStorageSet("k", "v")).toBe(false);
    expect(safeStorageRemove("k")).toBe(false);
    expect(warn).toHaveBeenCalledTimes(3);
  });

  it("returns defaults when accessing localStorage itself throws", () => {
    vi.spyOn(window, "localStorage", "get").mockImplementation(() => {
      throw new DOMException("denied", "SecurityError");
    });

    expect(safeStorageGet("k")).toBeNull();
    expect(safeStorageSet("k", "v")).toBe(false);
    expect(safeStorageRemove("k")).toBe(false);
  });
});
