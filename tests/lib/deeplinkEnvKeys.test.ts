import { describe, expect, it } from "vitest";
import { classifyClaudeEnvKey } from "@/lib/deeplink/envKeys";

describe("classifyClaudeEnvKey", () => {
  it("matches the backend allow/deny lists case-insensitively", () => {
    expect(classifyClaudeEnvKey("ANTHROPIC_CUSTOM_HEADERS")).toBe("allowed");
    expect(classifyClaudeEnvKey("API_TIMEOUT_MS")).toBe("allowed");
    expect(classifyClaudeEnvKey("https_proxy")).toBe("allowed");
    expect(classifyClaudeEnvKey("NODE_OPTIONS")).toBe("denied");
    expect(classifyClaudeEnvKey("ld_preload")).toBe("denied");
    expect(classifyClaudeEnvKey("Path")).toBe("denied");
    expect(classifyClaudeEnvKey("NODE_EXTRA_CA_CERTS")).toBe("unlisted");
  });

  it("only folds ASCII letters, like the backend", () => {
    // "ſ".toUpperCase() === "S"; the backend keeps this key as unlisted.
    expect(classifyClaudeEnvKey("NODE_OPTIONſ")).toBe("unlisted");
  });
});
