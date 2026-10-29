import type { ReactNode } from "react";
import { motion, AnimatePresence } from "framer-motion";
import type { AppId } from "@/lib/api";
import { SettingsPage } from "@/components/settings/SettingsPage";
import PromptPanel from "@/components/prompts/PromptPanel";
import UnifiedMcpPanel from "@/components/mcp/UnifiedMcpPanel";
import {
  SkillsPage,
  type SkillsPageSource,
} from "@/components/skills/SkillsPage";
import UnifiedSkillsPanel from "@/components/skills/UnifiedSkillsPanel";
import { AgentsPanel } from "@/components/agents/AgentsPanel";
import { UniversalProviderPanel } from "@/components/universal";
import { SessionManagerPage } from "@/components/sessions/SessionManagerPage";
import WorkspaceFilesPanel from "@/components/workspace/WorkspaceFilesPanel";
import EnvPanel from "@/components/openclaw/EnvPanel";
import ToolsPanel from "@/components/openclaw/ToolsPanel";
import AgentsDefaultsPanel from "@/components/openclaw/AgentsDefaultsPanel";
import HermesMemoryPanel from "@/components/hermes/HermesMemoryPanel";
import type { ShellPanelRefs } from "./panelRefs";
import type { View } from "./views";

export interface AppViewContentProps {
  view: View;
  onNavigate: (view: View) => void;
  sharedFeatureApp: AppId;
  panelRefs: ShellPanelRefs;
  settingsDefaultTab: string;
  onImportSuccess: () => Promise<void>;
  onSkillsDiscoverySourceChange: (source: SkillsPageSource) => void;
  onOpenSkillsDiscovery: () => void;
  /** 首页（供应商列表）内容 */
  providersView: ReactNode;
}

function renderView({
  view,
  onNavigate,
  sharedFeatureApp,
  panelRefs,
  settingsDefaultTab,
  onImportSuccess,
  onSkillsDiscoverySourceChange,
  onOpenSkillsDiscovery,
  providersView,
}: AppViewContentProps): ReactNode {
  const goHome = () => onNavigate("providers");
  // OpenClaw 不支持 Skills，Skills 视图回落到 Claude
  const skillsApp =
    sharedFeatureApp === "openclaw" ? "claude" : sharedFeatureApp;

  switch (view) {
    case "settings":
      return (
        <SettingsPage
          open={true}
          onOpenChange={goHome}
          onImportSuccess={onImportSuccess}
          defaultTab={settingsDefaultTab}
        />
      );
    case "prompts":
      return (
        <PromptPanel
          ref={panelRefs.prompts}
          open={true}
          onOpenChange={goHome}
          appId={sharedFeatureApp}
        />
      );
    case "hermesMemory":
      return <HermesMemoryPanel />;
    case "skills":
      return (
        <UnifiedSkillsPanel
          ref={panelRefs.skills}
          onOpenDiscovery={onOpenSkillsDiscovery}
          currentApp={skillsApp}
        />
      );
    case "skillsDiscovery":
      return (
        <SkillsPage
          ref={panelRefs.skillsDiscovery}
          initialApp={skillsApp}
          onSourceChange={onSkillsDiscoverySourceChange}
        />
      );
    case "mcp":
      return <UnifiedMcpPanel ref={panelRefs.mcp} onOpenChange={goHome} />;
    case "agents":
      return <AgentsPanel onOpenChange={goHome} />;
    case "universal":
      return (
        <div className="px-6 pt-4">
          <UniversalProviderPanel />
        </div>
      );
    case "sessions":
      return (
        <SessionManagerPage key={sharedFeatureApp} appId={sharedFeatureApp} />
      );
    case "workspace":
      return <WorkspaceFilesPanel />;
    case "openclawEnv":
      return <EnvPanel />;
    case "openclawTools":
      return <ToolsPanel />;
    case "openclawAgents":
      return <AgentsDefaultsPanel />;
    default:
      return providersView;
  }
}

/** 主内容区：按当前视图渲染对应面板，视图切换时淡入淡出 */
export function AppViewContent(props: AppViewContentProps) {
  return (
    <AnimatePresence mode="wait">
      <motion.div
        key={props.view}
        className="flex-1 min-h-0"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.2 }}
      >
        {renderView(props)}
      </motion.div>
    </AnimatePresence>
  );
}
