import { useSettingsQuery } from "@/lib/query/queries";

const DEFAULT_OPENCLAW_DIR = "~/.openclaw";

/**
 * Display path of the OpenClaw workspace (or a sub-directory of it). Follows
 * the `openclawConfigDir` override from Settings instead of a hard-coded
 * `~/.openclaw` (FE-UI-35); the backend resolves the same directory.
 */
export function useWorkspacePath(subdir?: string): string {
  const { data: settings } = useSettingsQuery();
  const base = (
    settings?.openclawConfigDir?.trim() || DEFAULT_OPENCLAW_DIR
  ).replace(/[\\/]+$/, "");
  const separator = base.includes("\\") && !base.includes("/") ? "\\" : "/";
  const parts = [base, "workspace", ...(subdir ? [subdir] : [])];
  return parts.join(separator) + separator;
}
