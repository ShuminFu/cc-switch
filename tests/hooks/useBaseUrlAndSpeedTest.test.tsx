import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AppId } from "@/lib/api";
import { useBaseUrlState } from "@/components/providers/forms/hooks/useBaseUrlState";
import { useSpeedTestEndpoints } from "@/components/providers/forms/hooks/useSpeedTestEndpoints";

const settingsConfig = JSON.stringify({
  env: {
    ANTHROPIC_BASE_URL: "https://anthropic.example",
    GOOGLE_GEMINI_BASE_URL: "https://gemini.example",
  },
});
const codexConfig = 'base_url = "https://codex.example/v1"\n';

// 每个应用从哪个字段回填 Base URL（取代 useBaseUrlState 中的 app-id 判断）
describe("useBaseUrlState per app", () => {
  it.each<[AppId, string, string, string]>([
    ["claude", "https://anthropic.example", "", ""],
    ["claude-desktop", "https://anthropic.example", "", ""],
    ["codex", "", "https://codex.example/v1", ""],
    ["gemini", "https://gemini.example", "", "https://gemini.example"],
    ["grokbuild", "", "", ""],
    ["opencode", "", "", ""],
    ["openclaw", "", "", ""],
    ["hermes", "", "", ""],
  ])(
    "%s fills baseUrl=%s codex=%s gemini=%s",
    (appType, baseUrl, codexBaseUrl, geminiBaseUrl) => {
      const { result } = renderHook(() =>
        useBaseUrlState({
          appType,
          category: "custom",
          settingsConfig,
          codexConfig,
          onSettingsConfigChange: vi.fn(),
          onCodexConfigChange: vi.fn(),
        }),
      );
      expect(result.current.baseUrl).toBe(baseUrl);
      expect(result.current.codexBaseUrl).toBe(codexBaseUrl);
      expect(result.current.geminiBaseUrl).toBe(geminiBaseUrl);
    },
  );

  it("does not fill official providers", () => {
    const { result } = renderHook(() =>
      useBaseUrlState({
        appType: "claude",
        category: "official",
        settingsConfig,
        onSettingsConfigChange: vi.fn(),
      }),
    );
    expect(result.current.baseUrl).toBe("");
  });
});

// 每个应用的测速候选端点来源（取代 useSpeedTestEndpoints 中的 app-id 判断）
describe("useSpeedTestEndpoints per app", () => {
  const envUrls = [
    "https://current.example",
    "https://anthropic.example",
    "https://gemini.example",
  ];

  it.each<[AppId, string[]]>([
    ["claude", envUrls],
    ["gemini", envUrls],
    ["codex", ["https://codex-current.example", "https://codex.example/v1"]],
    ["claude-desktop", []],
    ["grokbuild", []],
    ["opencode", []],
    ["openclaw", []],
    ["hermes", []],
  ])("%s collects %j", (appId, urls) => {
    const { result } = renderHook(() =>
      useSpeedTestEndpoints({
        appId,
        selectedPresetId: null,
        presetEntries: [],
        baseUrl: "https://current.example/",
        codexBaseUrl: "https://codex-current.example",
        initialData: {
          settingsConfig: {
            ...JSON.parse(settingsConfig),
            config: codexConfig,
          },
        },
      }),
    );
    expect(result.current.map((endpoint) => endpoint.url)).toEqual(urls);
  });
});
