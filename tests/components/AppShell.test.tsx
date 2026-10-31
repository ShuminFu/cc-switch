import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ALL_APP_IDS, appCaps } from "@/entities/app/registry";
import {
  VALID_VIEWS,
  VIEW_STORAGE_KEY,
  VIEW_TITLES,
  getBackView,
  getInitialView,
  isView,
  type View,
} from "@/components/app-shell/views";
import {
  AppFeatureNav,
  getFeatureNav,
} from "@/components/app-shell/AppFeatureNav";
import {
  AppHeader,
  type AppHeaderProps,
} from "@/components/app-shell/AppHeader";
import type { ShellPanelRefs } from "@/components/app-shell/panelRefs";

vi.mock("@/components/UpdateBadge", () => ({
  UpdateBadge: ({ onClick }: { onClick: () => void }) => (
    <button onClick={onClick}>update-badge</button>
  ),
}));

vi.mock("@/components/profiles/ProfileSwitcher", () => ({
  ProfileSwitcher: () => null,
}));

const t = i18n.t.bind(i18n);

const panelRefs: ShellPanelRefs = {
  prompts: { current: null },
  mcp: { current: null },
  skills: { current: null },
  skillsDiscovery: { current: null },
};

const renderHeader = (overrides: Partial<AppHeaderProps>) => {
  const props: AppHeaderProps = {
    currentView: "providers",
    onNavigate: vi.fn(),
    onOpenSettings: vi.fn(),
    activeApp: "claude",
    onSwitchApp: vi.fn(),
    visibleApps: Object.fromEntries(
      ALL_APP_IDS.map((id) => [id, true]),
    ) as unknown as AppHeaderProps["visibleApps"],
    activeAppCaps: appCaps("claude"),
    sharedFeatureApp: "claude",
    sharedFeatureCaps: appCaps("claude"),
    settings: undefined,
    isProxyRunning: false,
    isTakeoverActive: false,
    dragBarHeight: 0,
    panelRefs,
    skillsDiscoverySource: "repos",
    onOpenSkillsDiscovery: vi.fn(),
    onOpenHermesWebUI: vi.fn(),
    onAddProvider: vi.fn(),
    ...overrides,
  };
  render(
    <QueryClientProvider client={new QueryClient()}>
      <AppHeader {...props} />
    </QueryClientProvider>,
  );
  return props;
};

describe("app shell view map", () => {
  afterEach(() => {
    window.localStorage.clear();
  });

  it("has a title for every sub view and none for the home view", () => {
    const subViews = VALID_VIEWS.filter((view) => view !== "providers");
    expect(Object.keys(VIEW_TITLES).sort()).toEqual([...subViews].sort());
    expect(VALID_VIEWS).toContain("providers");
    expect(new Set(VALID_VIEWS).size).toBe(VALID_VIEWS.length);
  });

  it("resolves titles through i18n keys", () => {
    const ctx = { sharedFeatureApp: "codex" as const };
    expect(VIEW_TITLES.settings(t, ctx)).toBe("settings.title");
    expect(VIEW_TITLES.skillsDiscovery(t, ctx)).toBe("skills.title");
    expect(VIEW_TITLES.hermesMemory(t, ctx)).toBe("hermes.memory.title");
    expect(VIEW_TITLES.universal(t, ctx)).toBe("universalProvider.title");
    expect(VIEW_TITLES.prompts(t, ctx)).toBe("prompts.title");
  });

  it("goes back from skills discovery to skills, otherwise home", () => {
    expect(getBackView("skillsDiscovery")).toBe("skills");
    for (const view of VALID_VIEWS) {
      if (view !== "skillsDiscovery") {
        expect(getBackView(view)).toBe("providers");
      }
    }
  });

  it("restores only known views from storage", () => {
    expect(isView("mcp")).toBe(true);
    expect(isView("nope")).toBe(false);
    expect(isView(undefined)).toBe(false);

    window.localStorage.setItem(VIEW_STORAGE_KEY, "openclawTools");
    expect(getInitialView()).toBe("openclawTools");
    window.localStorage.setItem(VIEW_STORAGE_KEY, "bogus-view");
    expect(getInitialView()).toBe("providers");
  });
});

describe("app feature nav", () => {
  it("uses app-specific groups for hermes, openclaw and grokbuild", () => {
    expect(getFeatureNav("hermes").key).toBe("hermes");
    expect(getFeatureNav("openclaw").key).toBe("openclaw");
    expect(getFeatureNav("grokbuild").key).toBe("grokbuild");
    expect(getFeatureNav("grokbuild").items).toBe(
      getFeatureNav("claude").items,
    );
    for (const app of [
      "claude",
      "claude-desktop",
      "codex",
      "gemini",
    ] as const) {
      expect(getFeatureNav(app).key).toBe("default");
    }
    expect(getFeatureNav("hermes").items.map((i) => i.target)).toEqual([
      "skills",
      "hermesMemory",
      "hermesWebUI",
      "mcp",
    ]);
    expect(getFeatureNav("openclaw").items.map((i) => i.target)).toEqual([
      "workspace",
      "openclawEnv",
      "openclawTools",
      "openclawAgents",
      "sessions",
    ]);
  });

  it("only targets real views (or the hermes web ui action)", () => {
    for (const app of ALL_APP_IDS) {
      for (const item of getFeatureNav(app).items) {
        expect(item.target === "hermesWebUI" || isView(item.target)).toBe(true);
      }
    }
  });

  it("collapses capability-gated entries and navigates on click", () => {
    const onNavigate = vi.fn();
    render(
      <AppFeatureNav
        activeApp="claude-desktop"
        caps={{
          sessions: false,
          skills: false,
          mcp: true,
          additive: false,
          proxyTakeover: false,
          healthCheck: false,
          setDefault: false,
          defaultIsSwitch: false,
          defaultModel: false,
          universalProviders: false,
          omo: false,
          configStatus: false,
          pluginSync: false,
          providerTerminal: false,
          commonConfigSnippet: false,
        }}
        onNavigate={onNavigate}
      />,
    );

    expect(screen.getByTitle("skills.manage").className).toContain("w-0");
    expect(screen.getByTitle("sessionManager.title").className).toContain(
      "w-0",
    );
    expect(screen.getByTitle("prompts.manage").className).not.toContain("w-0");

    fireEvent.click(screen.getByTitle("mcp.title"));
    expect(onNavigate).toHaveBeenCalledWith("mcp");
  });
});

describe("AppHeader", () => {
  it.each<[View, View, string]>([
    ["agents", "providers", "agents.title"],
    ["skillsDiscovery", "skills", "skills.title"],
    ["openclawEnv", "providers", "openclaw.env.title"],
  ])("shows the %s title and goes back to %s", (view, back, title) => {
    const props = renderHeader({ currentView: view });

    expect(screen.getByRole("heading").textContent).toBe(title);
    fireEvent.click(screen.getByRole("button", { name: "common.back" }));
    expect(props.onNavigate).toHaveBeenCalledWith(back);
    expect(screen.queryByTestId("add-provider")).not.toBeInTheDocument();
  });

  it("routes the hermes web ui entry to its action instead of a view", () => {
    const props = renderHeader({
      activeApp: "hermes",
      activeAppCaps: appCaps("hermes"),
      sharedFeatureApp: "hermes",
      sharedFeatureCaps: appCaps("hermes"),
    });

    fireEvent.click(screen.getByTitle("hermes.webui.open"));
    expect(props.onOpenHermesWebUI).toHaveBeenCalledTimes(1);
    expect(props.onNavigate).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTitle("hermes.memory.title"));
    expect(props.onNavigate).toHaveBeenCalledWith("hermesMemory");
  });

  it("opens settings tabs from the home header", () => {
    const props = renderHeader({ isTakeoverActive: true });

    fireEvent.click(screen.getByTitle("common.settings"));
    expect(props.onOpenSettings).toHaveBeenCalledWith("general");
    fireEvent.click(screen.getByText("update-badge"));
    expect(props.onOpenSettings).toHaveBeenCalledWith("about");
    fireEvent.click(screen.getByTitle("usage.title"));
    expect(props.onOpenSettings).toHaveBeenCalledWith("usage");
    fireEvent.click(screen.getByTestId("add-provider"));
    expect(props.onAddProvider).toHaveBeenCalledTimes(1);
  });
});
