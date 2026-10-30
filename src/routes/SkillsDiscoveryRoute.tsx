import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { HeaderToolbar, PageHeaderActions } from "@/app/PageHeader";
import { useNavigation } from "@/app/navigation";
import { APP_REGISTRY } from "@/config/apps";
import {
  SkillsPage,
  getSkillsPageHeaderActions,
  type SkillsPageHandle,
  type SkillsPageSource,
} from "@/components/skills/SkillsPage";

const HEADER_BUTTON_CLASS = "hover:bg-black/5 dark:hover:bg-white/5";

/** Skills discovery page (repos / skills.sh). */
export default function SkillsDiscoveryRoute() {
  const { t } = useTranslation();
  const { activeApp } = useNavigation();
  const pageRef = useRef<SkillsPageHandle>(null);
  const [source, setSource] = useState<SkillsPageSource>("repos");

  return (
    <>
      <PageHeaderActions>
        <HeaderToolbar>
          {getSkillsPageHeaderActions(source).map(
            ({ key, labelKey, Icon, execute }) => (
              <Button
                key={key}
                variant="ghost"
                size="sm"
                onClick={() => execute(pageRef.current)}
                className={HEADER_BUTTON_CLASS}
              >
                <Icon className="w-4 h-4 mr-2" />
                {t(labelKey)}
              </Button>
            ),
          )}
        </HeaderToolbar>
      </PageHeaderActions>
      <SkillsPage
        ref={pageRef}
        initialApp={APP_REGISTRY[activeApp].skillsTargetApp}
        onSourceChange={setSource}
      />
    </>
  );
}
