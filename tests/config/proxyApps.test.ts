import { describe, it, expect } from "vitest";
import {
  PROXY_CAPABLE_APPS,
  getAppLabel,
  isAnyTakeoverActive,
  isProxyCapableApp,
} from "@/config/proxyApps";

describe("proxyApps", () => {
  it("lists the four proxy-capable apps", () => {
    expect([...PROXY_CAPABLE_APPS]).toEqual([
      "claude",
      "codex",
      "gemini",
      "grokbuild",
    ]);
    expect(isProxyCapableApp("grokbuild")).toBe(true);
    expect(isProxyCapableApp("opencode")).toBe(false);
  });

  it("resolves labels from the app config", () => {
    expect(getAppLabel("grokbuild")).toBe("Grok Build");
    expect(getAppLabel("claude")).toBe("Claude");
    expect(getAppLabel("gemini")).toBe("Gemini");
    expect(getAppLabel("unknown-app")).toBe("unknown-app");
  });

  it("detects any active takeover, including Grok Build", () => {
    expect(isAnyTakeoverActive(undefined)).toBe(false);
    expect(
      isAnyTakeoverActive({ claude: false, codex: false, gemini: false }),
    ).toBe(false);
    expect(isAnyTakeoverActive({ grokbuild: true })).toBe(true);
  });
});
