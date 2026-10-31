import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { FileDown, FileUp } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ALL_APP_IDS, APP_REGISTRY, appsWith } from "@/config/apps";
import { queryKeys } from "@/lib/query/keys";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  shareBundleApi,
  type BundlePreview,
  type BundlePreviewItem,
} from "./api";
import { defaultSelection, pointerLabel } from "./helpers";

const onError = (error: unknown) =>
  toast.error(extractErrorMessage(error) || String(error));

/**
 * Export a JSON bundle of providers / MCP servers / prompts (secrets
 * stripped by default) and import one with a merge preview.
 */
export function ShareBundlePanel() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [providerApps, setProviderApps] = useState<Set<string>>(
    () => new Set(["claude"]),
  );
  const [includeMcp, setIncludeMcp] = useState(true);
  const [includePrompts, setIncludePrompts] = useState(false);
  const [stripSecrets, setStripSecrets] = useState(true);
  const [preview, setPreview] = useState<BundlePreview | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [overwrite, setOverwrite] = useState(false);
  const [secrets, setSecrets] = useState<
    Record<string, Record<string, string>>
  >({});

  const exportBundle = useMutation({
    mutationFn: () =>
      shareBundleApi.export({
        providerApps: [...providerApps],
        mcp: includeMcp,
        promptApps: includePrompts ? appsWith("prompts") : [],
        stripSecrets,
      }),
    onSuccess: (summary) => {
      if (!summary.path) return;
      toast.success(
        t("workbench.bundle.exported", {
          defaultValue:
            "Saved {{providers}} providers, {{mcp}} MCP servers and {{prompts}} prompts ({{secrets}} secrets removed)",
          providers: summary.providers,
          mcp: summary.mcpServers,
          prompts: summary.prompts,
          secrets: summary.secretsStripped,
        }),
      );
    },
    onError,
  });

  const loadPreview = useMutation({
    mutationFn: shareBundleApi.preview,
    onSuccess: (p) => {
      if (!p) return;
      setPreview(p);
      setSelected(defaultSelection(p));
      setSecrets({});
    },
    onError,
  });

  const apply = useMutation({
    mutationFn: () =>
      shareBundleApi.apply({ keys: [...selected], overwrite, secrets }),
    onSuccess: async (result) => {
      setPreview(null);
      const summary = t("workbench.bundle.applied", {
        defaultValue:
          "Imported: {{added}} added, {{updated}} updated, {{skipped}} skipped",
        added: result.added,
        updated: result.updated,
        skipped: result.skipped,
      });
      if (result.errors.length > 0) {
        toast.error(`${summary}\n${result.errors.join("\n")}`);
      } else {
        toast.success(summary);
      }
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.providers.all }),
        queryClient.invalidateQueries({ queryKey: queryKeys.mcp.root }),
        queryClient.invalidateQueries({ queryKey: queryKeys.prompts.all }),
      ]);
    },
    onError,
  });

  const toggle = (set: Set<string>, value: string, on: boolean) => {
    const next = new Set(set);
    if (on) next.add(value);
    else next.delete(value);
    return next;
  };

  const statusLabel = (item: BundlePreviewItem) =>
    item.status === "new"
      ? t("workbench.bundle.statusNew", { defaultValue: "New" })
      : item.status === "identical"
        ? t("workbench.bundle.statusIdentical", { defaultValue: "Unchanged" })
        : t("workbench.bundle.statusConflict", {
            defaultValue: "Exists — differs",
          });

  return (
    <div className="space-y-4">
      <section className="space-y-2">
        <h4 className="text-sm font-medium">
          {t("workbench.bundle.exportTitle", {
            defaultValue: "Export a share bundle",
          })}
        </h4>
        <div className="flex flex-wrap gap-3">
          {ALL_APP_IDS.map((app) => (
            <label key={app} className="flex items-center gap-1.5 text-sm">
              <Checkbox
                checked={providerApps.has(app)}
                onCheckedChange={(v) =>
                  setProviderApps((s) => toggle(s, app, v === true))
                }
              />
              {APP_REGISTRY[app].label}
            </label>
          ))}
        </div>
        <div className="flex flex-wrap gap-4 text-sm">
          <label className="flex items-center gap-1.5">
            <Checkbox
              checked={includeMcp}
              onCheckedChange={(v) => setIncludeMcp(v === true)}
            />
            {t("workbench.bundle.mcp", { defaultValue: "MCP servers" })}
          </label>
          <label className="flex items-center gap-1.5">
            <Checkbox
              checked={includePrompts}
              onCheckedChange={(v) => setIncludePrompts(v === true)}
            />
            {t("workbench.bundle.prompts", { defaultValue: "Prompts" })}
          </label>
          <label className="flex items-center gap-1.5">
            <Checkbox
              checked={stripSecrets}
              onCheckedChange={(v) => setStripSecrets(v === true)}
            />
            {t("workbench.bundle.stripSecrets", {
              defaultValue: "Remove API keys and tokens",
            })}
          </label>
        </div>
        {!stripSecrets && (
          <p className="text-xs text-amber-600 dark:text-amber-400">
            {t("workbench.bundle.secretsWarning", {
              defaultValue:
                "The file will contain your API keys. Only share it over a trusted channel.",
            })}
          </p>
        )}
        <Button
          size="sm"
          onClick={() => exportBundle.mutate()}
          disabled={
            exportBundle.isPending ||
            (providerApps.size === 0 && !includeMcp && !includePrompts)
          }
        >
          <FileDown className="mr-1 h-3.5 w-3.5" />
          {t("workbench.bundle.export", { defaultValue: "Export…" })}
        </Button>
      </section>

      <section className="space-y-2">
        <h4 className="text-sm font-medium">
          {t("workbench.bundle.importTitle", {
            defaultValue: "Import a share bundle",
          })}
        </h4>
        <p className="text-xs text-muted-foreground">
          {t("workbench.bundle.importDescription", {
            defaultValue:
              "Items are merged into your data; nothing is activated. Existing items are only replaced when you allow it, and their keys are kept.",
          })}
        </p>
        <Button
          size="sm"
          variant="outline"
          onClick={() => loadPreview.mutate()}
          disabled={loadPreview.isPending}
        >
          <FileUp className="mr-1 h-3.5 w-3.5" />
          {t("workbench.bundle.chooseFile", { defaultValue: "Choose file…" })}
        </Button>
        {preview && (
          <div className="space-y-2 rounded-md border border-border/60 p-3">
            <ul className="max-h-72 space-y-2 overflow-y-auto">
              {preview.items.map((item) => (
                <li key={item.key} className="space-y-1 text-sm">
                  <label className="flex items-center gap-2">
                    <Checkbox
                      checked={selected.has(item.key)}
                      onCheckedChange={(v) =>
                        setSelected((s) => toggle(s, item.key, v === true))
                      }
                    />
                    <span className="font-medium">{item.name}</span>
                    <span className="text-xs text-muted-foreground">
                      {item.kind}
                      {item.app ? ` · ${item.app}` : ""}
                    </span>
                    <Badge
                      variant={
                        item.status === "conflict" ? "destructive" : "secondary"
                      }
                    >
                      {statusLabel(item)}
                    </Badge>
                  </label>
                  {selected.has(item.key) &&
                    item.missingSecrets.map((ptr) => {
                      const id = `${item.key}-${ptr}`;
                      return (
                        <div key={ptr} className="ml-6 flex items-center gap-2">
                          <Label htmlFor={id} className="text-xs">
                            {pointerLabel(ptr)}
                          </Label>
                          <Input
                            id={id}
                            type="password"
                            autoComplete="off"
                            className="h-7 max-w-xs"
                            placeholder={t("workbench.bundle.enterKey", {
                              defaultValue: "Enter key (optional)",
                            })}
                            value={secrets[item.key]?.[ptr] ?? ""}
                            onChange={(e) =>
                              setSecrets((s) => ({
                                ...s,
                                [item.key]: {
                                  ...s[item.key],
                                  [ptr]: e.target.value,
                                },
                              }))
                            }
                          />
                        </div>
                      );
                    })}
                </li>
              ))}
            </ul>
            <label className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={overwrite}
                onCheckedChange={(v) => setOverwrite(v === true)}
              />
              {t("workbench.bundle.overwrite", {
                defaultValue: "Replace existing items that differ",
              })}
            </label>
            <div className="flex justify-end gap-2">
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setPreview(null)}
              >
                {t("common.cancel", { defaultValue: "Cancel" })}
              </Button>
              <Button
                size="sm"
                onClick={() => apply.mutate()}
                disabled={apply.isPending || selected.size === 0}
              >
                {t("workbench.bundle.import", {
                  defaultValue: "Import {{count}} items",
                  count: selected.size,
                })}
              </Button>
            </div>
          </div>
        )}
      </section>
    </div>
  );
}
