import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { switchRulesApi, type SwitchRuleDraft } from "@/lib/api/switchRules";

export const switchRuleKeys = {
  all: ["switchRules"] as const,
  list: (appType?: string) =>
    [...switchRuleKeys.all, "list", appType ?? "all"] as const,
  states: () => [...switchRuleKeys.all, "states"] as const,
};

export function useSwitchRules(appType?: string) {
  return useQuery({
    queryKey: switchRuleKeys.list(appType),
    queryFn: () => switchRulesApi.list(appType),
    staleTime: 10 * 1000,
  });
}

export function useSwitchRuleStates(options?: { refetchInterval?: number }) {
  return useQuery({
    queryKey: switchRuleKeys.states(),
    queryFn: () => switchRulesApi.getStates(),
    refetchInterval: options?.refetchInterval ?? 30 * 1000,
    retry: false,
  });
}

function useInvalidateSwitchRules() {
  const queryClient = useQueryClient();
  return () => queryClient.invalidateQueries({ queryKey: switchRuleKeys.all });
}

export function useUpsertSwitchRule() {
  const invalidate = useInvalidateSwitchRules();
  return useMutation({
    mutationFn: (rule: SwitchRuleDraft) => switchRulesApi.upsert(rule),
    onSuccess: () => void invalidate(),
  });
}

export function useDeleteSwitchRule() {
  const invalidate = useInvalidateSwitchRules();
  return useMutation({
    mutationFn: (id: string) => switchRulesApi.delete(id),
    onSuccess: () => void invalidate(),
  });
}

export function useSetSwitchRuleEnabled() {
  const invalidate = useInvalidateSwitchRules();
  return useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      switchRulesApi.setEnabled(id, enabled),
    onSuccess: () => void invalidate(),
  });
}

export function useCancelSwitchRuleRevert() {
  const invalidate = useInvalidateSwitchRules();
  return useMutation({
    mutationFn: (ruleId: string) => switchRulesApi.cancelRevert(ruleId),
    onSuccess: () => void invalidate(),
  });
}

export function useEvaluateSwitchRulesNow() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (appType: string) => switchRulesApi.evaluateNow(appType),
    onSuccess: (_fired, appType) => {
      void queryClient.invalidateQueries({ queryKey: switchRuleKeys.all });
      void queryClient.invalidateQueries({ queryKey: ["providers", appType] });
    },
  });
}
