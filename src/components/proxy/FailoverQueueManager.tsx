/**
 * 故障转移队列管理组件
 *
 * 允许用户管理代理模式下的故障转移队列，支持：
 * - 添加/移除供应商
 * - 队列顺序基于首页供应商列表的 sort_index（上移/下移会交换两者的 sort_index）
 * - 每项显示健康状态与最近一次失败原因，可连通性检查与重置熔断（FE-UI-24）
 */

import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  Plus,
  Trash2,
  Loader2,
  Info,
  AlertTriangle,
  ArrowUp,
  ArrowDown,
  Activity,
  RotateCcw,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type { FailoverQueueItem, ProviderHealth } from "@/types/proxy";
import { providersApi, type AppId } from "@/lib/api";
import { queryKeys } from "@/lib/query/keys";
import { useStreamCheck } from "@/hooks/useStreamCheck";
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";
import { swapQueueSortIndex } from "./failoverQueueOrder";
import {
  useFailoverQueue,
  useAvailableProvidersForFailover,
  useAddToFailoverQueue,
  useRemoveFromFailoverQueue,
  useAutoFailoverEnabled,
  useSetAutoFailoverEnabled,
  useProvidersHealthMap,
  useResetCircuitBreaker,
} from "@/lib/query/failover";

interface FailoverQueueManagerProps {
  appType: AppId;
  disabled?: boolean;
}

export function FailoverQueueManager({
  appType,
  disabled = false,
}: FailoverQueueManagerProps) {
  const { t } = useTranslation();
  const [selectedProviderId, setSelectedProviderId] = useState<string>("");

  // 故障转移开关状态（每个应用独立）
  const { data: isFailoverEnabled = false } = useAutoFailoverEnabled(appType);
  const setFailoverEnabled = useSetAutoFailoverEnabled();

  // 查询数据
  const {
    data: queue,
    isLoading: isQueueLoading,
    error: queueError,
  } = useFailoverQueue(appType);
  const queueProviderIds = useMemo(
    () => (queue ?? []).map((item) => item.providerId),
    [queue],
  );
  const { data: healthMap } = useProvidersHealthMap(appType, queueProviderIds);
  const { data: availableProviders, isLoading: isProvidersLoading } =
    useAvailableProvidersForFailover(appType);

  // Mutations
  const addToQueue = useAddToFailoverQueue();
  const removeFromQueue = useRemoveFromFailoverQueue();
  const resetCircuit = useResetCircuitBreaker();
  const { checkProvider, isChecking } = useStreamCheck(appType);
  const queryClient = useQueryClient();
  const [isReordering, setIsReordering] = useState(false);

  const handleMove = async (from: number, to: number) => {
    if (!queue) return;
    const updates = swapQueueSortIndex(queue, from, to);
    if (!updates) return;
    setIsReordering(true);
    try {
      await providersApi.updateSortOrder(updates, appType);
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: queryKeys.failover.queue(appType),
        }),
        queryClient.invalidateQueries({
          queryKey: queryKeys.providers.byApp(appType),
        }),
      ]);
    } catch (error) {
      toast.error(
        t("proxy.failoverQueue.reorderFailed", {
          defaultValue: "Could not reorder the queue",
        }),
        { description: String(error) },
      );
    } finally {
      setIsReordering(false);
    }
  };

  const handleResetCircuit = async (providerId: string) => {
    try {
      await resetCircuit.mutateAsync({ providerId, appType });
      toast.success(
        t("proxy.failoverQueue.circuitReset", {
          defaultValue: "Circuit breaker reset",
        }),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(String(error));
    }
  };

  // 切换故障转移开关
  const handleToggleFailover = (enabled: boolean) => {
    setFailoverEnabled.mutate({ appType, enabled });
  };

  // 添加供应商到队列
  const handleAddProvider = async () => {
    if (!selectedProviderId) return;

    try {
      await addToQueue.mutateAsync({
        appType,
        providerId: selectedProviderId,
      });
      setSelectedProviderId("");
      toast.success(
        t("proxy.failoverQueue.addSuccess", "已添加到故障转移队列"),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(t("proxy.failoverQueue.addFailed", "添加失败"), {
        description: String(error),
      });
    }
  };

  // 从队列移除供应商
  const handleRemoveProvider = async (providerId: string) => {
    try {
      await removeFromQueue.mutateAsync({ appType, providerId });
      toast.success(
        t("proxy.failoverQueue.removeSuccess", "已从故障转移队列移除"),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(t("proxy.failoverQueue.removeFailed", "移除失败"), {
        description: String(error),
      });
    }
  };

  if (isQueueLoading) {
    return (
      <div className="flex items-center justify-center p-8">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (queueError) {
    return (
      <Alert variant="destructive">
        <AlertTriangle className="h-4 w-4" />
        <AlertDescription>{String(queueError)}</AlertDescription>
      </Alert>
    );
  }

  return (
    <div className="space-y-4">
      {/* 自动故障转移开关 */}
      <div className="flex items-center justify-between p-4 rounded-lg bg-muted/50 border border-border/50">
        <div className="space-y-0.5">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium">
              {t("proxy.failover.autoSwitch", {
                defaultValue: "自动故障转移",
              })}
            </span>
            {isFailoverEnabled && (
              <span className="px-2 py-0.5 text-xs rounded-full bg-emerald-500/20 text-emerald-600 dark:text-emerald-400">
                {t("common.enabled", { defaultValue: "已开启" })}
              </span>
            )}
          </div>
          <p className="text-xs text-muted-foreground">
            {t("proxy.failover.autoSwitchDescription", {
              defaultValue:
                "开启后将立即切换到队列 P1，并在请求失败时自动切换到队列中的下一个供应商",
            })}
          </p>
        </div>
        <Switch
          checked={isFailoverEnabled}
          onCheckedChange={handleToggleFailover}
          disabled={disabled || setFailoverEnabled.isPending}
        />
      </div>

      {/* 说明信息 */}
      <Alert className="border-blue-500/40 bg-blue-500/10">
        <Info className="h-4 w-4" />
        <AlertDescription className="text-sm">
          {t(
            "proxy.failoverQueue.info",
            "队列顺序与首页供应商列表顺序一致。当请求失败时，系统会按顺序依次尝试队列中的供应商。",
          )}
        </AlertDescription>
      </Alert>

      {/* 添加供应商 */}
      <div className="flex items-center gap-2">
        <Select
          value={selectedProviderId}
          onValueChange={setSelectedProviderId}
          disabled={disabled || isProvidersLoading}
        >
          <SelectTrigger className="flex-1">
            <SelectValue
              placeholder={t(
                "proxy.failoverQueue.selectProvider",
                "选择供应商添加到队列",
              )}
            />
          </SelectTrigger>
          <SelectContent>
            {availableProviders?.map((provider) => (
              <SelectItem key={provider.id} value={provider.id}>
                {provider.name}
                {provider.notes && (
                  <span className="ml-1 text-xs text-muted-foreground">
                    ({provider.notes})
                  </span>
                )}
              </SelectItem>
            ))}
            {(!availableProviders || availableProviders.length === 0) && (
              <div className="px-2 py-4 text-center text-sm text-muted-foreground">
                {t(
                  "proxy.failoverQueue.noAvailableProviders",
                  "没有可添加的供应商",
                )}
              </div>
            )}
          </SelectContent>
        </Select>
        <Button
          onClick={handleAddProvider}
          disabled={disabled || !selectedProviderId || addToQueue.isPending}
          size="icon"
          variant="outline"
        >
          {addToQueue.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Plus className="h-4 w-4" />
          )}
        </Button>
      </div>

      {/* 队列列表 */}
      {!queue || queue.length === 0 ? (
        <div className="rounded-lg border border-dashed border-muted-foreground/40 p-8 text-center">
          <p className="text-sm text-muted-foreground">
            {t(
              "proxy.failoverQueue.empty",
              "故障转移队列为空。添加供应商以启用自动故障转移。",
            )}
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {queue.map((item, index) => (
            <QueueItem
              key={item.providerId}
              item={item}
              health={healthMap?.[item.providerId]}
              index={index}
              disabled={disabled}
              onRemove={handleRemoveProvider}
              isRemoving={removeFromQueue.isPending}
              canMoveUp={
                !isReordering && !!swapQueueSortIndex(queue, index, index - 1)
              }
              canMoveDown={
                !isReordering && !!swapQueueSortIndex(queue, index, index + 1)
              }
              onMoveUp={() => void handleMove(index, index - 1)}
              onMoveDown={() => void handleMove(index, index + 1)}
              onTest={() =>
                void checkProvider(item.providerId, item.providerName)
              }
              isTesting={isChecking(item.providerId)}
              onResetCircuit={() => void handleResetCircuit(item.providerId)}
              isResetting={resetCircuit.isPending}
            />
          ))}
        </div>
      )}

      {/* 队列说明 */}
      {queue && queue.length > 0 && (
        <p className="text-xs text-muted-foreground">
          {t(
            "proxy.failoverQueue.orderHint",
            "队列顺序与首页供应商列表顺序一致，可在首页拖拽调整顺序。",
          )}
        </p>
      )}
    </div>
  );
}

interface QueueItemProps {
  item: FailoverQueueItem;
  health?: ProviderHealth;
  index: number;
  disabled: boolean;
  onRemove: (providerId: string) => void;
  isRemoving: boolean;
  canMoveUp: boolean;
  canMoveDown: boolean;
  onMoveUp: () => void;
  onMoveDown: () => void;
  onTest: () => void;
  isTesting: boolean;
  onResetCircuit: () => void;
  isResetting: boolean;
}

function QueueItem({
  item,
  health,
  index,
  disabled,
  onRemove,
  isRemoving,
  canMoveUp,
  canMoveDown,
  onMoveUp,
  onMoveDown,
  onTest,
  isTesting,
  onResetCircuit,
  isResetting,
}: QueueItemProps) {
  const { t, i18n } = useTranslation();
  const lastFailure =
    health?.last_error && health.last_failure_at
      ? {
          error: health.last_error,
          at: new Date(health.last_failure_at).toLocaleString(
            i18n?.language || undefined,
          ),
        }
      : null;

  return (
    <div
      className={cn(
        "flex items-center gap-3 rounded-lg border bg-card p-3 transition-colors",
      )}
    >
      {/* 序号 */}
      <div className="flex h-6 w-6 items-center justify-center rounded-full bg-muted text-xs font-medium">
        {index + 1}
      </div>

      {/* 供应商名称 + 健康状态 */}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium truncate">
            {item.providerName}
            {item.providerNotes && (
              <span className="ml-1 text-xs text-muted-foreground">
                ({item.providerNotes})
              </span>
            )}
          </span>
          {health && (
            <ProviderHealthBadge
              consecutiveFailures={health.consecutive_failures}
              isHealthy={health.is_healthy}
            />
          )}
        </div>
        {lastFailure && (
          <p
            className="mt-0.5 truncate text-xs text-muted-foreground"
            title={lastFailure.error}
            data-testid={`queue-last-error-${item.providerId}`}
          >
            {t("proxy.failoverQueue.lastFailure", {
              defaultValue: "Last failure {{time}}: {{error}}",
              time: lastFailure.at,
              error: lastFailure.error,
            })}
          </p>
        )}
      </div>

      <div className="flex items-center">
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8"
          onClick={onMoveUp}
          disabled={disabled || !canMoveUp}
          aria-label={t("proxy.failoverQueue.moveUp", {
            defaultValue: "Move up",
          })}
        >
          <ArrowUp className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8"
          onClick={onMoveDown}
          disabled={disabled || !canMoveDown}
          aria-label={t("proxy.failoverQueue.moveDown", {
            defaultValue: "Move down",
          })}
        >
          <ArrowDown className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8"
          onClick={onTest}
          disabled={isTesting}
          aria-label={t("proxy.failoverQueue.test", {
            defaultValue: "Test connectivity",
          })}
        >
          {isTesting ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Activity className="h-4 w-4" />
          )}
        </Button>
        {health && health.consecutive_failures > 0 && (
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={onResetCircuit}
            disabled={disabled || isResetting}
            aria-label={t("proxy.failoverQueue.resetCircuit", {
              defaultValue: "Reset circuit breaker",
            })}
          >
            <RotateCcw className="h-4 w-4" />
          </Button>
        )}
      </div>

      {/* 删除按钮 */}
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8 text-muted-foreground hover:text-destructive"
        onClick={() => onRemove(item.providerId)}
        disabled={disabled || isRemoving}
        aria-label={t("common.delete", "删除")}
      >
        {isRemoving ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <Trash2 className="h-4 w-4" />
        )}
      </Button>
    </div>
  );
}
