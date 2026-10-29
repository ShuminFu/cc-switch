import { useRef, type RefObject } from "react";
import type { PromptPanelHandle } from "@/components/prompts/PromptPanel";
import type { UnifiedMcpPanelHandle } from "@/components/mcp/UnifiedMcpPanel";
import type { SkillsPageHandle } from "@/components/skills/SkillsPage";
import type { UnifiedSkillsPanelHandle } from "@/components/skills/UnifiedSkillsPanel";

/** 顶栏操作按钮需要调用的各视图面板句柄 */
export interface ShellPanelRefs {
  prompts: RefObject<PromptPanelHandle>;
  mcp: RefObject<UnifiedMcpPanelHandle>;
  skills: RefObject<UnifiedSkillsPanelHandle>;
  skillsDiscovery: RefObject<SkillsPageHandle>;
}

export function useShellPanelRefs(): ShellPanelRefs {
  return {
    prompts: useRef<PromptPanelHandle>(null),
    mcp: useRef<UnifiedMcpPanelHandle>(null),
    skills: useRef<UnifiedSkillsPanelHandle>(null),
    skillsDiscovery: useRef<SkillsPageHandle>(null),
  };
}
