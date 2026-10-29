import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  createPresetLoader,
  useLazyPresets,
} from "@/features/providers/forms/lazyPresets";

describe("lazy preset loading (CCS-191)", () => {
  it("imports once, caches the result and retries after a failure", async () => {
    const importer = vi
      .fn<() => Promise<string[]>>()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue(["a"]);
    const loader = createPresetLoader(importer);

    expect(loader.peek()).toBeUndefined();
    await expect(loader.load()).rejects.toThrow("offline");
    expect(loader.peek()).toBeUndefined();

    const [first, second] = await Promise.all([loader.load(), loader.load()]);
    expect(first).toEqual(["a"]);
    expect(second).toBe(first);
    expect(loader.peek()).toBe(first);
    expect(importer).toHaveBeenCalledTimes(2);
  });

  it("returns undefined until loaded, then the cached presets synchronously", async () => {
    let resolve!: (value: string[]) => void;
    const loader = createPresetLoader(
      () => new Promise<string[]>((r) => (resolve = r)),
    );

    const first = renderHook(() => useLazyPresets(loader));
    expect(first.result.current).toBeUndefined();
    await act(async () => resolve(["a"]));
    await waitFor(() => expect(first.result.current).toEqual(["a"]));

    const second = renderHook(() => useLazyPresets(loader));
    expect(second.result.current).toEqual(["a"]);
  });

  it("keeps the preset modules out of the editors' static imports", async () => {
    const { readFileSync } = await import("node:fs");
    const path = await import("node:path");
    const root = path.resolve(__dirname, "../../..");
    const files = [
      "src/features/providers/forms/claude/ClaudeProviderForm.tsx",
      "src/features/providers/forms/codex/CodexProviderForm.tsx",
      "src/features/providers/forms/gemini/GeminiProviderForm.tsx",
      "src/features/providers/forms/claude-desktop/ClaudeDesktopProviderForm.tsx",
      "src/features/providers/forms/grokbuild/GrokBuildProviderForm.tsx",
      "src/components/providers/AddProviderDialog.tsx",
      "src/components/providers/forms/ClaudeFormFields.tsx",
      "src/components/providers/forms/hooks/useProviderCategory.ts",
    ];
    const valueImport =
      /^import\s+(?!type\b)[^;]*from\s+"@\/config\/(claude|codex|gemini|claudeDesktop)ProviderPresets";/m;
    for (const file of files) {
      const source = readFileSync(path.join(root, file), "utf8");
      expect(source, file).not.toMatch(valueImport);
    }
  });
});
