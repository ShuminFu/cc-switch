import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import type { SessionMeta } from "@/types";
import { queryKeys } from "@/lib/query/keys";

export interface ProviderScanError {
  providerId: string;
  message: string;
}

export interface SessionScanReport {
  sessions: SessionMeta[];
  errors: ProviderScanError[];
  generation: number;
  changed: boolean;
  parsedFiles: number;
  /** True when the backend omitted `sessions` because `knownGeneration` matched. */
  unchanged?: boolean;
}

export interface SessionSearchHit {
  sessionKey: string;
  providerId: string;
  sessionId: string;
  snippet?: string;
}

export interface SessionSearchResponse {
  hits: SessionSearchHit[];
  /** False while transcripts are still being indexed; search again to continue. */
  complete: boolean;
}

export const sessionIndexApi = {
  scanReport(knownGeneration?: number): Promise<SessionScanReport> {
    return invoke("scan_sessions_report", { knownGeneration });
  },
  snapshot(): Promise<SessionMeta[]> {
    return invoke("get_session_index_snapshot");
  },
  search(query: string, limit = 200): Promise<SessionSearchResponse> {
    return invoke("search_sessions", { query, limit });
  },
};

/** Index metadata lives under the shared `["sessions"]` prefix; the list
 * itself is cached at `queryKeys.sessions.all` so the existing optimistic
 * delete updates keep working. */
export const sessionIndexKeys = {
  reportMeta: ["sessions", "indexReportMeta"] as const,
  snapshot: ["sessions", "indexSnapshot"] as const,
  search: (query: string) => ["sessions", "indexSearch", query] as const,
};

interface ReportMeta {
  generation: number;
  errors: ProviderScanError[];
}

/** How often the open Session Manager re-checks provider directories. The
 * backend only `stat`s unchanged files and omits the payload when nothing
 * changed, so this is cheap. */
export const SESSION_POLL_INTERVAL_MS = 2000;

function usePageVisible(): boolean {
  const [visible, setVisible] = useState(
    typeof document === "undefined" ? true : !document.hidden,
  );
  useEffect(() => {
    if (typeof document === "undefined") return;
    const onChange = () => setVisible(!document.hidden);
    document.addEventListener("visibilitychange", onChange);
    return () => document.removeEventListener("visibilitychange", onChange);
  }, []);
  return visible;
}

/**
 * Sessions served from the backend session index: the persisted snapshot
 * renders immediately on cold start, then incremental rescans keep the list
 * current (new sessions appear without clicking refresh).
 */
export function useIndexedSessions(options?: { poll?: boolean }) {
  const queryClient = useQueryClient();
  const visible = usePageVisible();
  const poll = options?.poll ?? true;

  const snapshotQuery = useQuery<SessionMeta[]>({
    queryKey: sessionIndexKeys.snapshot,
    queryFn: () => sessionIndexApi.snapshot(),
    staleTime: Infinity,
  });

  const listQuery = useQuery<SessionMeta[]>({
    queryKey: queryKeys.sessions.all,
    queryFn: async () => {
      const previousMeta = queryClient.getQueryData<ReportMeta>(
        sessionIndexKeys.reportMeta,
      );
      const previous = queryClient.getQueryData<SessionMeta[]>(
        queryKeys.sessions.all,
      );
      const report = await sessionIndexApi.scanReport(
        previous ? previousMeta?.generation : undefined,
      );
      queryClient.setQueryData<ReportMeta>(sessionIndexKeys.reportMeta, {
        generation: report.generation,
        errors: report.errors ?? [],
      });
      if (report.unchanged && previous) {
        return previous;
      }
      return report.sessions;
    },
    placeholderData: snapshotQuery.data?.length
      ? snapshotQuery.data
      : undefined,
    refetchInterval: poll && visible ? SESSION_POLL_INTERVAL_MS : false,
    refetchIntervalInBackground: false,
  });

  const metaQuery = useQuery<ReportMeta | null>({
    queryKey: sessionIndexKeys.reportMeta,
    queryFn: () =>
      queryClient.getQueryData<ReportMeta>(sessionIndexKeys.reportMeta) ?? null,
    staleTime: Infinity,
    enabled: false,
  });

  return {
    sessions: listQuery.data ?? [],
    errors: metaQuery.data?.errors ?? [],
    isLoading: listQuery.isLoading,
    error: listQuery.error,
    refetch: listQuery.refetch,
  };
}

/** Debounced transcript search across all providers. */
export function useSessionTranscriptSearch(query: string, enabled: boolean) {
  const [debounced, setDebounced] = useState(query.trim());
  useEffect(() => {
    const handle = setTimeout(() => setDebounced(query.trim()), 300);
    return () => clearTimeout(handle);
  }, [query]);

  return useQuery<SessionSearchResponse>({
    queryKey: sessionIndexKeys.search(debounced),
    queryFn: () => sessionIndexApi.search(debounced),
    enabled: enabled && debounced.length > 0,
    staleTime: 10_000,
    // Keep indexing in the background until every transcript is indexed.
    refetchInterval: (query) =>
      query.state.data && !query.state.data.complete ? 1500 : false,
  });
}
