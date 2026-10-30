import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HeaderToolbar, PageHeaderActions } from "@/app/PageHeader";
import { useNavigation } from "@/app/navigation";
import PromptPanel, {
  type PromptPanelHandle,
} from "@/components/prompts/PromptPanel";

const HEADER_BUTTON_CLASS = "hover:bg-black/5 dark:hover:bg-white/5";

/** Prompts page: prompt list for the active app, "add" in the header. */
export default function PromptsRoute() {
  const { t } = useTranslation();
  const { featureApp, goBack } = useNavigation();
  const panelRef = useRef<PromptPanelHandle>(null);

  return (
    <>
      <PageHeaderActions>
        <HeaderToolbar>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => panelRef.current?.openAdd()}
            className={HEADER_BUTTON_CLASS}
          >
            <Plus className="w-4 h-4 mr-2" />
            {t("prompts.add")}
          </Button>
        </HeaderToolbar>
      </PageHeaderActions>
      <PromptPanel
        ref={panelRef}
        open={true}
        onOpenChange={goBack}
        appId={featureApp}
      />
    </>
  );
}
