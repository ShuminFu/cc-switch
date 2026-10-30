import { Suspense, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { ArrowLeft } from "lucide-react";
import { useSettingsQuery } from "@/lib/query";
import { appHas, getFirstVisibleApp, isAppVisible } from "@/config/apps";
import { useOpenClawHealth } from "@/hooks/useOpenClaw";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
import { isTextEditableTarget } from "@/utils/domUtils";
import {
  isWindows,
  isLinux,
  DRAG_REGION_ATTR,
  DRAG_REGION_STYLE,
} from "@/lib/platform";
import { Button } from "@/components/ui/button";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { FirstRunWizard } from "@/components/onboarding";
import OpenClawHealthBanner from "@/components/openclaw/OpenClawHealthBanner";
import { ErrorBoundary } from "@/app/ErrorBoundary";
import { EnvConflictBanner } from "@/app/EnvConflictBanner";
import {
  NavigationProvider,
  VIEW_PARENT,
  useNavigation,
  type ViewId,
} from "@/app/navigation";
import {
  NO_DRAG_STYLE,
  PageHeaderSlotsProvider,
  type PageHeaderSlots,
} from "@/app/PageHeader";
import { ROUTES } from "@/app/routes";
import { TitleBar } from "@/app/TitleBar";
import { useAppEvents } from "@/app/useAppEvents";

const DEFAULT_DRAG_BAR_HEIGHT = isWindows() || isLinux() ? 0 : 28; // px
const HEADER_HEIGHT = 64; // px

/**
 * Keyboard navigation: Cmd/Ctrl+, opens settings, Escape goes to the parent
 * page unless a dialog or a text field has focus.
 */
function useShellShortcuts() {
  const { view, navigate, goBack } = useNavigation();
  const viewRef = useRef<ViewId>(view);
  viewRef.current = view;

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "," && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        navigate("settings");
        return;
      }

      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (document.body.style.overflow === "hidden") return;
      if (VIEW_PARENT[viewRef.current] === null) return;
      if (isTextEditableTarget(event.target)) return;

      event.preventDefault();
      goBack();
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [navigate, goBack]);
}

/**
 * App shell: title bar, header chrome with page-owned slots, and the current
 * page. Navigation state lives in `NavigationProvider`, pages in
 * `src/routes/` and the route table in `src/app/routes.tsx`.
 */
function AppShell() {
  const { t } = useTranslation();
  const { view, activeApp, featureApp, goBack, setActiveApp } = useNavigation();
  const route = ROUTES[view];
  const Page = route.Component;
  const isRootPage = VIEW_PARENT[view] === null;

  const { data: settingsData } = useSettingsQuery();
  const useAppWindowControls =
    isLinux() && (settingsData?.useAppWindowControls ?? false);
  const dragBarHeight = useAppWindowControls ? 32 : DEFAULT_DRAG_BAR_HEIGHT;
  const contentTopOffset = dragBarHeight + HEADER_HEIGHT;
  const visibleApps = settingsData?.visibleApps;

  useEffect(() => {
    if (!isAppVisible(visibleApps, activeApp)) {
      setActiveApp(getFirstVisibleApp(visibleApps));
    }
  }, [visibleApps, activeApp, setActiveApp]);

  useAppEvents();
  useUsageCacheBridge();
  useShellShortcuts();

  const showHealthBanner =
    appHas(activeApp, "healthBanner") && route.appHealthBanner === true;
  const { data: healthWarnings = [] } = useOpenClawHealth(showHealthBanner);

  const [slots, setSlots] = useState<PageHeaderSlots>({
    start: null,
    actions: null,
  });
  // Stable ref callbacks: a new function each render would make React detach
  // and re-attach the ref, resetting the slots in a render loop.
  const setStartSlot = useCallback(
    (el: HTMLDivElement | null) =>
      setSlots((prev) => (prev.start === el ? prev : { ...prev, start: el })),
    [],
  );
  const setActionsSlot = useCallback(
    (el: HTMLDivElement | null) =>
      setSlots((prev) =>
        prev.actions === el ? prev : { ...prev, actions: el },
      ),
    [],
  );

  return (
    <div
      className="flex flex-col h-screen overflow-hidden bg-background text-foreground selection:bg-primary/30 pb-4"
      style={{ overflowX: "hidden", paddingTop: contentTopOffset }}
    >
      <TitleBar
        height={dragBarHeight}
        showWindowControls={useAppWindowControls}
        settingsLoaded={Boolean(settingsData)}
      />
      <EnvConflictBanner activeApp={activeApp} />

      <PageHeaderSlotsProvider value={slots}>
        <header
          className="fixed z-50 w-full transition-all duration-300 bg-background/80 backdrop-blur-md"
          {...DRAG_REGION_ATTR}
          style={{
            ...DRAG_REGION_STYLE,
            top: dragBarHeight,
            height: HEADER_HEIGHT,
          }}
        >
          <div
            className="flex h-full items-center justify-between gap-2 px-6"
            {...DRAG_REGION_ATTR}
            style={{ ...DRAG_REGION_STYLE }}
          >
            <div className="flex items-center gap-1" style={NO_DRAG_STYLE}>
              {!isRootPage && (
                <div className="flex items-center gap-2">
                  <Button
                    variant="outline"
                    size="icon"
                    onClick={goBack}
                    className="mr-2 rounded-lg"
                    aria-label={t("common.back", { defaultValue: "Back" })}
                  >
                    <ArrowLeft className="w-4 h-4" />
                  </Button>
                  <h1 className="text-lg font-semibold">
                    {route.title?.(t, { featureApp })}
                  </h1>
                </div>
              )}
              {/* Filled by the root page via <PageHeaderStart>. */}
              <div ref={setStartSlot} className="contents" />
            </div>

            {/* Filled by the current page via <PageHeaderActions>. */}
            <div
              ref={setActionsSlot}
              className="flex flex-1 min-w-0 items-center justify-end gap-1.5"
            />
          </div>
        </header>

        <main className="flex-1 min-h-0 flex flex-col overflow-y-auto animate-fade-in">
          {showHealthBanner && healthWarnings.length > 0 && (
            <OpenClawHealthBanner warnings={healthWarnings} />
          )}
          <AnimatePresence mode="wait">
            <motion.div
              key={view}
              className="flex-1 min-h-0"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.2 }}
            >
              <ErrorBoundary resetKey={view} variant="page">
                <Suspense fallback={null}>
                  <Page />
                </Suspense>
              </ErrorBoundary>
            </motion.div>
          </AnimatePresence>
        </main>
      </PageHeaderSlotsProvider>

      <DeepLinkImportDialog />
      <FirstRunWizard />
    </div>
  );
}

function App() {
  return (
    <NavigationProvider>
      <AppShell />
    </NavigationProvider>
  );
}

export default App;
