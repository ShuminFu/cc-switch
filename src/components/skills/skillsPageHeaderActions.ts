import { RefreshCw, Settings, type LucideIcon } from "lucide-react";

// 头部操作与类型独立成轻量模块：应用头部常驻引用，而 SkillsPage 本体按需加载

export type SkillsPageSource = "repos" | "skillssh";

export interface SkillsPageHandle {
  refresh: () => void;
  openRepoManager: () => void;
}

type SkillsPageHeaderAction = {
  key: string;
  sources: readonly SkillsPageSource[];
  labelKey: string;
  Icon: LucideIcon;
  execute: (page: SkillsPageHandle | null) => void;
};

const SKILLS_PAGE_HEADER_ACTIONS: readonly SkillsPageHeaderAction[] = [
  {
    key: "refresh-repos",
    sources: ["repos"],
    labelKey: "skills.refresh",
    Icon: RefreshCw,
    execute: (page) => page?.refresh(),
  },
  {
    key: "manage-repos",
    sources: ["repos", "skillssh"],
    labelKey: "skills.repoManager",
    Icon: Settings,
    execute: (page) => page?.openRepoManager(),
  },
];

export const getSkillsPageHeaderActions = (source: SkillsPageSource) =>
  SKILLS_PAGE_HEADER_ACTIONS.filter((action) =>
    action.sources.includes(source),
  );
