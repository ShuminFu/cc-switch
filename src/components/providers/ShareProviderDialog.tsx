import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Copy, Link2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { copyText } from "@/lib/clipboard";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  buildProviderDeepLink,
  type ShareableAppId,
} from "@/utils/providerDeepLink";
import type { Provider } from "@/types";

interface ShareProviderDialogProps {
  provider: Provider;
  appId: ShareableAppId;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * 把已有供应商生成为 ccswitch:// 深链，供团队 / 多设备一键导入。
 * 默认不带 API Key；勾选后密钥会同时出现在 apiKey 参数和附带的 config 里。
 */
export function ShareProviderDialog({
  provider,
  appId,
  open,
  onOpenChange,
}: ShareProviderDialogProps) {
  const { t } = useTranslation();
  const [includeApiKey, setIncludeApiKey] = useState(false);
  const [isCopying, setIsCopying] = useState(false);

  const link = useMemo(
    () => buildProviderDeepLink(provider, appId, { includeApiKey }),
    [provider, appId, includeApiKey],
  );

  const handleCopy = async () => {
    setIsCopying(true);
    try {
      await copyText(link);
      toast.success(
        t("provider.shareDialog.copied", { defaultValue: "链接已复制" }),
      );
    } catch (error) {
      toast.error(
        t("provider.shareDialog.copyFailed", {
          defaultValue: "复制失败：{{detail}}",
          detail: extractErrorMessage(error),
        }),
      );
    } finally {
      setIsCopying(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Link2 className="h-4 w-4" />
            {t("provider.shareDialog.title", {
              defaultValue: "分享供应商 {{name}}",
              name: provider.name,
            })}
          </DialogTitle>
          <DialogDescription>
            {t("provider.shareDialog.description", {
              defaultValue:
                "生成 ccswitch:// 深链，接收方在安装了 CC Switch 的设备上打开即可一键导入。",
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="flex items-start gap-3 rounded-lg border border-border/60 bg-muted/30 p-3">
            <Checkbox
              id="share-provider-include-key"
              checked={includeApiKey}
              onCheckedChange={(checked) => setIncludeApiKey(checked === true)}
            />
            <div className="space-y-1">
              <Label htmlFor="share-provider-include-key">
                {t("provider.shareDialog.includeApiKey", {
                  defaultValue: "在链接中包含 API Key",
                })}
              </Label>
              <p className="text-xs text-muted-foreground">
                {t("provider.shareDialog.includeApiKeyHint", {
                  defaultValue:
                    "密钥会明文写入链接，只分享给允许使用该密钥的人。不勾选时接收方导入后需自行填写密钥。",
                })}
              </p>
            </div>
          </div>

          <textarea
            readOnly
            aria-label={t("provider.shareDialog.linkLabel", {
              defaultValue: "深链",
            })}
            value={link}
            rows={5}
            className="w-full resize-none rounded-md border border-border bg-background p-2 font-mono text-xs break-all"
            onFocus={(event) => event.currentTarget.select()}
          />
          <p className="text-xs text-muted-foreground">
            {t("provider.shareDialog.lengthHint", {
              defaultValue: "{{count}} 个字符",
              count: link.length,
            })}
          </p>
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
          >
            {t("common.close", { defaultValue: "关闭" })}
          </Button>
          <Button
            type="button"
            onClick={() => void handleCopy()}
            disabled={isCopying}
            className="gap-1.5"
          >
            <Copy className="h-4 w-4" />
            {t("provider.shareDialog.copyLink", { defaultValue: "复制链接" })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
