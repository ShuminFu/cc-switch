import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { Download, FolderArchive, History, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HeaderToolbar, PageHeaderActions } from "@/app/PageHeader";
import { useNavigation } from "@/app/navigation";
import { APP_REGISTRY } from "@/config/apps";
import { useScanUnmanagedSkills } from "@/hooks/useSkills";
import UnifiedSkillsPanel, {
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";

const HEADER_BUTTON_CLASS = "hover:bg-black/5 dark:hover:bg-white/5";

/** Installed skills page with backup/zip/import/discover header actions. */
export default function SkillsRoute() {
  const { t } = useTranslation();
  const { activeApp, navigate } = useNavigation();
  const panelRef = useRef<UnifiedSkillsPanelHandle>(null);
  // 订阅未管理 Skill 的共享缓存（实际扫描由 UnifiedSkillsPanel 进入页面时触发）。
  // 这里 enabled 默认 false，仅用于「导入」按钮的绿点提示，不主动发起扫描。
  const { data: unmanagedSkills } = useScanUnmanagedSkills();
  const hasUnmanagedSkills = (unmanagedSkills?.length ?? 0) > 0;
  const openDiscovery = () => navigate("skillsDiscovery");

  return (
    <>
      <PageHeaderActions>
        <HeaderToolbar>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => panelRef.current?.openRestoreFromBackup()}
            className={HEADER_BUTTON_CLASS}
          >
            <History className="w-4 h-4 mr-2" />
            {t("skills.restoreFromBackup.button")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => panelRef.current?.openInstallFromZip()}
            className={HEADER_BUTTON_CLASS}
          >
            <FolderArchive className="w-4 h-4 mr-2" />
            {t("skills.installFromZip.button")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => panelRef.current?.openImport()}
            className={`relative ${HEADER_BUTTON_CLASS}`}
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
            onClick={openDiscovery}
            className={HEADER_BUTTON_CLASS}
          >
            <Search className="w-4 h-4 mr-2" />
            {t("skills.discover")}
          </Button>
        </HeaderToolbar>
      </PageHeaderActions>
      <UnifiedSkillsPanel
        ref={panelRef}
        onOpenDiscovery={openDiscovery}
        currentApp={APP_REGISTRY[activeApp].skillsTargetApp}
      />
    </>
  );
}
