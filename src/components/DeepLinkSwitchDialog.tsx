import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { ArrowRight, Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { providersApi } from "@/lib/api/providers";
import type { DeepLinkSwitchRequest } from "@/lib/api/deeplink";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * 深链接切换确认框：`ccswitch://v1/switch?app=…&provider=…`
 *
 * 后端只解析并校验目标供应商，真正的切换必须经用户在这里确认，
 * 避免网页上的一个链接就能悄悄改写实时配置。
 */
export function DeepLinkSwitchDialog() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [request, setRequest] = useState<DeepLinkSwitchRequest | null>(null);
  const [isSwitching, setIsSwitching] = useState(false);

  useEffect(() => {
    const unlisten = listen<DeepLinkSwitchRequest>(
      "deeplink-switch",
      (event) => {
        const payload = event.payload;
        if (payload.alreadyCurrent) {
          toast.info(
            t("deeplink.switchAlreadyCurrent", {
              defaultValue: "{{provider}} 已经是 {{app}} 的当前供应商",
              provider: payload.providerName,
              app: t(`apps.${payload.app}`, { defaultValue: payload.app }),
            }),
          );
          return;
        }
        setRequest(payload);
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

  const appLabel = request
    ? t(`apps.${request.app}`, { defaultValue: request.app })
    : "";

  const handleConfirm = async () => {
    if (!request) return;
    setIsSwitching(true);
    try {
      const result = await providersApi.switch(request.providerId, request.app);
      await queryClient.invalidateQueries({
        queryKey: ["providers", request.app],
      });
      await queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      toast.success(
        t("deeplink.switchSuccess", {
          defaultValue: "{{app}} 已切换到 {{provider}}",
          app: appLabel,
          provider: request.providerName,
        }),
      );
      for (const warning of result?.warnings ?? []) {
        toast.warning(warning);
      }
      setRequest(null);
    } catch (error) {
      toast.error(t("deeplink.switchFailed", { defaultValue: "切换失败" }), {
        description: extractErrorMessage(error) || undefined,
      });
    } finally {
      setIsSwitching(false);
    }
  };

  return (
    <Dialog
      open={request !== null}
      onOpenChange={(open) => {
        if (!open && !isSwitching) setRequest(null);
      }}
    >
      <DialogContent
        className="sm:max-w-md"
        data-testid="deeplink-switch-dialog"
      >
        <DialogHeader>
          <DialogTitle>
            {t("deeplink.switchTitle", { defaultValue: "通过链接切换供应商" })}
          </DialogTitle>
          <DialogDescription>
            {t("deeplink.switchMessage", {
              defaultValue: "是否将 {{app}} 切换到 {{provider}}？",
              app: appLabel,
              provider: request?.providerName ?? "",
            })}
          </DialogDescription>
        </DialogHeader>
        {request && (
          <div className="flex items-center gap-2 rounded-lg border border-border/60 bg-muted/30 px-3 py-2 text-sm">
            <span
              className="truncate text-muted-foreground"
              data-testid="deeplink-switch-current"
            >
              {request.currentProviderName ??
                request.currentProviderId ??
                t("deeplink.switchNoCurrent", { defaultValue: "未选择供应商" })}
            </span>
            <ArrowRight className="h-4 w-4 shrink-0 text-muted-foreground" />
            <span className="truncate font-medium">{request.providerName}</span>
          </div>
        )}
        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            disabled={isSwitching}
            onClick={() => setRequest(null)}
          >
            {t("common.cancel", { defaultValue: "取消" })}
          </Button>
          <Button
            type="button"
            disabled={isSwitching}
            onClick={() => void handleConfirm()}
          >
            {isSwitching && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            {t("deeplink.switchConfirm", { defaultValue: "切换" })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
