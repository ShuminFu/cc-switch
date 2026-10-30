import { useQuery } from "@tanstack/react-query";
import { Check, Copy, Loader2, Minus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { appRegistryQueryKey } from "@/entities/app/registry";
import {
  appsApi,
  type BackendAppCaps,
  type BackendAppDescriptor,
} from "@/lib/api/apps";
import { copyText } from "@/lib/clipboard";
import { extractErrorMessage } from "@/utils/errorUtils";

/** 矩阵展示的能力列（顺序即列顺序）；托盘分区属于内部细节，不展示 */
export const CAPABILITY_COLUMNS = [
  "mcp",
  "skills",
  "prompts",
  "proxyTakeover",
  "usage",
  "sessionResume",
] as const satisfies readonly (keyof BackendAppCaps)[];

type CapabilityColumn = (typeof CAPABILITY_COLUMNS)[number];

const COLUMN_LABEL_KEYS = {
  mcp: "settings.advanced.capabilityMatrix.caps.mcp",
  skills: "settings.advanced.capabilityMatrix.caps.skills",
  prompts: "settings.advanced.capabilityMatrix.caps.prompts",
  proxyTakeover: "settings.advanced.capabilityMatrix.caps.proxyTakeover",
  usage: "settings.advanced.capabilityMatrix.caps.usage",
  sessionResume: "settings.advanced.capabilityMatrix.caps.sessionResume",
} as const satisfies Record<CapabilityColumn, string>;

/**
 * 生成可粘贴到支持工单的纯文本矩阵（Markdown 表格）。
 * 刻意不做本地化：应用 ID 与能力名保持后端字段名，便于跨语言排查。
 */
export function formatCapabilityMatrix(
  registry: readonly BackendAppDescriptor[],
): string {
  const header = ["app", ...CAPABILITY_COLUMNS];
  const rows = registry.map((app) => [
    app.id,
    ...CAPABILITY_COLUMNS.map((cap) => (app.caps[cap] ? "yes" : "no")),
  ]);
  const line = (cells: readonly string[]) => `| ${cells.join(" | ")} |`;
  return [
    "CC Switch capability matrix",
    "",
    line(header),
    line(header.map(() => "---")),
    ...rows.map(line),
  ].join("\n");
}

/** 应用 x 能力矩阵，数据来自后端 `get_app_registry`；兼作支持诊断视图 */
export function CapabilityMatrix() {
  const { t } = useTranslation();
  const {
    data: registry,
    isLoading,
    error,
  } = useQuery({
    queryKey: appRegistryQueryKey,
    queryFn: () => appsApi.getRegistry(),
    staleTime: Infinity,
  });

  const handleCopy = async () => {
    if (!registry) return;
    try {
      await copyText(formatCapabilityMatrix(registry));
      toast.success(t("settings.advanced.capabilityMatrix.copied"));
    } catch (e) {
      toast.error(t("settings.advanced.capabilityMatrix.copyFailed"), {
        description: extractErrorMessage(e),
      });
    }
  };

  if (isLoading) {
    return (
      <div className="flex justify-center py-4">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (error || !registry) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {t("settings.advanced.capabilityMatrix.loadFailed")}
      </p>
    );
  }

  const supported = t("settings.advanced.capabilityMatrix.supported");
  const unsupported = t("settings.advanced.capabilityMatrix.unsupported");

  return (
    <div className="space-y-4">
      <div className="flex justify-end">
        <Button variant="outline" size="sm" onClick={handleCopy}>
          <Copy className="h-4 w-4" />
          {t("settings.advanced.capabilityMatrix.copy")}
        </Button>
      </div>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead scope="col">
              {t("settings.advanced.capabilityMatrix.app")}
            </TableHead>
            {CAPABILITY_COLUMNS.map((cap) => (
              <TableHead key={cap} scope="col" className="text-center">
                {t(COLUMN_LABEL_KEYS[cap])}
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {registry.map((app) => (
            <TableRow key={app.id} data-app-id={app.id}>
              <TableHead scope="row" className="font-medium">
                {app.displayName}
              </TableHead>
              {CAPABILITY_COLUMNS.map((cap) => {
                const enabled = app.caps[cap];
                return (
                  <TableCell
                    key={cap}
                    data-cap={cap}
                    data-supported={enabled}
                    className="text-center"
                  >
                    {enabled ? (
                      <Check
                        aria-hidden="true"
                        className="mx-auto h-4 w-4 text-emerald-500"
                      />
                    ) : (
                      <Minus
                        aria-hidden="true"
                        className="mx-auto h-4 w-4 text-muted-foreground"
                      />
                    )}
                    <span className="sr-only">
                      {enabled ? supported : unsupported}
                    </span>
                  </TableCell>
                );
              })}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
