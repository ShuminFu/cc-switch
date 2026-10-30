import { Bot } from "lucide-react";
import { useTranslation } from "react-i18next";

interface AgentsPanelProps {
  onOpenChange: (open: boolean) => void;
}

export function AgentsPanel({}: AgentsPanelProps) {
  const { t } = useTranslation();
  return (
    <div className="px-6 flex flex-col flex-1 min-h-0">
      <div className="flex-1 glass-card rounded-xl p-8 flex flex-col items-center justify-center text-center space-y-4">
        <div className="w-20 h-20 rounded-full bg-white/5 flex items-center justify-center mb-4 animate-pulse-slow">
          <Bot className="w-10 h-10 text-muted-foreground" />
        </div>
        <h3 className="text-xl font-semibold">
          {t("agents.comingSoon.title", { defaultValue: "Coming Soon" })}
        </h3>
        <p className="text-muted-foreground max-w-md">
          {t("agents.comingSoon.description", {
            defaultValue:
              "The Agents management feature is currently under development. Stay tuned for powerful autonomous capabilities.",
          })}
        </p>
      </div>
    </div>
  );
}
