import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { Download, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HeaderToolbar, PageHeaderActions } from "@/app/PageHeader";
import { useNavigation } from "@/app/navigation";
import UnifiedMcpPanel, {
  type UnifiedMcpPanelHandle,
} from "@/components/mcp/UnifiedMcpPanel";

const HEADER_BUTTON_CLASS = "hover:bg-black/5 dark:hover:bg-white/5";

/** Unified MCP page with import/add header actions. */
export default function McpRoute() {
  const { t } = useTranslation();
  const { goBack } = useNavigation();
  const panelRef = useRef<UnifiedMcpPanelHandle>(null);

  return (
    <>
      <PageHeaderActions>
        <HeaderToolbar>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => panelRef.current?.openImport()}
            className={HEADER_BUTTON_CLASS}
          >
            <Download className="w-4 h-4 mr-2" />
            {t("mcp.importExisting")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => panelRef.current?.openAdd()}
            className={HEADER_BUTTON_CLASS}
          >
            <Plus className="w-4 h-4 mr-2" />
            {t("mcp.addMcp")}
          </Button>
        </HeaderToolbar>
      </PageHeaderActions>
      <UnifiedMcpPanel ref={panelRef} onOpenChange={goBack} />
    </>
  );
}
