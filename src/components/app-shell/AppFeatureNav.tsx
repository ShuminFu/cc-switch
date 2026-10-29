import type { ComponentType } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import {
  Book,
  Brain,
  Cpu,
  FolderOpen,
  History,
  KeyRound,
  LayoutDashboard,
  Shield,
  Wrench,
} from "lucide-react";
import type { AppId } from "@/lib/api";
import type { AppCaps } from "@/entities/app/registry";
import { McpIcon } from "@/components/BrandIcons";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { View } from "./views";

/** 导航项目标：切换到某个视图，或打开 Hermes Web UI */
export type FeatureNavTarget = View | "hermesWebUI";

export interface FeatureNavItem {
  target: FeatureNavTarget;
  titleKey: string;
  Icon: ComponentType<{ className?: string }>;
  /** 当前应用缺少该能力时折叠隐藏（带过渡动画） */
  requiresCap?: keyof Pick<AppCaps, "skills" | "sessions">;
}

const McpNavIcon = () => <McpIcon size={16} />;

const DEFAULT_FEATURE_NAV: readonly FeatureNavItem[] = [
  {
    target: "skills",
    titleKey: "skills.manage",
    Icon: Wrench,
    requiresCap: "skills",
  },
  { target: "prompts", titleKey: "prompts.manage", Icon: Book },
  {
    target: "sessions",
    titleKey: "sessionManager.title",
    Icon: History,
    requiresCap: "sessions",
  },
  { target: "mcp", titleKey: "mcp.title", Icon: McpNavIcon },
];

/**
 * 应用专属的功能导航；未列出的应用使用 `DEFAULT_FEATURE_NAV`。
 * 列出的应用拥有独立的动画 key，切换时整组淡入淡出。
 */
export const APP_FEATURE_NAV: Partial<
  Record<AppId, readonly FeatureNavItem[]>
> = {
  hermes: [
    { target: "skills", titleKey: "skills.manage", Icon: Wrench },
    { target: "hermesMemory", titleKey: "hermes.memory.title", Icon: Brain },
    {
      target: "hermesWebUI",
      titleKey: "hermes.webui.open",
      Icon: LayoutDashboard,
    },
    { target: "mcp", titleKey: "mcp.title", Icon: McpNavIcon },
  ],
  openclaw: [
    { target: "workspace", titleKey: "workspace.manage", Icon: FolderOpen },
    { target: "openclawEnv", titleKey: "openclaw.env.title", Icon: KeyRound },
    {
      target: "openclawTools",
      titleKey: "openclaw.tools.title",
      Icon: Shield,
    },
    { target: "openclawAgents", titleKey: "openclaw.agents.title", Icon: Cpu },
    { target: "sessions", titleKey: "sessionManager.title", Icon: History },
  ],
  grokbuild: DEFAULT_FEATURE_NAV,
};

export function getFeatureNav(app: AppId): {
  key: string;
  items: readonly FeatureNavItem[];
} {
  const items = APP_FEATURE_NAV[app];
  return items
    ? { key: app, items }
    : { key: "default", items: DEFAULT_FEATURE_NAV };
}

const NAV_BUTTON_CLASS =
  "text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5";

export function AppFeatureNav({
  activeApp,
  caps,
  onNavigate,
}: {
  activeApp: AppId;
  /** 决定 `requiresCap` 项是否显示的能力集 */
  caps: AppCaps;
  onNavigate: (target: FeatureNavTarget) => void;
}) {
  const { t } = useTranslation();
  const { key, items } = getFeatureNav(activeApp);

  return (
    <div className="flex items-center gap-1 p-1 bg-muted rounded-xl">
      <AnimatePresence mode="wait">
        <motion.div
          key={key}
          className="flex items-center gap-1"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
        >
          {items.map(({ target, titleKey, Icon, requiresCap }) => (
            <Button
              key={target}
              variant="ghost"
              size="sm"
              onClick={() => onNavigate(target)}
              className={
                requiresCap
                  ? cn(
                      NAV_BUTTON_CLASS,
                      "transition-all duration-200 ease-in-out overflow-hidden",
                      caps[requiresCap]
                        ? "opacity-100 w-8 scale-100 px-2"
                        : "opacity-0 w-0 scale-75 pointer-events-none px-0 -ml-1",
                    )
                  : `${NAV_BUTTON_CLASS} w-8 px-2`
              }
              title={t(titleKey)}
            >
              <Icon
                className={requiresCap ? "flex-shrink-0 w-4 h-4" : "w-4 h-4"}
              />
            </Button>
          ))}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
