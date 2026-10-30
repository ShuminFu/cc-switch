import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import {
  NavigationProvider,
  VIEW_IDS,
  getInitialApp,
  getInitialView,
  useNavigation,
} from "@/app/navigation";
import { ROUTES } from "@/app/routes";

const wrapper = ({ children }: { children: ReactNode }) => (
  <NavigationProvider>{children}</NavigationProvider>
);

describe("navigation", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("ignores unknown persisted values", () => {
    localStorage.setItem("cc-switch-last-app", "not-an-app");
    localStorage.setItem("cc-switch-last-view", "nowhere");
    expect(getInitialApp()).toBe("claude");
    expect(getInitialView()).toBe("providers");
  });

  it("has a route for every view", () => {
    for (const view of VIEW_IDS) {
      expect(ROUTES[view]?.Component).toBeDefined();
    }
  });

  it("navigates with params, goes back to the parent page and persists", () => {
    const { result } = renderHook(() => useNavigation(), { wrapper });
    expect(result.current.view).toBe("providers");

    act(() => result.current.navigate("settings", { tab: "usage" }));
    expect(result.current.view).toBe("settings");
    expect(result.current.params).toEqual({ tab: "usage" });
    expect(localStorage.getItem("cc-switch-last-view")).toBe("settings");

    act(() => result.current.navigate("skillsDiscovery"));
    act(() => result.current.goBack());
    expect(result.current.view).toBe("skills");
    act(() => result.current.goBack());
    expect(result.current.view).toBe("providers");
    act(() => result.current.goBack());
    expect(result.current.view).toBe("providers");
  });

  it("derives the shared-feature app from the registry", () => {
    const { result } = renderHook(() => useNavigation(), { wrapper });
    act(() => result.current.setActiveApp("claude-desktop"));
    expect(result.current.activeApp).toBe("claude-desktop");
    expect(result.current.featureApp).toBe("claude");
    expect(localStorage.getItem("cc-switch-last-app")).toBe("claude-desktop");
  });
});
