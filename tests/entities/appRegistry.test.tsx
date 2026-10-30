import type { ReactNode } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import type { BackendAppDescriptor } from "@/lib/api/apps";
import type { AppId } from "@/lib/api/types";
import {
  ALL_APP_IDS,
  APP_REGISTRY,
  appCaps,
  appLabel,
  appRegistryQueryKey,
  appsWithCap,
  isAppId,
  mergeAppRegistry,
  resolveAppId,
  sharedFeatureAppOf,
  useAppCaps,
  useAppRegistry,
} from "@/entities/app/registry";
import { APP_ICON_MAP, MCP_APP_IDS, SKILLS_APP_IDS } from "@/config/appConfig";
import { APP_STORAGE_KEY, getInitialApp } from "@/components/app-shell/views";
import registryFixture from "../fixtures/app-registry.json";
import { command } from "../msw/handlers";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

// 与 src-tauri/src/apps/registry.rs 的 registry_dto_matches_shared_fixture 共用同一份快照
const fixture = registryFixture as BackendAppDescriptor[];

const wrapper = () => {
  const client = createTestQueryClient();
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
};

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
    expect(MCP_APP_IDS).toEqual([
      "claude",
      "codex",
      "gemini",
      "grokbuild",
      "opencode",
      "openclaw",
      "hermes",
    ]);
    expect(appsWithCap("sessions")).toEqual(
      ALL_APP_IDS.filter((id) => id !== "claude-desktop"),
    );
    expect(appsWithCap("additive")).toEqual(["opencode", "openclaw", "hermes"]);
    expect(appsWithCap("proxyTakeover")).toEqual(
      ALL_APP_IDS.filter((id) => id !== "claude-desktop"),
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

  it("useAppCaps returns the registry caps", async () => {
    const { result } = renderHook(() => useAppCaps("openclaw"), {
      wrapper: wrapper(),
    });
    expect(result.current).toEqual(appCaps("openclaw"));
    await waitFor(() => expect(result.current).toEqual(appCaps("openclaw")));
    expect(result.current.skills).toBe(false);
    expect(result.current.sessions).toBe(true);
  });
});

describe("static registry vs backend get_app_registry fixture", () => {
  it("lists the same apps in the same order", () => {
    expect(fixture.map((app) => app.id)).toEqual([...ALL_APP_IDS]);
  });

  it.each(APP_REGISTRY.map((app, index) => [app.id, app, index] as const))(
    "%s matches the backend descriptor",
    (id, app, index) => {
      const remote = fixture[index];
      expect(remote.id).toBe(id);
      expect(app.label).toBe(remote.displayName);
      expect(app.aliases ?? []).toEqual(remote.aliases);
      expect(app.caps.mcp).toBe(remote.caps.mcp);
      expect(app.caps.skills).toBe(remote.caps.skills);
      expect(app.caps.proxyTakeover).toBe(remote.caps.proxyTakeover);
      expect(app.caps.additive).toBe(remote.syncMode === "additive");
    },
  );

  it("merging the fixture reproduces the static table", () => {
    expect(mergeAppRegistry(fixture)).toEqual(
      APP_REGISTRY.map((app) => ({ ...app, aliases: app.aliases ?? [] })),
    );
  });

  it("takes order, label and backend caps from the backend and keeps FE-only fields", () => {
    const codex = fixture.find((app) => app.id === "codex")!;
    const desktop = fixture.find((app) => app.id === "claude-desktop")!;
    const merged = mergeAppRegistry([
      { ...codex, displayName: "Codex CLI", syncMode: "additive" },
      { ...desktop, aliases: ["cd"] },
      { ...codex, id: "unknown-app" as AppId },
    ]);
    expect(merged.map((app) => app.id)).toEqual([
      "codex",
      "claude-desktop",
      ...ALL_APP_IDS.filter((id) => id !== "codex" && id !== "claude-desktop"),
    ]);
    expect(merged[0].label).toBe("Codex CLI");
    expect(merged[0].caps.additive).toBe(true);
    expect(merged[0].caps.sessions).toBe(true);
    expect(merged[1].aliases).toEqual(["cd"]);
    expect(merged[1].featureHost).toBe("claude");
  });
});

describe("useAppRegistry", () => {
  it("shows static caps before the backend resolves and backend caps after", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    server.use(
      command("get_app_registry", async () => {
        await gate;
        return fixture.map((app) =>
          app.id === "codex"
            ? { ...app, caps: { ...app.caps, mcp: false } }
            : app,
        );
      }),
    );

    const { result } = renderHook(
      () => ({ caps: useAppCaps("codex"), registry: useAppRegistry() }),
      { wrapper: wrapper() },
    );
    expect(result.current.registry).toBe(APP_REGISTRY);
    expect(result.current.caps.mcp).toBe(true);

    release();
    await waitFor(() => expect(result.current.caps.mcp).toBe(false));
    expect(result.current.caps.sessions).toBe(true);
    expect(result.current.registry).not.toBe(APP_REGISTRY);
    // 同步函数仍读取静态表
    expect(appCaps("codex").mcp).toBe(true);
  });

  it("falls back to the static table when the backend query fails", async () => {
    server.use(
      command("get_app_registry", () =>
        HttpResponse.json({ message: "boom" }, { status: 500 }),
      ),
    );
    const client = createTestQueryClient();

    const { result } = renderHook(() => useAppRegistry(), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    });
    await waitFor(() =>
      expect(client.getQueryState(appRegistryQueryKey)?.status).toBe("error"),
    );
    expect(result.current).toBe(APP_REGISTRY);
  });
});
