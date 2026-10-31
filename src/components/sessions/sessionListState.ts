/**
 * List state helpers for the session manager (SESS-25): page sizes, the
 * persisted view mode and group expansion, and small set utilities.
 */

/** Rows mounted per "show more" step in the flat list (PERF-46). */
export const SESSION_LIST_PAGE_SIZE = 100;
/** Rows mounted per directory group before "show more" (PERF-46). */
export const SESSION_DIRECTORY_PAGE_SIZE = 50;

export const SESSION_LIST_VIEW_MODE_STORAGE_KEY =
  "cc-switch.sessionManager.listViewMode";
export const SESSION_GROUP_EXPANSION_STORAGE_KEY =
  "cc-switch.sessionManager.groupExpansionState";

export type ProviderFilter =
  | "all"
  | "codex"
  | "grokbuild"
  | "claude"
  | "opencode"
  | "openclaw"
  | "gemini"
  | "hermes";

export type SessionListViewMode = "flat" | "grouped";

export type GroupSelectionState = {
  checked: boolean | "indeterminate";
  isSelected: boolean;
  selectedCount: number;
  selectableCount: number;
};

export type SessionGroupExpansionState = {
  expandedProviderIds: Set<string>;
  expandedDirectoryKeys: Set<string>;
};

export const readInitialSessionListViewMode = (): SessionListViewMode => {
  if (typeof window === "undefined") return "flat";
  const stored = window.localStorage.getItem(
    SESSION_LIST_VIEW_MODE_STORAGE_KEY,
  );
  return stored === "grouped" || stored === "flat" ? stored : "flat";
};

export const readInitialSessionGroupExpansionState =
  (): SessionGroupExpansionState => {
    if (typeof window === "undefined") {
      return {
        expandedProviderIds: new Set(),
        expandedDirectoryKeys: new Set(),
      };
    }

    try {
      const stored = window.localStorage.getItem(
        SESSION_GROUP_EXPANSION_STORAGE_KEY,
      );
      const parsed = stored ? JSON.parse(stored) : null;

      if (!parsed || typeof parsed !== "object") {
        return {
          expandedProviderIds: new Set(),
          expandedDirectoryKeys: new Set(),
        };
      }

      const expandedProviderIds = Array.isArray(parsed.expandedProviderIds)
        ? parsed.expandedProviderIds.filter(
            (providerId: unknown): providerId is string =>
              typeof providerId === "string",
          )
        : [];
      const expandedDirectoryKeys = Array.isArray(parsed.expandedDirectoryKeys)
        ? parsed.expandedDirectoryKeys.filter(
            (directoryKey: unknown): directoryKey is string =>
              typeof directoryKey === "string",
          )
        : [];

      return {
        expandedProviderIds: new Set(expandedProviderIds),
        expandedDirectoryKeys: new Set(expandedDirectoryKeys),
      };
    } catch {
      return {
        expandedProviderIds: new Set(),
        expandedDirectoryKeys: new Set(),
      };
    }
  };

export const serializeSessionGroupExpansionState = (
  expandedProviderGroups: Set<string>,
  expandedDirectoryGroups: Set<string>,
) =>
  JSON.stringify({
    expandedProviderIds: Array.from(expandedProviderGroups).sort(),
    expandedDirectoryKeys: Array.from(expandedDirectoryGroups).sort(),
  });

export const filterSetToAllowedValues = (
  current: Set<string>,
  allowedValues: Set<string>,
) => {
  let changed = false;
  const next = new Set<string>();

  current.forEach((value) => {
    if (allowedValues.has(value)) {
      next.add(value);
    } else {
      changed = true;
    }
  });

  return changed ? next : current;
};
