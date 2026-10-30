import { describe, it, expect } from "vitest";
import {
  ALL_APP_IDS,
  APP_REGISTRY,
  appHas,
  appsWith,
  getFirstVisibleApp,
  isAppId,
  isAppVisible,
} from "@/config/apps";
import { APP_ICON_MAP, MCP_APP_IDS, SKILLS_APP_IDS } from "@/config/appConfig";
import {
  PROXY_CAPABLE_APPS,
  proxyCapableAppsMatchRegistry,
} from "@/config/proxyApps";

describe("APP_REGISTRY", () => {
  it("lists every app once, in switcher order", () => {
    expect(ALL_APP_IDS).toEqual([
      "claude",
      "claude-desktop",
      "codex",
      "gemini",
      "grokbuild",
      "opencode",
      "openclaw",
      "hermes",
    ]);
    for (const id of ALL_APP_IDS) {
      expect(APP_REGISTRY[id].id).toBe(id);
      expect(APP_REGISTRY[id].labelKey).toBe(`apps.${id}`);
      expect(APP_ICON_MAP[id].label).toBe(APP_REGISTRY[id].label);
    }
  });

  it("derives the panel app lists from capabilities", () => {
    expect(SKILLS_APP_IDS).toEqual([
      "claude",
      "codex",
      "gemini",
      "grokbuild",
      "opencode",
      "hermes",
    ]);
    expect(MCP_APP_IDS).toEqual(SKILLS_APP_IDS);
    expect(appsWith("additiveLiveConfig")).toEqual([
      "opencode",
      "openclaw",
      "hermes",
    ]);
    expect(appsWith("universal")).toEqual(["claude", "codex", "gemini"]);
  });

  it("keeps PROXY_CAPABLE_APPS in sync with the proxy capability", () => {
    expect(proxyCapableAppsMatchRegistry()).toBe(true);
    expect(appsWith("failover")).toEqual([...PROXY_CAPABLE_APPS]);
  });

  it("maps shared-feature and skills target apps", () => {
    expect(APP_REGISTRY["claude-desktop"].featureApp).toBe("claude");
    expect(APP_REGISTRY["claude-desktop"].skillsTargetApp).toBe("claude");
    expect(APP_REGISTRY.openclaw.skillsTargetApp).toBe("claude");
    expect(APP_REGISTRY.codex.featureApp).toBe("codex");
    expect(appHas("claude-desktop", "desktopRoute")).toBe(true);
    expect(appHas("claude", "terminal")).toBe(true);
    expect(appHas("codex", "terminal")).toBe(false);
  });

  it("resolves visibility like the settings backend", () => {
    expect(isAppVisible(undefined, "hermes")).toBe(true);
    expect(isAppVisible({ hermes: false }, "hermes")).toBe(false);
    expect(isAppVisible({}, "hermes")).toBe(false);
    expect(getFirstVisibleApp({ claude: false, codex: true })).toBe("codex");
    expect(getFirstVisibleApp({})).toBe("claude");
    expect(getFirstVisibleApp(undefined)).toBe("claude");
  });

  it("validates app ids", () => {
    expect(isAppId("grokbuild")).toBe(true);
    expect(isAppId("toString")).toBe(false);
    expect(isAppId(42)).toBe(false);
  });
});
