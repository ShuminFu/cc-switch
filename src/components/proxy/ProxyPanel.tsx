import { useState, useEffect } from "react";
import {
  Activity,
  Clock,
  TrendingUp,
  Server,
  ListOrdered,
  Save,
  Loader2,
  Zap,
  Power,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { ToggleRow } from "@/components/ui/toggle-row";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { toast } from "sonner";
import { useFailoverQueue } from "@/lib/query/failover";
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";
import { useProviderHealth } from "@/lib/query/failover";
import {
  useProxyTakeoverStatus,
  useSetProxyTakeoverForApp,
  useGlobalProxyConfig,
  useUpdateGlobalProxyConfig,
} from "@/lib/query/proxy";
import type { ProxyStatus } from "@/types/proxy";
import { useTranslation } from "react-i18next";
import { AnimatePresence, motion } from "framer-motion";
import { extractErrorMessage } from "@/utils/errorUtils";

// 与后端 inbound_auth::is_loopback_listen_address 保持一致：
// 非回环监听会把代理暴露给局域网，需要在 UI 上提示。
function isLoopbackListenAddress(address: string): boolean {
  const addr = address
    .trim()
    .replace(/^\[|\]$/g, "")
    .toLowerCase();
  if (addr === "" || addr === "localhost" || addr === "::1") return true;
  return /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(addr);
}

interface ProxyPanelProps {
  enableLocalProxy: boolean;
  onEnableLocalProxyChange: (checked: boolean) => void;
  onToggleProxy: (checked: boolean) => Promise<void>;
  isProxyPending: boolean;
}

export function ProxyPanel({
  enableLocalProxy,
  onEnableLocalProxyChange,
  onToggleProxy,
  isProxyPending,
}: ProxyPanelProps) {
  const { t } = useTranslation();
  const { status, isRunning } = useProxyStatus();

  // 获取应用接管状态
  const { data: takeoverStatus } = useProxyTakeoverStatus();
  const setTakeoverForApp = useSetProxyTakeoverForApp();

  // 获取全局代理配置
  const { data: globalConfig } = useGlobalProxyConfig();
  const updateGlobalConfig = useUpdateGlobalProxyConfig();

  // 监听地址/端口的本地状态（端口用字符串以支持完全清空）
  const [listenAddress, setListenAddress] = useState("127.0.0.1");
  const [listenPort, setListenPort] = useState("15721");

  // 同步全局配置到本地状态
  useEffect(() => {
    if (globalConfig) {
      setListenAddress(globalConfig.listenAddress);
      setListenPort(String(globalConfig.listenPort));
    }
  }, [globalConfig]);

  // 获取所有三个应用类型的故障转移队列
  // 启用自动故障转移后，将按队列优先级（P1→P2→...）选择供应商
  const { data: claudeQueue = [] } = useFailoverQueue("claude");
  const { data: codexQueue = [] } = useFailoverQueue("codex");
  const { data: geminiQueue = [] } = useFailoverQueue("gemini");
  const { data: grokQueue = [] } = useFailoverQueue("grokbuild");

  const handleTakeoverChange = async (appType: string, enabled: boolean) => {
    try {
      await setTakeoverForApp.mutateAsync({ appType, enabled });
      toast.success(
        enabled
          ? t("proxy.takeover.enabled", {
              app: appType,
            })
          : t("proxy.takeover.disabled", {
              app: appType,
            }),
        { closeButton: true },
      );
    } catch (error) {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("proxy.takeover.failed", {
          detail,
        }),
      );
    }
  };

  const handleLoggingChange = async (enabled: boolean) => {
    if (!globalConfig) return;
    try {
      await updateGlobalConfig.mutateAsync({
        ...globalConfig,
        enableLogging: enabled,
      });
      toast.success(
        enabled ? t("proxy.logging.enabled") : t("proxy.logging.disabled"),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(t("proxy.logging.failed"));
    }
  };

  const handleSaveBasicConfig = async () => {
    if (!globalConfig) return;

    // 校验地址格式（IPv4 / IPv6 字面量 / localhost）
    const addressTrimmed = listenAddress.trim();
    const ipv4Regex = /^(\d{1,3}\.){3}\d{1,3}$/;
    const isValidIpv4 = (addr: string): boolean =>
      ipv4Regex.test(addr) &&
      addr.split(".").every((n) => {
        const num = parseInt(n, 10);
        return num >= 0 && num <= 255;
      });
    // IPv6 字面量校验：必须含 `:` 且能在 [..] 包装后被 URL 解析器接受。
    // 后端 (services/proxy.rs) 会把 `::` 改写成 `::1`，所以这里也接受 `::`。
    const isValidIpv6 = (addr: string): boolean => {
      if (!addr.includes(":")) return false;
      try {
        new URL(`http://[${addr}]/`);
        return true;
      } catch {
        return false;
      }
    };
    const normalizedAddress =
      addressTrimmed === "localhost" ? "127.0.0.1" : addressTrimmed;
    const isValidAddress =
      addressTrimmed === "localhost" ||
      addressTrimmed === "0.0.0.0" ||
      isValidIpv4(addressTrimmed) ||
      isValidIpv6(addressTrimmed);
    if (!isValidAddress) {
      toast.error(t("proxy.settings.invalidAddress"));
      return;
    }

    // 严格校验端口：必须是纯数字
    const portTrimmed = listenPort.trim();
    if (!/^\d+$/.test(portTrimmed)) {
      toast.error(t("proxy.settings.invalidPort"));
      return;
    }
    const port = parseInt(portTrimmed);
    if (isNaN(port) || port < 1024 || port > 65535) {
      toast.error(t("proxy.settings.invalidPort"));
      return;
    }
    try {
      await updateGlobalConfig.mutateAsync({
        ...globalConfig,
        listenAddress: normalizedAddress,
        listenPort: port,
      });
      toast.success(t("proxy.settings.configSaved"), { closeButton: true });
    } catch (error) {
      toast.error(t("proxy.settings.configSaveFailed"));
    }
  };

  const formatUptime = (seconds: number): string => {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secs = seconds % 60;

    if (hours > 0) {
      return `${hours}h ${minutes}m ${secs}s`;
    } else if (minutes > 0) {
      return `${minutes}m ${secs}s`;
    } else {
      return `${secs}s`;
    }
  };

  // 格式化地址用于 URL（IPv6 需要方括号）
  const formatAddressForUrl = (address: string, port: number): string => {
    const isIPv6 = address.includes(":");
    const host = isIPv6 ? `[${address}]` : address;
    return `http://${host}:${port}`;
  };

  return (
    <>
      <section className="space-y-4">
        {/* [1] Enable proxy button on main page — always visible */}
        <ToggleRow
          icon={<Zap className="h-4 w-4 text-green-500" />}
          title={t("settings.advanced.proxy.enableFeature")}
          description={t("settings.advanced.proxy.enableFeatureDescription")}
          checked={enableLocalProxy}
          onCheckedChange={onEnableLocalProxyChange}
        />

        {/* [2] Proxy service toggle — always visible */}
        <div className="flex items-center justify-between rounded-xl border border-border bg-card/50 p-4 transition-colors hover:bg-muted/50">
          <div className="flex items-center gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-background ring-1 ring-border">
              <Power className="h-4 w-4 text-green-500" />
            </div>
            <div className="space-y-1">
              <p className="text-sm font-medium leading-none">
                {t("proxyConfig.proxyEnabled")}
              </p>
              <p className="text-xs text-muted-foreground">
                {isRunning
                  ? t("settings.advanced.proxy.running")
                  : t("settings.advanced.proxy.stopped")}
              </p>
            </div>
          </div>
          <Switch
            checked={isRunning}
            onCheckedChange={onToggleProxy}
            disabled={isProxyPending}
          />
        </div>

        {/* [3] App takeover switches — animated, visible only when proxy is running */}
        <AnimatePresence>
          {isRunning && (
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: "auto" }}
              exit={{ opacity: 0, height: 0 }}
              transition={{ duration: 0.25, ease: "easeInOut" }}
              className="overflow-hidden"
            >
              <div className="rounded-xl border-2 border-primary/20 bg-primary/5 p-4 space-y-3">
                <p className="text-xs font-medium text-primary">
                  {t("proxyConfig.appTakeover")}
                </p>
                <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                  {(["claude", "codex", "gemini", "grokbuild"] as const).map(
                    (appType) => {
                      const isEnabled =
                        takeoverStatus?.[
                          appType as keyof typeof takeoverStatus
                        ] ?? false;
                      return (
                        <div
                          key={appType}
                          className="flex items-center justify-between rounded-md border border-primary/20 bg-background/60 px-3 py-2"
                        >
                          <span className="text-sm font-medium capitalize">
                            {appType === "grokbuild" ? "Grok Build" : appType}
                          </span>
                          <Switch
                            checked={isEnabled}
                            onCheckedChange={(checked) =>
                              handleTakeoverChange(appType, checked)
                            }
                            disabled={setTakeoverForApp.isPending}
                          />
                        </div>
                      );
                    },
                  )}
                </div>
                <p className="text-xs text-muted-foreground">
                  {t("proxy.takeover.hint")}
                </p>
              </div>
            </motion.div>
          )}
        </AnimatePresence>

        {/* Running state: service info + stats */}
        {isRunning && status ? (
          <div className="space-y-6">
            {/* [4] Running info: address + current provider */}
            <div className="rounded-lg border border-border bg-muted/40 p-4 space-y-4">
              <div>
                <p className="text-xs text-muted-foreground mb-2">
                  {t("proxy.panel.serviceAddress")}
                </p>
                <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                  <code className="flex-1 text-sm bg-background px-3 py-2 rounded border border-border/60">
                    {formatAddressForUrl(status.address, status.port)}
                  </code>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => {
                      navigator.clipboard.writeText(
                        formatAddressForUrl(status.address, status.port),
                      );
                      toast.success(t("proxy.panel.addressCopied"), {
                        closeButton: true,
                      });
                    }}
                  >
                    {t("common.copy")}
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground mt-2">
                  {t("proxy.settings.restartRequired")}
                </p>
              </div>

              <div className="pt-3 border-t border-border space-y-2">
                <p className="text-xs text-muted-foreground">
                  {t("provider.inUse")}
                </p>
                {status.active_targets && status.active_targets.length > 0 ? (
                  <div className="grid gap-2 sm:grid-cols-2">
                    {status.active_targets.map((target) => (
                      <div
                        key={target.app_type}
                        className="flex items-center justify-between rounded-md border border-border bg-background/60 px-2 py-1.5 text-xs"
                      >
                        <span className="text-muted-foreground">
                          {target.app_type}
                        </span>
                        <span
                          className="ml-2 font-medium truncate text-foreground"
                          title={target.provider_name}
                        >
                          {target.provider_name}
                        </span>
                      </div>
                    ))}
                  </div>
                ) : status.current_provider ? (
                  <p className="text-sm text-muted-foreground">
                    {t("proxy.panel.currentProvider")}{" "}
                    <span className="font-medium text-foreground">
                      {status.current_provider}
                    </span>
                  </p>
                ) : (
                  <p className="text-sm text-yellow-600 dark:text-yellow-400">
                    {t("proxy.panel.waitingFirstRequest")}
                  </p>
                )}
              </div>

              {/* [5] Logging toggle */}
              <div className="pt-3 border-t border-border">
                <div className="flex items-center justify-between rounded-md border border-border bg-background/60 px-3 py-2">
                  <div className="space-y-0.5">
                    <Label className="text-sm font-medium">
                      {t("proxy.settings.fields.enableLogging.label")}
                    </Label>
                    <p className="text-xs text-muted-foreground">
                      {t("proxy.settings.fields.enableLogging.description")}
                    </p>
                  </div>
                  <Switch
                    checked={globalConfig?.enableLogging ?? true}
                    onCheckedChange={handleLoggingChange}
                    disabled={updateGlobalConfig.isPending}
                  />
                </div>
              </div>

              {/* [6] Provider queues */}
              {(claudeQueue.length > 0 ||
                codexQueue.length > 0 ||
                geminiQueue.length > 0 ||
                grokQueue.length > 0) && (
                <div className="pt-3 border-t border-border space-y-3">
                  <div className="flex items-center gap-2">
                    <ListOrdered className="h-3.5 w-3.5 text-muted-foreground" />
                    <p className="text-xs text-muted-foreground">
                      {t("proxy.failoverQueue.title")}
                    </p>
                  </div>

                  {claudeQueue.length > 0 && (
                    <ProviderQueueGroup
                      appType="claude"
                      appLabel="Claude"
                      targets={claudeQueue.map((item) => ({
                        id: item.providerId,
                        name: item.providerName,
                      }))}
                      status={status}
                    />
                  )}

                  {codexQueue.length > 0 && (
                    <ProviderQueueGroup
                      appType="codex"
                      appLabel="Codex"
                      targets={codexQueue.map((item) => ({
                        id: item.providerId,
                        name: item.providerName,
                      }))}
                      status={status}
                    />
                  )}

                  {geminiQueue.length > 0 && (
                    <ProviderQueueGroup
                      appType="gemini"
                      appLabel="Gemini"
                      targets={geminiQueue.map((item) => ({
                        id: item.providerId,
                        name: item.providerName,
                      }))}
                      status={status}
                    />
                  )}

                  {grokQueue.length > 0 && (
                    <ProviderQueueGroup
                      appType="grokbuild"
                      appLabel="Grok Build"
                      targets={grokQueue.map((item) => ({
                        id: item.providerId,
                        name: item.providerName,
                      }))}
                      status={status}
                    />
                  )}
                </div>
              )}
            </div>

            {/* [7] Stats cards */}
            <div className="grid gap-3 md:grid-cols-4">
              <StatCard
                icon={<Activity className="h-4 w-4" />}
                label={t("proxy.panel.stats.activeConnections")}
                value={status.active_connections}
              />
              <StatCard
                icon={<TrendingUp className="h-4 w-4" />}
                label={t("proxy.panel.stats.totalRequests")}
                value={status.total_requests}
              />
              <StatCard
                icon={<Clock className="h-4 w-4" />}
                label={t("proxy.panel.stats.successRate")}
                value={`${status.success_rate.toFixed(1)}%`}
                variant={status.success_rate > 90 ? "success" : "warning"}
              />
              <StatCard
                icon={<Clock className="h-4 w-4" />}
                label={t("proxy.panel.stats.uptime")}
                value={formatUptime(status.uptime_seconds)}
              />
            </div>
          </div>
        ) : (
          <div className="space-y-6">
            {/* [8] Basic settings — address/port (only when stopped) */}
            <div className="rounded-lg border border-border bg-muted/40 p-4 space-y-4">
              <div>
                <h4 className="text-sm font-semibold">
                  {t("proxy.settings.basic.title")}
                </h4>
                <p className="text-xs text-muted-foreground">
                  {t("proxy.settings.basic.description")}
                </p>
              </div>

              <div className="grid gap-4 md:grid-cols-2">
                <div className="space-y-2">
                  <Label htmlFor="listen-address">
                    {t("proxy.settings.fields.listenAddress.label")}
                  </Label>
                  <Input
                    id="listen-address"
                    value={listenAddress}
                    onChange={(e) => setListenAddress(e.target.value)}
                    placeholder={t(
                      "proxy.settings.fields.listenAddress.placeholder",
                    )}
                  />
                  <p className="text-xs text-muted-foreground">
                    {t("proxy.settings.fields.listenAddress.description")}
                  </p>
                  {!isLoopbackListenAddress(listenAddress) && (
                    <p
                      role="alert"
                      className="text-xs text-yellow-600 dark:text-yellow-500"
                    >
                      {t("proxy.settings.fields.listenAddress.lanWarning")}
                    </p>
                  )}
                </div>

                <div className="space-y-2">
                  <Label htmlFor="listen-port">
                    {t("proxy.settings.fields.listenPort.label")}
                  </Label>
                  <Input
                    id="listen-port"
                    type="number"
                    value={listenPort}
                    onChange={(e) => setListenPort(e.target.value)}
                    placeholder={t(
                      "proxy.settings.fields.listenPort.placeholder",
                    )}
                  />
                  <p className="text-xs text-muted-foreground">
                    {t("proxy.settings.fields.listenPort.description")}
                  </p>
                </div>
              </div>

              <div className="flex justify-end">
                <Button
                  size="sm"
                  onClick={handleSaveBasicConfig}
                  disabled={updateGlobalConfig.isPending}
                >
                  {updateGlobalConfig.isPending ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      {t("common.saving")}
                    </>
                  ) : (
                    <>
                      <Save className="mr-2 h-4 w-4" />
                      {t("common.save")}
                    </>
                  )}
                </Button>
              </div>
            </div>

            {/* Stopped hint */}
            <div className="text-center py-6 text-muted-foreground">
              <div className="mx-auto w-16 h-16 rounded-full bg-muted flex items-center justify-center mb-4">
                <Server className="h-8 w-8" />
              </div>
              <p className="text-base font-medium text-foreground mb-1">
                {t("proxy.panel.stoppedTitle")}
              </p>
              <p className="text-sm text-muted-foreground">
                {t("proxy.panel.stoppedDescription")}
              </p>
            </div>
          </div>
        )}
      </section>
    </>
  );
}

interface StatCardProps {
  icon: React.ReactNode;
  label: string;
  value: string | number;
  variant?: "default" | "success" | "warning";
}

function StatCard({ icon, label, value, variant = "default" }: StatCardProps) {
  const variantStyles = {
    default: "",
    success: "border-green-500/40 bg-green-500/5",
    warning: "border-yellow-500/40 bg-yellow-500/5",
  };

  return (
    <div
      className={`rounded-lg border border-border bg-card/60 p-4 text-sm text-muted-foreground ${variantStyles[variant]}`}
    >
      <div className="flex items-center gap-2 text-muted-foreground mb-2">
        {icon}
        <span className="text-xs">{label}</span>
      </div>
      <p className="text-xl font-semibold text-foreground">{value}</p>
    </div>
  );
}

interface ProviderQueueGroupProps {
  appType: string;
  appLabel: string;
  targets: Array<{
    id: string;
    name: string;
  }>;
  status: ProxyStatus;
}

function ProviderQueueGroup({
  appType,
  appLabel,
  targets,
  status,
}: ProviderQueueGroupProps) {
  // 查找该应用类型的当前活跃目标
  const activeTarget = status.active_targets?.find(
    (t) => t.app_type === appType,
  );

  return (
    <div className="space-y-2">
      {/* 应用类型标题 */}
      <div className="flex items-center gap-2 px-2">
        <span className="text-xs font-semibold text-foreground/80">
          {appLabel}
        </span>
        <div className="flex-1 h-px bg-border/50" />
      </div>

      {/* 供应商列表 */}
      <div className="space-y-1.5">
        {targets.map((target, index) => (
          <ProviderQueueItem
            key={target.id}
            provider={target}
            priority={index + 1}
            appType={appType}
            isCurrent={activeTarget?.provider_id === target.id}
          />
        ))}
      </div>
    </div>
  );
}

interface ProviderQueueItemProps {
  provider: {
    id: string;
    name: string;
  };
  priority: number;
  appType: string;
  isCurrent: boolean;
}

function ProviderQueueItem({
  provider,
  priority,
  appType,
  isCurrent,
}: ProviderQueueItemProps) {
  const { t } = useTranslation();
  const { data: health } = useProviderHealth(provider.id, appType);

  return (
    <div
      className={`flex items-center justify-between rounded-md border px-3 py-2 text-sm transition-colors ${
        isCurrent
          ? "border-primary/40 bg-primary/10 text-primary font-medium"
          : "border-border bg-background/60"
      }`}
    >
      <div className="flex items-center gap-2">
        <span
          className={`flex-shrink-0 flex items-center justify-center w-5 h-5 rounded-full text-xs font-bold ${
            isCurrent
              ? "bg-primary text-primary-foreground"
              : "bg-muted text-muted-foreground"
          }`}
        >
          {priority}
        </span>
        <span className={isCurrent ? "" : "text-foreground"}>
          {provider.name}
        </span>
        {isCurrent && (
          <span className="text-xs px-1.5 py-0.5 rounded bg-primary/20 text-primary">
            {t("provider.inUse")}
          </span>
        )}
      </div>
      {/* 健康徽章 */}
      <ProviderHealthBadge
        consecutiveFailures={health?.consecutive_failures ?? 0}
        isHealthy={health?.is_healthy}
      />
    </div>
  );
}
