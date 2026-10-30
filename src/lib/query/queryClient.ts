import { QueryClient } from "@tanstack/react-query";

/**
 * Data older than this is refetched on mount / window focus. Most data only
 * changes through this app's own mutations (which invalidate) or backend
 * events, so refetching everything on every focus of a tray-resident window
 * is wasted IPC (FE-ARCH-25). Edits made outside the app are still picked up
 * on the next focus after this window. Live state (proxy status) opts back
 * into `staleTime: 0`.
 */
export const DEFAULT_STALE_TIME_MS = 30_000;

export const createAppQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: {
        retry: 1,
        refetchOnWindowFocus: true,
        staleTime: DEFAULT_STALE_TIME_MS,
      },
      mutations: {
        retry: false,
      },
    },
  });

export const queryClient = createAppQueryClient();
