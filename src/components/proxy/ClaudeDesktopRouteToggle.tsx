import { Loader2, Radio } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { cn } from "@/lib/utils";
import { useClaudeDesktopProxyDependency } from "@/lib/query/adapters";

interface ClaudeDesktopRouteToggleProps {
  className?: string;
}

export function ClaudeDesktopRouteToggle({
  className,
}: ClaudeDesktopRouteToggleProps) {
  const { t } = useTranslation();
  const {
    isRunning,
    status,
    takeoverStatus,
    startProxyServer,
    stopProxyServer,
    isStarting,
    isStoppingServer,
  } = useProxyStatus();

  const isBusy = isStarting || isStoppingServer;
  const otherTakeoverActive = Boolean(
    takeoverStatus?.claude ||
      takeoverStatus?.codex ||
      takeoverStatus?.gemini ||
      takeoverStatus?.grokbuild,
  );
  // PROXY-30: the backend refuses to stop while Claude Desktop's live
  // profile still points at the local route; explain that up front.
  const dependency = useClaudeDesktopProxyDependency(isRunning);
  const desktopDependsOnRoute = isRunning && dependency.data === true;
  const dependencyMessage = t("claudeDesktop.route.stopBlockedByDesktop", {
    defaultValue:
      "Claude Desktop still sends its requests through local routing. Switch Claude Desktop to a direct provider first, then stop local routing.",
  });
  const routeAddress = status?.address ?? "127.0.0.1";
  const routePort = status?.port ?? 15721;

  const handleToggle = async (checked: boolean) => {
    try {
      if (checked) {
        await startProxyServer();
        return;
      }

      if (otherTakeoverActive) {
        toast.warning(
          t("claudeDesktop.route.stopBlockedByTakeover", {
            defaultValue:
              "Another app is still using routing takeover. Turn off that app's takeover in Settings first, then stop local routing.",
          }),
          { duration: 5000 },
        );
        return;
      }

      const { data: dependsNow } = await dependency.refetch();
      if (dependsNow) {
        toast.warning(dependencyMessage, { duration: 5000 });
        return;
      }

      await stopProxyServer();
    } catch (error) {
      console.error("[ClaudeDesktopRouteToggle] Toggle route failed:", error);
    }
  };

  const tooltipText = isRunning
    ? t("claudeDesktop.route.tooltip.active", {
        address: routeAddress,
        port: routePort,
        defaultValue: `Claude Desktop 本地路由已开启 - ${routeAddress}:${routePort}`,
      })
    : t("claudeDesktop.route.tooltip.inactive", {
        address: routeAddress,
        port: routePort,
        defaultValue: `开启 Claude Desktop 本地路由，用于需要模型映射或格式转换的供应商。当前配置地址：${routeAddress}:${routePort}`,
      });

  return (
    <div
      className={cn(
        "flex items-center gap-1 px-1.5 h-8 rounded-lg bg-muted/50 transition-all",
        className,
      )}
      title={
        desktopDependsOnRoute
          ? `${tooltipText}\n${dependencyMessage}`
          : tooltipText
      }
    >
      {isBusy ? (
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      ) : (
        <Radio
          className={cn(
            "h-4 w-4 transition-colors",
            isRunning
              ? "text-emerald-500 animate-pulse"
              : "text-muted-foreground",
          )}
        />
      )}
      <Switch
        checked={isRunning}
        onCheckedChange={handleToggle}
        disabled={isBusy}
      />
    </div>
  );
}
