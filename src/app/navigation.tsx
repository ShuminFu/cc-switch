import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import type { AppId } from "@/lib/api/types";
import { APP_REGISTRY, isAppId } from "@/config/apps";

/**
 * Typed view state for the app shell (FE-ARCH-02).
 *
 * The shell is a single window with one page at a time, so a tiny typed
 * router is enough: the current page (`view`), its parameters, and the
 * active app. All navigation goes through `navigate` / `goBack`, and the
 * state is persisted under the same localStorage keys the previous shell
 * used, so users land on the page they left.
 */

export const VIEW_IDS = [
  "providers",
  "settings",
  "prompts",
  "skills",
  "skillsDiscovery",
  "mcp",
  "agents",
  "universal",
  "sessions",
  "workspace",
  "openclawEnv",
  "openclawTools",
  "openclawAgents",
  "hermesMemory",
] as const;

export type ViewId = (typeof VIEW_IDS)[number];

/** Per-view parameters. Views without parameters map to `undefined`. */
export interface ViewParams {
  settings: { tab?: string } | undefined;
}

export type ParamsOf<V extends ViewId> = V extends keyof ViewParams
  ? ViewParams[V]
  : undefined;

export interface NavigationState {
  view: ViewId;
  params: { tab?: string } | undefined;
}

/** Parent page for the back button / Escape; `null` for the root page. */
export const VIEW_PARENT: Record<ViewId, ViewId | null> = {
  providers: null,
  settings: "providers",
  prompts: "providers",
  skills: "providers",
  skillsDiscovery: "skills",
  mcp: "providers",
  agents: "providers",
  universal: "providers",
  sessions: "providers",
  workspace: "providers",
  openclawEnv: "providers",
  openclawTools: "providers",
  openclawAgents: "providers",
  hermesMemory: "providers",
};

export const APP_STORAGE_KEY = "cc-switch-last-app";
export const VIEW_STORAGE_KEY = "cc-switch-last-view";

export const isViewId = (value: unknown): value is ViewId =>
  typeof value === "string" && (VIEW_IDS as readonly string[]).includes(value);

const readStorage = (key: string): string | null => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};

const writeStorage = (key: string, value: string) => {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage can be unavailable (private mode, blocked site data).
  }
};

export const getInitialApp = (): AppId => {
  const saved = readStorage(APP_STORAGE_KEY);
  return isAppId(saved) ? saved : "claude";
};

export const getInitialView = (): ViewId => {
  const saved = readStorage(VIEW_STORAGE_KEY);
  return isViewId(saved) ? saved : "providers";
};

export interface NavigationContextValue {
  view: ViewId;
  params: NavigationState["params"];
  /** The app selected in the app switcher. */
  activeApp: AppId;
  /** App whose shared features (skills, prompts, MCP, sessions) are shown. */
  featureApp: AppId;
  navigate: <V extends ViewId>(view: V, params?: ParamsOf<V>) => void;
  /** Go to the parent page (no-op on the root page). */
  goBack: () => void;
  setActiveApp: (app: AppId) => void;
}

const NavigationContext = createContext<NavigationContextValue | null>(null);

interface NavigationProviderProps {
  children: ReactNode;
  initialView?: ViewId;
  initialApp?: AppId;
}

export function NavigationProvider({
  children,
  initialView,
  initialApp,
}: NavigationProviderProps) {
  const [state, setState] = useState<NavigationState>(() => ({
    view: initialView ?? getInitialView(),
    params: undefined,
  }));
  const [activeApp, setActiveAppState] = useState<AppId>(
    () => initialApp ?? getInitialApp(),
  );

  useEffect(() => {
    writeStorage(VIEW_STORAGE_KEY, state.view);
  }, [state.view]);

  useEffect(() => {
    writeStorage(APP_STORAGE_KEY, activeApp);
  }, [activeApp]);

  const navigate = useCallback(
    <V extends ViewId>(view: V, params?: ParamsOf<V>) => {
      setState({ view, params: params as NavigationState["params"] });
    },
    [],
  );

  const goBack = useCallback(() => {
    setState((current) => {
      const parent = VIEW_PARENT[current.view];
      return parent ? { view: parent, params: undefined } : current;
    });
  }, []);

  const setActiveApp = useCallback((app: AppId) => {
    setActiveAppState(app);
  }, []);

  const value = useMemo<NavigationContextValue>(
    () => ({
      view: state.view,
      params: state.params,
      activeApp,
      featureApp: APP_REGISTRY[activeApp].featureApp,
      navigate,
      goBack,
      setActiveApp,
    }),
    [state, activeApp, navigate, goBack, setActiveApp],
  );

  return (
    <NavigationContext.Provider value={value}>
      {children}
    </NavigationContext.Provider>
  );
}

export function useNavigation(): NavigationContextValue {
  const value = useContext(NavigationContext);
  if (!value) {
    throw new Error("useNavigation must be used inside <NavigationProvider>");
  }
  return value;
}
