import type { AppTypeFilter, UsageRangeSelection } from "@/types/usage";

/**
 * Usage dashboard view state that survives the dashboard unmounting
 * (FE-UI-31): the dashboard is a Settings tab, and Radix unmounts inactive
 * tabs, which used to reset a carefully chosen range and filters. Kept in
 * memory for the app session only.
 */
export interface UsageDashboardViewState {
  range: UsageRangeSelection;
  appType: AppTypeFilter;
  providerName?: string;
  model?: string;
  tab: "logs" | "providers" | "models";
}

export const DEFAULT_DASHBOARD_VIEW_STATE: UsageDashboardViewState = {
  range: { preset: "today" },
  appType: "all",
  tab: "logs",
};

let current: UsageDashboardViewState = DEFAULT_DASHBOARD_VIEW_STATE;

export function getDashboardViewState(): UsageDashboardViewState {
  return current;
}

export function saveDashboardViewState(
  patch: Partial<UsageDashboardViewState>,
): void {
  current = { ...current, ...patch };
}

/** Test helper. */
export function resetDashboardViewState(): void {
  current = DEFAULT_DASHBOARD_VIEW_STATE;
}
