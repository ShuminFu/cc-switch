import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { AppId } from "@/lib/api/types";
import {
  ALL_APP_IDS,
  APP_REGISTRY,
  appCaps,
  appLabel,
  appsWithCap,
  isAppId,
  resolveAppId,
  sharedFeatureAppOf,
  useAppCaps,
} from "@/entities/app/registry";
import { APP_ICON_MAP, MCP_APP_IDS, SKILLS_APP_IDS } from "@/config/appConfig";
import { APP_STORAGE_KEY, getInitialApp } from "@/components/app-shell/views";

const EXPECTED_LABELS: Record<AppId, string> = {
  claude: "Claude",
  "claude-desktop": "Claude Desktop",
  codex: "Codex",
  gemini: "Gemini",
  grokbuild: "Grok Build",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
};

describe("app registry", () => {
  it("covers every AppId exactly once, in UI order", () => {
    expect(ALL_APP_IDS).toEqual(Object.keys(EXPECTED_LABELS));
    expect(new Set(ALL_APP_IDS).size).toBe(APP_REGISTRY.length);
  });

  it.each(Object.entries(EXPECTED_LABELS))(
    "appLabel(%s) is %s",
    (id, label) => {
      expect(appLabel(id)).toBe(label);
    },
  );

  it("does not fall back to OpenCode for apps that are not OpenCode", () => {
    for (const id of ALL_APP_IDS) {
      if (id !== "opencode") {
        expect(appLabel(id)).not.toBe("OpenCode");
      }
    }
    expect(appLabel("unknown-app")).toBe("unknown-app");
    expect(appLabel("")).not.toBe("OpenCode");
  });

  it("has an icon and colours for every app, with matching labels", () => {
    for (const id of ALL_APP_IDS) {
      expect(APP_ICON_MAP[id]).toBeDefined();
      expect(APP_ICON_MAP[id].label).toBe(appLabel(id));
    }
  });

  it("validates app ids from untrusted input", () => {
    for (const id of ALL_APP_IDS) {
      expect(isAppId(id)).toBe(true);
    }
    expect(isAppId("unknown")).toBe(false);
    expect(isAppId(null)).toBe(false);
    expect(isAppId(undefined)).toBe(false);
    expect(isAppId("__proto__")).toBe(false);
  });

  it("resolves persisted app ids through registry aliases", () => {
    for (const id of ALL_APP_IDS) {
      expect(resolveAppId(id)).toBe(id);
    }
    expect(resolveAppId("claude_desktop")).toBe("claude-desktop");
    expect(resolveAppId("claudeDesktop")).toBe("claude-desktop");
    expect(resolveAppId(" Grok ")).toBe("grokbuild");
    expect(resolveAppId("grok-build")).toBe("grokbuild");
    expect(resolveAppId("unknown")).toBeUndefined();
    expect(resolveAppId("__proto__")).toBeUndefined();
    expect(resolveAppId(null)).toBeUndefined();
  });

  it("restores the last app from localStorage via aliases", () => {
    try {
      window.localStorage.setItem(APP_STORAGE_KEY, "claude_desktop");
      expect(getInitialApp()).toBe("claude-desktop");
      window.localStorage.setItem(APP_STORAGE_KEY, "codex");
      expect(getInitialApp()).toBe("codex");
      window.localStorage.setItem(APP_STORAGE_KEY, "bogus");
      expect(getInitialApp()).toBe("claude");
    } finally {
      window.localStorage.removeItem(APP_STORAGE_KEY);
    }
  });

  it("matches the previous capability chains", () => {
    expect(appsWithCap("skills")).toEqual([
      "claude",
      "codex",
      "gemini",
      "grokbuild",
      "opencode",
      "hermes",
    ]);
    expect(SKILLS_APP_IDS).toEqual(appsWithCap("skills"));
    expect(MCP_APP_IDS).toEqual(appsWithCap("skills"));
    expect(appsWithCap("sessions")).toEqual(
      ALL_APP_IDS.filter((id) => id !== "claude-desktop"),
    );
    expect(appsWithCap("additive")).toEqual(["opencode", "openclaw", "hermes"]);
    expect(appsWithCap("proxyTakeover")).toEqual(
      ALL_APP_IDS.filter(
        (id) => id !== "claude-desktop" && !appCaps(id).additive,
      ),
    );
    expect(appsWithCap("healthCheck")).toEqual(["openclaw"]);
    expect(appsWithCap("setDefault")).toEqual(["openclaw", "hermes"]);
    expect(appsWithCap("defaultIsSwitch")).toEqual(["hermes"]);
  });

  it("routes shared feature views of Claude Desktop to Claude", () => {
    expect(sharedFeatureAppOf("claude-desktop")).toBe("claude");
    for (const id of ALL_APP_IDS) {
      if (id !== "claude-desktop") {
        expect(sharedFeatureAppOf(id)).toBe(id);
      }
    }
  });

  it("treats unknown apps as having no capabilities", () => {
    expect(appCaps("unknown")).toEqual({
      sessions: false,
      skills: false,
      mcp: false,
      additive: false,
      proxyTakeover: false,
      healthCheck: false,
      setDefault: false,
      defaultIsSwitch: false,
    });
  });

  it("useAppCaps returns the registry caps", () => {
    const { result } = renderHook(() => useAppCaps("openclaw"));
    expect(result.current).toEqual(appCaps("openclaw"));
    expect(result.current.skills).toBe(false);
    expect(result.current.sessions).toBe(true);
  });
});
