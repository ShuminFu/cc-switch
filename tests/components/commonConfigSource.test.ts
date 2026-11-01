import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  getCommonConfigSnippet: vi.fn(),
  setCommonConfigSnippet: vi.fn(),
}));
vi.mock("@/lib/api", () => ({ configApi: api }));

import {
  LEGACY_COMMON_CONFIG_KEYS,
  loadCommonConfigSnippet,
} from "@/components/providers/forms/hooks/commonConfigSource";

describe("loadCommonConfigSnippet (FE-PROV-18)", () => {
  beforeEach(() => {
    api.getCommonConfigSnippet.mockReset();
    api.setCommonConfigSnippet.mockReset().mockResolvedValue(undefined);
    window.localStorage.clear();
  });

  it("returns the stored snippet without touching localStorage", async () => {
    api.getCommonConfigSnippet.mockResolvedValue('{"a":1}');
    window.localStorage.setItem(LEGACY_COMMON_CONFIG_KEYS.claude, "legacy");
    await expect(loadCommonConfigSnippet("claude")).resolves.toBe('{"a":1}');
    expect(api.setCommonConfigSnippet).not.toHaveBeenCalled();
    expect(window.localStorage.getItem(LEGACY_COMMON_CONFIG_KEYS.claude)).toBe(
      "legacy",
    );
  });

  it("migrates a legacy snippet into config.json and removes it", async () => {
    api.getCommonConfigSnippet.mockResolvedValue("");
    window.localStorage.setItem(LEGACY_COMMON_CONFIG_KEYS.codex, "a = 1");
    await expect(loadCommonConfigSnippet("codex")).resolves.toBe("a = 1");
    expect(api.setCommonConfigSnippet).toHaveBeenCalledWith("codex", "a = 1");
    expect(
      window.localStorage.getItem(LEGACY_COMMON_CONFIG_KEYS.codex),
    ).toBeNull();
  });

  it("keeps a legacy snippet the caller rejects", async () => {
    api.getCommonConfigSnippet.mockResolvedValue(null);
    window.localStorage.setItem(LEGACY_COMMON_CONFIG_KEYS.gemini, "bad");
    await expect(
      loadCommonConfigSnippet("gemini", () => false),
    ).resolves.toBeNull();
    expect(api.setCommonConfigSnippet).not.toHaveBeenCalled();
    expect(window.localStorage.getItem(LEGACY_COMMON_CONFIG_KEYS.gemini)).toBe(
      "bad",
    );
  });

  it("does not remove the legacy copy when the migration write fails", async () => {
    api.getCommonConfigSnippet.mockResolvedValue("");
    api.setCommonConfigSnippet.mockRejectedValue(new Error("disk"));
    window.localStorage.setItem(LEGACY_COMMON_CONFIG_KEYS.claude, "{}x");
    await expect(loadCommonConfigSnippet("claude")).resolves.toBeNull();
    expect(window.localStorage.getItem(LEGACY_COMMON_CONFIG_KEYS.claude)).toBe(
      "{}x",
    );
  });
});
