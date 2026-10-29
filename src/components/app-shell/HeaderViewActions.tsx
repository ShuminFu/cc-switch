import { useTranslation } from "react-i18next";
import { Download, FolderArchive, History, Plus, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  getSkillsPageHeaderActions,
  type SkillsPageSource,
} from "@/components/skills/SkillsPage";
import { useScanUnmanagedSkills } from "@/hooks/useSkills";
import type { ShellPanelRefs } from "./panelRefs";
import type { View } from "./views";

const ACTION_BUTTON_CLASS = "hover:bg-black/5 dark:hover:bg-white/5";

/**
 * 子视图（Prompts / MCP / Skills / Skills 发现）在顶栏右侧的操作按钮。
 */
export function HeaderViewActions({
  view,
  panelRefs,
  skillsDiscoverySource,
  onOpenSkillsDiscovery,
}: {
  view: View;
  panelRefs: ShellPanelRefs;
  skillsDiscoverySource: SkillsPageSource;
  onOpenSkillsDiscovery: () => void;
}) {
  const { t } = useTranslation();
  // 订阅未管理 Skill 的共享缓存（实际扫描由 UnifiedSkillsPanel 进入页面时触发）。
  // 这里 enabled 默认 false，仅用于「导入」按钮的绿点提示，不主动发起扫描。
  const { data: unmanagedSkills } = useScanUnmanagedSkills();
  const hasUnmanagedSkills = (unmanagedSkills?.length ?? 0) > 0;

  switch (view) {
    case "prompts":
      return (
        <Button
          variant="ghost"
          size="sm"
          onClick={() => panelRefs.prompts.current?.openAdd()}
          className={ACTION_BUTTON_CLASS}
        >
          <Plus className="w-4 h-4 mr-2" />
          {t("prompts.add")}
        </Button>
      );
    case "mcp":
      return (
        <>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => panelRefs.mcp.current?.openImport()}
            className={ACTION_BUTTON_CLASS}
          >
            <Download className="w-4 h-4 mr-2" />
            {t("mcp.importExisting")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => panelRefs.mcp.current?.openAdd()}
            className={ACTION_BUTTON_CLASS}
          >
            <Plus className="w-4 h-4 mr-2" />
            {t("mcp.addMcp")}
          </Button>
        </>
      );
    case "skills":
      return (
        <>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => panelRefs.skills.current?.openRestoreFromBackup()}
            className={ACTION_BUTTON_CLASS}
          >
            <History className="w-4 h-4 mr-2" />
            {t("skills.restoreFromBackup.button")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => panelRefs.skills.current?.openInstallFromZip()}
            className={ACTION_BUTTON_CLASS}
          >
            <FolderArchive className="w-4 h-4 mr-2" />
            {t("skills.installFromZip.button")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => panelRefs.skills.current?.openImport()}
            className={`relative ${ACTION_BUTTON_CLASS}`}
            title={
              hasUnmanagedSkills ? t("skills.unmanagedAvailable") : undefined
            }
          >
            <Download className="w-4 h-4 mr-2" />
            {t("skills.import")}
            {hasUnmanagedSkills && (
              <span
                className="absolute top-1 right-1 h-2 w-2 rounded-full bg-green-500"
                aria-hidden="true"
              />
            )}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={onOpenSkillsDiscovery}
            className={ACTION_BUTTON_CLASS}
          >
            <Search className="w-4 h-4 mr-2" />
            {t("skills.discover")}
          </Button>
        </>
      );
    case "skillsDiscovery":
      return (
        <>
          {getSkillsPageHeaderActions(skillsDiscoverySource).map(
            ({ key, labelKey, Icon, execute }) => (
              <Button
                key={key}
                variant="ghost"
                size="sm"
                onClick={() => execute(panelRefs.skillsDiscovery.current)}
                className={ACTION_BUTTON_CLASS}
              >
                <Icon className="w-4 h-4 mr-2" />
                {t(labelKey)}
              </Button>
            ),
          )}
        </>
      );
    default:
      return null;
  }
}
