import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ArrowRight, FolderOpen, Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useApplyProfileMutation } from "@/lib/query/profiles";
import type { DeepLinkApplyProfileRequest } from "@/lib/api/deeplink";

/**
 * 深链接应用项目确认框：`ccswitch://v1/apply?profile=…&scope=…`
 *
 * 后端只解析校验，真正的应用必须经用户确认；应用本身走与头部切换器相同的
 * mutation（含自动保存旧项目、失效缓存与 toast）。
 */
export function DeepLinkApplyProfileDialog() {
  const { t } = useTranslation();
  const [request, setRequest] = useState<DeepLinkApplyProfileRequest | null>(
    null,
  );
  const applyMutation = useApplyProfileMutation();

  useEffect(() => {
    const unlisten = listen<DeepLinkApplyProfileRequest>(
      "deeplink-apply-profile",
      (event) => {
        const payload = event.payload;
        if (payload.alreadyCurrent) {
          toast.info(
            t("deeplink.applyProfileAlreadyCurrent", {
              defaultValue: "{{profile}} 已经是 {{scope}} 的当前项目",
              profile: payload.profileName,
              scope: t(`apps.${payload.scope}`, {
                defaultValue: payload.scope,
              }),
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

  const scopeLabel = request
    ? t(`apps.${request.scope}`, { defaultValue: request.scope })
    : "";

  const handleConfirm = async () => {
    if (!request) return;
    try {
      await applyMutation.mutateAsync({
        id: request.profileId,
        scope: request.scope,
      });
      setRequest(null);
    } catch {
      // useApplyProfileMutation already reports the failure; keep the dialog open
    }
  };

  return (
    <Dialog
      open={request !== null}
      onOpenChange={(open) => {
        if (!open && !applyMutation.isPending) setRequest(null);
      }}
    >
      <DialogContent
        className="sm:max-w-md"
        data-testid="deeplink-apply-profile-dialog"
      >
        <DialogHeader>
          <DialogTitle>
            {t("deeplink.applyProfileTitle", {
              defaultValue: "通过链接应用项目",
            })}
          </DialogTitle>
          <DialogDescription>
            {t("deeplink.applyProfileMessage", {
              defaultValue:
                "是否在 {{scope}} 上应用项目 {{profile}}？当前配置会先自动保存到原项目。",
              scope: scopeLabel,
              profile: request?.profileName ?? "",
            })}
          </DialogDescription>
        </DialogHeader>
        {request && (
          <div className="flex items-center gap-2 rounded-lg border border-border/60 bg-muted/30 px-3 py-2 text-sm">
            <FolderOpen className="h-4 w-4 shrink-0 text-muted-foreground" />
            <span
              className="truncate text-muted-foreground"
              data-testid="deeplink-apply-profile-current"
            >
              {request.currentProfileName ??
                request.currentProfileId ??
                t("deeplink.applyProfileNoCurrent", {
                  defaultValue: "未使用项目",
                })}
            </span>
            <ArrowRight className="h-4 w-4 shrink-0 text-muted-foreground" />
            <span className="truncate font-medium">{request.profileName}</span>
          </div>
        )}
        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            disabled={applyMutation.isPending}
            onClick={() => setRequest(null)}
          >
            {t("common.cancel", { defaultValue: "取消" })}
          </Button>
          <Button
            type="button"
            disabled={applyMutation.isPending}
            onClick={() => void handleConfirm()}
          >
            {applyMutation.isPending && (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            )}
            {t("deeplink.applyProfileConfirm", { defaultValue: "应用" })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
