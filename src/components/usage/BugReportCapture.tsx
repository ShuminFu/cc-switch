import { useTranslation } from "react-i18next";
import { Bug, FileDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  BUG_REPORT_CAPTURE_MINUTES,
  useExportBugReport,
  useToggleProxyCapture,
} from "@/lib/query/proxy";
import type { CaptureStatus } from "@/types/proxy";

interface BugReportCaptureToggleProps {
  status?: CaptureStatus;
}

/**
 * 请求日志工具栏中的抓包开关：开启后在限定时间内记录经过协议转换的请求，
 * 到期由后端自动关闭。
 */
export function BugReportCaptureToggle({
  status,
}: BugReportCaptureToggleProps) {
  const { t } = useTranslation();
  const toggle = useToggleProxyCapture();
  const active = status?.active ?? false;
  const minutesLeft = Math.max(1, Math.ceil((status?.remainingSecs ?? 0) / 60));

  return (
    <Button
      type="button"
      size="sm"
      variant={active ? "default" : "outline"}
      className="h-8 text-xs"
      aria-pressed={active}
      disabled={toggle.isPending}
      title={
        active
          ? t("usage.bugReport.captureStopHint")
          : t("usage.bugReport.captureHint", {
              minutes: BUG_REPORT_CAPTURE_MINUTES,
            })
      }
      onClick={() => toggle.mutate(!active)}
    >
      <Bug className="mr-1 h-3.5 w-3.5" />
      {active
        ? t("usage.bugReport.captureActive", {
            minutes: minutesLeft,
          })
        : t("usage.bugReport.captureStart")}
    </Button>
  );
}

interface ExportBugReportButtonProps {
  requestId: string;
}

/** 已抓包的请求行上的"导出缺陷报告"按钮 */
export function ExportBugReportButton({
  requestId,
}: ExportBugReportButtonProps) {
  const { t } = useTranslation();
  const exportReport = useExportBugReport();
  const label = t("usage.bugReport.export");

  return (
    <Button
      type="button"
      size="sm"
      variant="ghost"
      className="h-6 w-6 p-0"
      aria-label={label}
      title={label}
      disabled={exportReport.isPending}
      onClick={() => exportReport.mutate(requestId)}
    >
      <FileDown className="h-3.5 w-3.5" />
    </Button>
  );
}
