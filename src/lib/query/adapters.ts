import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { adaptersApi } from "@/lib/api/adapters";
import type { AppId } from "@/lib/api/types";
import { queryKeys } from "./keys";

/** Each tool's resolved config dir and where it came from (CFG-12). */
export const useAppConfigDirs = (enabled = true) =>
  useQuery({
    queryKey: queryKeys.adapters.configDirs,
    queryFn: () => adaptersApi.getConfigDirs(),
    enabled,
    refetchOnMount: "always",
  });

/** MCP apps whose config dir is missing, so MCP sync skipped them (CFG-46). */
export const useMcpSyncSkips = () =>
  useQuery({
    queryKey: queryKeys.mcp.syncSkips,
    queryFn: () => adaptersApi.getMcpSyncSkips(),
  });

/** Providers in an app's live config that can be imported (CFG-23). */
export const useImportCandidates = (app: AppId, enabled: boolean) =>
  useQuery({
    queryKey: queryKeys.adapters.importCandidates(app),
    queryFn: () => adaptersApi.getImportCandidates(app),
    enabled,
    staleTime: 0,
    // Candidates carry credentials: do not keep them cached after the picker closes.
    gcTime: 0,
  });

/** Disk used by the Codex history migration backups (CFG-44). */
export const useCodexHistoryBackupUsage = () =>
  useQuery({
    queryKey: queryKeys.adapters.codexHistoryBackupUsage,
    queryFn: () => adaptersApi.getCodexHistoryBackupUsage(),
  });

export const usePurgeCodexHistoryBackups = () => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (includeRestoreLedger: boolean) =>
      adaptersApi.purgeCodexHistoryBackups(includeRestoreLedger),
    onSuccess: (usage) => {
      queryClient.setQueryData(
        queryKeys.adapters.codexHistoryBackupUsage,
        usage,
      );
    },
  });
};

/** Whether Claude Desktop still routes through the local proxy (PROXY-30). */
export const useClaudeDesktopProxyDependency = (enabled: boolean) =>
  useQuery({
    queryKey: queryKeys.adapters.claudeDesktopProxyDependency,
    queryFn: () => adaptersApi.getClaudeDesktopProxyDependency(),
    enabled,
  });
