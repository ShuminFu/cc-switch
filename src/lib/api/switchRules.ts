import { invoke } from "@tauri-apps/api/core";

export type SwitchRuleSource = "subscription" | "coding_plan";

/** 配额规则：订阅 / Coding Plan 窗口越过阈值时自动切换供应商 */
export interface SwitchRule {
  id: string;
  appType: string;
  /** undefined = 该应用的官方订阅；否则为带 Coding Plan 用量脚本的供应商 */
  watchedProviderId?: string;
  source: SwitchRuleSource;
  tierName: string;
  thresholdPct: number;
  targetProviderId: string;
  revertOnReset: boolean;
  enabled: boolean;
  sortIndex: number;
  lastFiredAt?: number;
  createdAt: number;
}

export interface SwitchRuleState {
  ruleId: string;
  firedAt: number;
  resetsAt?: number;
  switchedFrom: string;
  switchedTo: string;
  lastUtilization: number;
  reason?: string;
}

export type SwitchRuleDraft = Omit<
  SwitchRule,
  "id" | "createdAt" | "sortIndex"
> &
  Partial<Pick<SwitchRule, "id" | "createdAt" | "sortIndex">>;

export const switchRulesApi = {
  list: async (appType?: string): Promise<SwitchRule[]> => {
    return invoke("list_switch_rules", { appType });
  },

  upsert: async (rule: SwitchRuleDraft): Promise<SwitchRule> => {
    return invoke("upsert_switch_rule", {
      rule: { id: "", createdAt: 0, sortIndex: 0, ...rule },
    });
  },

  delete: async (id: string): Promise<boolean> => {
    return invoke("delete_switch_rule", { id });
  },

  setEnabled: async (id: string, enabled: boolean): Promise<boolean> => {
    return invoke("set_switch_rule_enabled", { id, enabled });
  },

  getStates: async (): Promise<SwitchRuleState[]> => {
    return invoke("get_switch_rule_states");
  },

  cancelRevert: async (ruleId: string): Promise<boolean> => {
    return invoke("cancel_switch_rule_revert", { ruleId });
  },

  /** 立即用官方订阅额度评估该应用的规则；返回触发条数 */
  evaluateNow: async (appType: string): Promise<number> => {
    return invoke("evaluate_switch_rules_now", { appType });
  },
};
