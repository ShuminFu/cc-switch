import { isUpdateAvailable } from "@/lib/version";

/**
 * Pure install/update result logic extracted from AboutSection (FE-UI-19),
 * so the soft/hard failure classification and the batch summary can be
 * unit-tested without rendering the About tab.
 */

export const TOOL_NAMES = [
  "claude",
  "codex",
  "gemini",
  "grok",
  "opencode",
  "openclaw",
  "hermes",
] as const;
export type ToolName = (typeof TOOL_NAMES)[number];
export type ToolLifecycleAction = "install" | "update";

export const TOOL_DISPLAY_NAMES: Record<ToolName, string> = {
  claude: "Claude Code",
  codex: "Codex",
  gemini: "Gemini CLI",
  grok: "Grok Build",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
};

export interface RefreshedToolState {
  version: string | null;
  latest_version: string | null;
  error: string | null;
}

export type RunResult =
  | { kind: "success" }
  /** The command exited 0 but the version did not move although an update exists. */
  | { kind: "versionUnchanged"; version: string; latest: string | null }
  /** The command exited 0 but the tool still cannot report a version. */
  | { kind: "notRunnable"; detail: string | null };

/** Classify a lifecycle run whose command exited successfully. */
export function classifyRunResult(input: {
  action: ToolLifecycleAction;
  previousVersion: string | null;
  previousLatestVersion: string | null;
  refreshed: RefreshedToolState | undefined;
}): RunResult {
  const { action, previousVersion, previousLatestVersion, refreshed } = input;
  if (!refreshed?.version) {
    return { kind: "notRunnable", detail: refreshed?.error?.trim() || null };
  }
  const latest = refreshed.latest_version ?? previousLatestVersion;
  const versionUnchanged =
    action === "update" &&
    Boolean(previousVersion) &&
    refreshed.version === previousVersion &&
    isUpdateAvailable(refreshed.version, latest);
  return versionUnchanged
    ? { kind: "versionUnchanged", version: refreshed.version, latest }
    : { kind: "success" };
}

export interface ToolRunFailure {
  toolName: ToolName;
  detail: string;
  /** The command ran but the result needs attention (not a hard error). */
  soft: boolean;
  kind?: "notRunnable" | "versionUnchanged";
}

export type RunSummary =
  | { level: "success"; succeeded: number }
  | {
      level: "versionUnchanged" | "notRunnable" | "failed";
      description: string | undefined;
    }
  | {
      level: "partial";
      succeeded: number;
      failed: number;
      description: string | undefined;
    };

const lastLine = (text: string) => {
  const lines = text.trim().split("\n").filter(Boolean);
  return lines[lines.length - 1] ?? text;
};

/**
 * Summarise a (batch) run for the result toast. Batch descriptions keep the
 * last (most relevant) error line per tool; a single tool gets its full
 * detail.
 */
export function summarizeRun(
  failures: ToolRunFailure[],
  succeeded: number,
  isBatch: boolean,
): RunSummary {
  if (failures.length === 0) return { level: "success", succeeded };

  const description =
    (isBatch
      ? failures
          .map(
            (f) => `${TOOL_DISPLAY_NAMES[f.toolName]}: ${lastLine(f.detail)}`,
          )
          .join("\n")
      : failures[0]?.detail) || undefined;
  const hardFailures = failures.filter((f) => !f.soft);

  if (succeeded === 0 && hardFailures.length === 0) {
    const allVersionUnchanged = failures.every(
      (f) => f.kind === "versionUnchanged",
    );
    return {
      level: allVersionUnchanged ? "versionUnchanged" : "notRunnable",
      description,
    };
  }
  if (succeeded === 0) return { level: "failed", description };
  return {
    level: "partial",
    succeeded,
    failed: failures.length,
    description,
  };
}
