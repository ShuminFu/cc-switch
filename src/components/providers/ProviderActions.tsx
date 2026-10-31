import {
  Activity,
  BarChart3,
  Check,
  Copy,
  Edit,
  Loader2,
  Minus,
  Play,
  Link2,
  Plus,
  Terminal,
  Trash2,
  Zap,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { AppId } from "@/lib/api";

interface ProviderActionsProps {
  appId?: AppId;
  isCurrent: boolean;
  isInConfig?: boolean;
  /** Additive apps: live config ids not loaded yet (FE-PROV-51). */
  isConfigStatusLoading?: boolean;
  isTesting?: boolean;
  // Some provider mutation is in flight; disable the main/failover button
  isActionPending?: boolean;
  isProxyTakeover?: boolean;
  isOmo?: boolean;
  onSwitch: () => void;
  onEdit: () => void;
  onDuplicate: () => void;
  onTest?: () => void;
  onConfigureUsage?: () => void;
  onDelete: () => void;
  onRemoveFromConfig?: () => void;
  onDisableOmo?: () => void;
  onOpenTerminal?: () => void;
  /** Copy a ccswitch:// share link (without the API key). */
  onShare?: () => void;
  isAutoFailoverEnabled?: boolean;
  isInFailoverQueue?: boolean;
  onToggleFailover?: (enabled: boolean) => void;
  isOfficialBlockedByProxy?: boolean;
  // Hermes v12+ providers: dict overlay — edit/delete must go through Web UI
  isReadOnly?: boolean;
  // OpenClaw: default model
  isDefaultModel?: boolean;
  onSetAsDefault?: () => void;
}

// 主按钮的呈现状态。title 用于 disabled 态向用户解释为何不可点击；
// 因 Button 基类带 disabled:pointer-events-none，title 必须挂在外层非禁用
// 的 wrapper 上才会在 hover 时显示（见下方 <span> 包裹）。
interface MainButtonState {
  disabled: boolean;
  variant: "default" | "secondary";
  className: string;
  icon: JSX.Element;
  text: string;
  title?: string;
}

export function ProviderActions({
  appId,
  isCurrent,
  isInConfig = false,
  isConfigStatusLoading = false,
  isTesting,
  isActionPending = false,
  isProxyTakeover = false,
  isOmo = false,
  onSwitch,
  onEdit,
  onDuplicate,
  onTest,
  onConfigureUsage,
  onDelete,
  onRemoveFromConfig,
  onDisableOmo,
  onOpenTerminal,
  onShare,
  isAutoFailoverEnabled = false,
  isInFailoverQueue = false,
  onToggleFailover,
  isOfficialBlockedByProxy = false,
  isReadOnly = false,
  // OpenClaw: default model
  isDefaultModel = false,
  onSetAsDefault,
}: ProviderActionsProps) {
  const { t } = useTranslation();
  const iconButtonClass = "h-8 w-8 p-1";

  // 累加模式应用（OpenCode 非 OMO / OpenClaw / Hermes）
  const isAdditiveMode =
    (appId === "opencode" && !isOmo) ||
    appId === "openclaw" ||
    appId === "hermes";

  // 故障转移模式下的按钮逻辑（累加模式和 OMO 应用不支持故障转移）
  const isFailoverMode =
    !isAdditiveMode && !isOmo && isAutoFailoverEnabled && onToggleFailover;

  const handleMainButtonClick = () => {
    if (isActionPending) return;
    if (isOmo) {
      if (isCurrent) {
        onDisableOmo?.();
      } else {
        onSwitch();
      }
    } else if (isAdditiveMode) {
      // 累加模式：切换配置状态（添加/移除）
      if (isInConfig) {
        if (onRemoveFromConfig) {
          onRemoveFromConfig();
        } else {
          onDelete();
        }
      } else {
        onSwitch(); // 添加到配置
      }
    } else if (isFailoverMode) {
      onToggleFailover(!isInFailoverQueue);
    } else {
      onSwitch();
    }
  };

  const getMainButtonState = (): MainButtonState => {
    if (isOmo) {
      if (isCurrent) {
        return {
          disabled: false,
          variant: "secondary" as const,
          className:
            "bg-gray-200 text-muted-foreground hover:bg-gray-200 hover:text-muted-foreground dark:bg-gray-700 dark:hover:bg-gray-700",
          icon: <Check className="h-4 w-4" />,
          text: t("provider.inUse"),
        };
      }
      return {
        disabled: false,
        variant: "default" as const,
        className: "",
        icon: <Play className="h-4 w-4" />,
        text: t("provider.enable"),
      };
    }

    // 累加模式（OpenCode 非 OMO / OpenClaw）
    if (isAdditiveMode) {
      if (isConfigStatusLoading) {
        return {
          disabled: true,
          variant: "secondary" as const,
          className: "",
          icon: <Loader2 className="h-4 w-4 animate-spin" />,
          text: "",
          title: t("provider.configStatusLoading", {
            defaultValue: "Checking the app config…",
          }),
        };
      }
      if (isInConfig) {
        return {
          disabled: isDefaultModel === true,
          variant: "secondary" as const,
          className: cn(
            "bg-orange-100 text-orange-600 hover:bg-orange-200 dark:bg-orange-900/50 dark:text-orange-400 dark:hover:bg-orange-900/70",
            isDefaultModel && "opacity-40 cursor-not-allowed",
          ),
          icon: <Minus className="h-4 w-4" />,
          text: t("provider.removeFromConfig", { defaultValue: "Remove" }),
        };
      }
      return {
        disabled: false,
        variant: "default" as const,
        className:
          "bg-emerald-500 hover:bg-emerald-600 dark:bg-emerald-600 dark:hover:bg-emerald-700",
        icon: <Plus className="h-4 w-4" />,
        text: t("provider.addToConfig", { defaultValue: "Add" }),
      };
    }

    if (isFailoverMode) {
      if (isInFailoverQueue) {
        return {
          disabled: false,
          variant: "secondary" as const,
          className:
            "bg-blue-100 text-blue-600 hover:bg-blue-200 dark:bg-blue-900/50 dark:text-blue-400 dark:hover:bg-blue-900/70",
          icon: <Check className="h-4 w-4" />,
          text: t("failover.inQueue", { defaultValue: "In queue" }),
        };
      }
      return {
        disabled: false,
        variant: "default" as const,
        className:
          "bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700",
        icon: <Plus className="h-4 w-4" />,
        text: t("failover.addQueue", { defaultValue: "Add" }),
      };
    }

    if (isCurrent) {
      return {
        disabled: true,
        variant: "secondary" as const,
        className:
          "bg-gray-200 text-muted-foreground hover:bg-gray-200 hover:text-muted-foreground dark:bg-gray-700 dark:hover:bg-gray-700",
        icon: <Check className="h-4 w-4" />,
        text: t("provider.inUse"),
      };
    }

    if (isOfficialBlockedByProxy) {
      return {
        disabled: true,
        variant: "default" as const,
        className: "",
        icon: <Play className="h-4 w-4" />,
        text: t("provider.enable"),
        title: t("provider.blockedByProxyHint"),
      };
    }

    return {
      disabled: false,
      variant: "default" as const,
      className: isProxyTakeover
        ? "bg-emerald-500 hover:bg-emerald-600 dark:bg-emerald-600 dark:hover:bg-emerald-700"
        : "",
      icon: <Play className="h-4 w-4" />,
      text: t("provider.enable"),
    };
  };

  const buttonState = getMainButtonState();

  const canDelete =
    !isReadOnly && (isOmo || isAdditiveMode ? true : !isCurrent);
  const readOnlyHint = t("provider.managedByHermesHint", {
    defaultValue:
      "Defined in Hermes' providers: dict. Edit or remove it via Hermes Web UI.",
  });

  return (
    <div className="flex items-center gap-1.5">
      {(appId === "openclaw" || appId === "hermes") &&
        isInConfig &&
        !isConfigStatusLoading &&
        onSetAsDefault &&
        (() => {
          const activeLabel =
            appId === "hermes"
              ? t("provider.inUse", { defaultValue: "In Use" })
              : t("provider.isDefault", { defaultValue: "Current Default" });
          const inactiveLabel =
            appId === "hermes"
              ? t("provider.enable", { defaultValue: "Enable" })
              : t("provider.setAsDefault", { defaultValue: "Set Default" });
          return (
            <Button
              size="sm"
              variant={isDefaultModel ? "secondary" : "default"}
              onClick={isDefaultModel ? undefined : onSetAsDefault}
              disabled={isDefaultModel}
              className={cn(
                "w-fit px-2.5",
                isDefaultModel
                  ? "bg-gray-200 text-muted-foreground dark:bg-gray-700 opacity-60 cursor-not-allowed"
                  : "bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700",
              )}
            >
              <Zap className="h-4 w-4" />
              {isDefaultModel ? activeLabel : inactiveLabel}
            </Button>
          );
        })()}

      {/* wrapper span 承接 hover：disabled 按钮自身 pointer-events:none，
          原生 title 与 cursor 都必须挂在未禁用的外层元素上才会生效 */}
      <span
        title={buttonState.title}
        className={cn(
          "inline-flex",
          buttonState.disabled && "cursor-not-allowed",
        )}
      >
        <Button
          size="sm"
          variant={buttonState.variant}
          onClick={handleMainButtonClick}
          disabled={buttonState.disabled || isActionPending}
          aria-busy={isActionPending || isConfigStatusLoading || undefined}
          aria-label={buttonState.text ? undefined : buttonState.title}
          className={cn("w-[4.5rem] px-2.5", buttonState.className)}
        >
          {buttonState.icon}
          {buttonState.text}
        </Button>
      </span>

      {/* Secondary actions: revealed on hover/focus where hover exists,
          always visible on touch; the main button is always shown
          (FE-PROV-29). */}
      <div className="flex items-center gap-1 duration-200 [@media(hover:hover)]:opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100 transition-opacity">
        <Button
          size="icon"
          variant="ghost"
          onClick={isReadOnly ? undefined : onEdit}
          disabled={isReadOnly}
          title={isReadOnly ? readOnlyHint : t("common.edit")}
          className={cn(
            iconButtonClass,
            isReadOnly && "opacity-40 cursor-not-allowed text-muted-foreground",
          )}
        >
          <Edit className="h-4 w-4" />
        </Button>

        <Button
          size="icon"
          variant="ghost"
          onClick={isReadOnly ? undefined : onDuplicate}
          disabled={isReadOnly}
          title={isReadOnly ? readOnlyHint : t("provider.duplicate")}
          className={cn(
            iconButtonClass,
            isReadOnly && "opacity-40 cursor-not-allowed text-muted-foreground",
          )}
        >
          <Copy className="h-4 w-4" />
        </Button>

        <Button
          size="icon"
          variant="ghost"
          onClick={onTest || undefined}
          disabled={isTesting || !onTest}
          title={t("provider.connectivityCheck", {
            defaultValue: "Check connectivity",
          })}
          className={cn(
            iconButtonClass,
            !onTest && "opacity-40 cursor-not-allowed text-muted-foreground",
          )}
        >
          {isTesting ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Activity className="h-4 w-4" />
          )}
        </Button>

        <Button
          size="icon"
          variant="ghost"
          onClick={onConfigureUsage || undefined}
          disabled={!onConfigureUsage}
          title={t("provider.configureUsage")}
          className={cn(
            iconButtonClass,
            !onConfigureUsage &&
              "opacity-40 cursor-not-allowed text-muted-foreground",
          )}
        >
          <BarChart3 className="h-4 w-4" />
        </Button>

        {onShare && (
          <Button
            size="icon"
            variant="ghost"
            onClick={onShare}
            title={t("provider.copyShareLink", {
              defaultValue: "Copy share link (without API key)",
            })}
            className={iconButtonClass}
          >
            <Link2 className="h-4 w-4" />
          </Button>
        )}

        {onOpenTerminal && (
          <Button
            size="icon"
            variant="ghost"
            onClick={onOpenTerminal}
            title={t("provider.openTerminal", {
              defaultValue: "Open terminal",
            })}
            className={cn(
              iconButtonClass,
              "hover:text-emerald-600 dark:hover:text-emerald-400",
            )}
          >
            <Terminal className="h-4 w-4" />
          </Button>
        )}

        <Button
          size="icon"
          variant="ghost"
          onClick={canDelete ? onDelete : undefined}
          disabled={!canDelete}
          title={isReadOnly ? readOnlyHint : t("common.delete")}
          className={cn(
            iconButtonClass,
            canDelete && "hover:text-red-500 dark:hover:text-red-400",
            !canDelete && "opacity-40 cursor-not-allowed text-muted-foreground",
          )}
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}
