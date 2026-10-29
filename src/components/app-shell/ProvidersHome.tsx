import { motion, AnimatePresence } from "framer-motion";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { useAppCaps } from "@/entities/app/registry";
import type { useProviderShellActions } from "@/hooks/useProviderShellActions";
import { ProviderList } from "@/components/providers/ProviderList";
import type { ProviderConfirmKind } from "./AppConfirmDialogs";

export interface ProvidersHomeProps {
  activeApp: AppId;
  providers: Record<string, Provider>;
  currentProviderId: string;
  isLoading: boolean;
  isProxyRunning: boolean;
  isProxyTakeover: boolean;
  activeProviderId?: string;
  onSwitch: (provider: Provider) => void;
  /** 正在切换中的供应商 ID（切换在途时其余卡片的切换按钮禁用） */
  switchingProviderId?: string;
  /** OpenClaw：设为默认模型 */
  onSetAsDefaultModel: (provider: Provider) => void;
  shellActions: ReturnType<typeof useProviderShellActions>;
  onEdit: (provider: Provider) => void;
  onConfigureUsage: (provider: Provider) => void;
  onCreate: () => void;
  onRequestConfirm: (provider: Provider, action: ProviderConfirmKind) => void;
}

/** 首页：当前应用的供应商列表，切换应用时淡入淡出 */
export function ProvidersHome({
  activeApp,
  shellActions,
  onSwitch,
  onSetAsDefaultModel,
  onRequestConfirm,
  ...listProps
}: ProvidersHomeProps) {
  const activeAppCaps = useAppCaps(activeApp);

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
      <div className="flex-1 overflow-y-auto overflow-x-hidden pb-12 px-1">
        <AnimatePresence mode="wait">
          <motion.div
            key={activeApp}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15 }}
            className="space-y-4"
          >
            <ProviderList
              {...listProps}
              appId={activeApp}
              onSwitch={onSwitch}
              onDelete={(provider) => onRequestConfirm(provider, "delete")}
              onRemoveFromConfig={
                activeAppCaps.additive
                  ? (provider) => onRequestConfirm(provider, "remove")
                  : undefined
              }
              onDisableOmo={
                activeApp === "opencode" ? shellActions.disableOmo : undefined
              }
              onDisableOmoSlim={
                activeApp === "opencode"
                  ? shellActions.disableOmoSlim
                  : undefined
              }
              onDuplicate={shellActions.duplicateProvider}
              onOpenWebsite={shellActions.openWebsite}
              onOpenTerminal={
                activeApp === "claude" ? shellActions.openTerminal : undefined
              }
              onSetAsDefault={
                activeApp === "openclaw"
                  ? onSetAsDefaultModel
                  : activeApp === "hermes"
                    ? onSwitch
                    : undefined
              }
            />
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  );
}
