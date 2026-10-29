import { useCallback, useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { FileDiff } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { providersApi, type AppId } from "@/lib/api";
import type { LiveFilePreview } from "@/shared/bindings";
import type { Provider } from "@/types";
import {
  collapseUnchanged,
  diffLines,
  hasChanges,
  type DiffLine,
  type DiffRow,
} from "./liveDiff";

interface LiveDiffPreviewDialogProps {
  isOpen: boolean;
  /** 仅包含有改动的文件 */
  files: LiveFilePreview[];
  onConfirm: () => void;
  onCancel: () => void;
}

const ROW_TONE: Record<DiffRow["kind"], { left: string; right: string }> = {
  same: { left: "", right: "" },
  removed: { left: "bg-red-500/10", right: "bg-muted/30" },
  added: { left: "bg-muted/30", right: "bg-green-500/10" },
  changed: { left: "bg-red-500/10", right: "bg-green-500/10" },
};

function DiffCells({ line, tone }: { line?: DiffLine; tone: string }) {
  return (
    <>
      <td
        className={cn(
          "w-10 select-none border-r border-border-default px-2 text-right align-top text-muted-foreground",
          tone,
        )}
      >
        {line?.lineNo ?? ""}
      </td>
      <td
        className={cn(
          "w-1/2 whitespace-pre-wrap break-all px-2 align-top",
          tone,
        )}
      >
        {line?.text ?? ""}
      </td>
    </>
  );
}

function FileDiffTable({ file }: { file: LiveFilePreview }) {
  const { t } = useTranslation();
  const segments = useMemo(
    () => collapseUnchanged(diffLines(file.before, file.after)),
    [file.before, file.after],
  );

  const status =
    file.before === null
      ? t("providerForm.liveDiff.newFile")
      : file.after === null
        ? t("providerForm.liveDiff.deletedFile")
        : null;

  return (
    <section
      className="overflow-hidden rounded-md border border-border-default"
      data-testid="live-diff-file"
    >
      <h3 className="flex items-center gap-2 border-b border-border-default bg-muted/40 px-3 py-2 font-mono text-xs">
        <FileDiff className="h-3.5 w-3.5 shrink-0" aria-hidden />
        <span className="break-all">{file.path}</span>
        {status && (
          <span className="ml-auto shrink-0 rounded bg-muted px-1.5 py-0.5 font-sans text-[11px] text-muted-foreground">
            {status}
          </span>
        )}
      </h3>
      <table
        className="w-full table-fixed border-collapse font-mono text-xs"
        aria-label={file.path}
      >
        <thead>
          <tr className="border-b border-border-default text-left text-muted-foreground">
            <th colSpan={2} className="px-2 py-1 font-sans font-medium">
              {t("providerForm.liveDiff.current")}
            </th>
            <th
              colSpan={2}
              className="border-l border-border-default px-2 py-1 font-sans font-medium"
            >
              {t("providerForm.liveDiff.afterSave")}
            </th>
          </tr>
        </thead>
        <tbody>
          {segments.map((segment, index) =>
            segment.type === "skipped" ? (
              <tr key={`skip-${index}`} className="bg-muted/20">
                <td
                  colSpan={4}
                  className="px-2 py-0.5 text-center font-sans text-[11px] text-muted-foreground"
                >
                  {t("providerForm.liveDiff.unchangedLines", {
                    count: segment.count,
                  })}
                </td>
              </tr>
            ) : (
              segment.rows.map((row, rowIndex) => (
                <tr key={`${index}-${rowIndex}`} data-diff-kind={row.kind}>
                  <DiffCells line={row.left} tone={ROW_TONE[row.kind].left} />
                  <DiffCells
                    line={row.right}
                    tone={cn(
                      "border-l border-border-default",
                      ROW_TONE[row.kind].right,
                    )}
                  />
                </tr>
              ))
            ),
          )}
        </tbody>
      </table>
    </section>
  );
}

/** 保存前并排展示 live 配置文件将发生的改动，确认后才保存 */
export function LiveDiffPreviewDialog({
  isOpen,
  files,
  onConfirm,
  onCancel,
}: LiveDiffPreviewDialogProps) {
  const { t } = useTranslation();
  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
    >
      <DialogContent className="max-w-5xl" zIndex="alert">
        <DialogHeader>
          <DialogTitle>{t("providerForm.liveDiff.title")}</DialogTitle>
          <DialogDescription>
            {t("providerForm.liveDiff.description")}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-6 py-4">
          {files.map((file) => (
            <FileDiffTable key={file.path} file={file} />
          ))}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            {t("providerForm.liveDiff.back")}
          </Button>
          <Button data-testid="live-diff-confirm" onClick={onConfirm}>
            {t("providerForm.liveDiff.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export interface LiveDiffPreviewGate {
  /**
   * 保存前调用：保存会改动 live 配置时弹出 diff 并等待用户选择，
   * 返回是否继续保存。无改动、不写 live 或预览失败时直接返回 `true`。
   */
  confirmBeforeSave: (
    provider: Provider,
    originalId?: string,
  ) => Promise<boolean>;
  /** diff 对话框，需渲染在面板内部 */
  dialog: ReactNode;
}

export function useLiveDiffPreview(appId: AppId): LiveDiffPreviewGate {
  const [pending, setPending] = useState<{
    files: LiveFilePreview[];
    resolve: (proceed: boolean) => void;
  } | null>(null);

  const confirmBeforeSave = useCallback(
    async (provider: Provider, originalId?: string) => {
      let files: LiveFilePreview[];
      try {
        const preview = await providersApi.previewLiveWrite(
          provider,
          appId,
          originalId,
        );
        files = preview.writesLive
          ? preview.files.filter((file) =>
              hasChanges(diffLines(file.before, file.after)),
            )
          : [];
      } catch (error) {
        // 预览只是辅助：失败时照常保存，真正的错误由保存流程报告
        console.warn("[ProviderForm] live diff preview failed:", error);
        return true;
      }
      if (files.length === 0) return true;
      return await new Promise<boolean>((resolve) => {
        setPending({ files, resolve });
      });
    },
    [appId],
  );

  const settle = (proceed: boolean) => {
    pending?.resolve(proceed);
    setPending(null);
  };

  const dialog = (
    <LiveDiffPreviewDialog
      isOpen={pending !== null}
      files={pending?.files ?? []}
      onConfirm={() => settle(true)}
      onCancel={() => settle(false)}
    />
  );

  return { confirmBeforeSave, dialog };
}
