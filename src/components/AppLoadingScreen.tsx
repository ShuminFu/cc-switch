/**
 * Shown while the app bootstraps (locale chunk + backend init check), so the
 * window is never blank before the first real render. It cannot rely on i18n
 * or React Query, which are not ready yet, so it has no translated text.
 */
export function AppLoadingScreen() {
  return (
    <div
      className="flex h-screen w-screen items-center justify-center bg-background"
      role="status"
      aria-busy="true"
      aria-label="Loading"
      data-testid="app-loading-screen"
    >
      <div className="h-8 w-8 animate-spin rounded-full border-2 border-muted-foreground/30 border-t-primary" />
    </div>
  );
}
