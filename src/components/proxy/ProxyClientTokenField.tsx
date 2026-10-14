import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { proxyApi } from "@/lib/api/proxy";
import { extractErrorMessage } from "@/utils/errorUtils";

const MASK = "•".repeat(24);

/**
 * Proxy client token shown when the proxy listens on a non-loopback address.
 * Other devices must send it as their API key; local CLIs need nothing.
 */
export function ProxyClientTokenField() {
  const { t } = useTranslation();
  const [token, setToken] = useState<string | null>(null);
  const [revealed, setRevealed] = useState(false);
  const [busy, setBusy] = useState(false);

  // Load once; `t` is read through a ref so a changing `t` identity cannot
  // re-fetch and overwrite a freshly regenerated token.
  const tRef = useRef(t);
  tRef.current = t;

  useEffect(() => {
    const t = tRef.current;
    let cancelled = false;
    proxyApi
      .getProxyClientToken()
      .then((value) => {
        if (!cancelled) setToken(value);
      })
      .catch((error) => {
        if (!cancelled) {
          toast.error(
            t("proxy.settings.fields.clientToken.loadFailed", {
              error: extractErrorMessage(error),
            }),
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleCopy = async () => {
    if (!token) return;
    try {
      await navigator.clipboard.writeText(token);
      toast.success(t("proxy.settings.fields.clientToken.copied"), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(extractErrorMessage(error));
    }
  };

  const handleRegenerate = async () => {
    setBusy(true);
    try {
      const next = await proxyApi.regenerateProxyClientToken();
      setToken(next);
      toast.success(t("proxy.settings.fields.clientToken.regenerated"), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(
        t("proxy.settings.fields.clientToken.regenerateFailed", {
          error: extractErrorMessage(error),
        }),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-2" data-testid="proxy-client-token">
      <p className="text-xs font-medium">
        {t("proxy.settings.fields.clientToken.label")}
      </p>
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
        <code
          className="flex-1 break-all text-xs bg-background px-3 py-2 rounded border border-border/60"
          data-testid="proxy-client-token-value"
        >
          {token === null ? "…" : revealed ? token : MASK}
        </code>
        <div className="flex gap-2">
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={!token}
            onClick={() => setRevealed((v) => !v)}
          >
            {revealed
              ? t("proxy.settings.fields.clientToken.hide")
              : t("proxy.settings.fields.clientToken.show")}
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={!token}
            onClick={handleCopy}
          >
            {t("common.copy")}
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy || !token}
            onClick={handleRegenerate}
          >
            {t("proxy.settings.fields.clientToken.regenerate")}
          </Button>
        </div>
      </div>
      <p className="text-xs text-muted-foreground">
        {t("proxy.settings.fields.clientToken.description")}
      </p>
    </div>
  );
}
