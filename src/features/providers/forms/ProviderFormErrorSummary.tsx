import { useEffect, useRef } from "react";
import { useFormContext, type FieldErrors } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { AlertCircle } from "lucide-react";
import type { ProviderFormData } from "@/lib/schemas/provider";

export const PROVIDER_FORM_ERROR_SUMMARY_ID = "provider-form-error-summary";

const fieldErrorMessages = (errors: FieldErrors<ProviderFormData>) =>
  Object.values(errors).flatMap((error) =>
    typeof error?.message === "string" && error.message ? [error.message] : [],
  );

interface ProviderFormErrorSummaryProps {
  /** 提交时检查出的阻断错误（来自 SaveGate） */
  errors: string[];
}

/**
 * 内联错误汇总：列出 schema 字段错误与提交检查出的阻断错误。
 * 只要有任意一条，保存就不会进行；下一次提交时重新检查。
 */
export function ProviderFormErrorSummary({
  errors,
}: ProviderFormErrorSummaryProps) {
  const { t } = useTranslation();
  const {
    formState: { errors: fieldErrors },
  } = useFormContext<ProviderFormData>();
  const containerRef = useRef<HTMLDivElement>(null);

  const messages = Array.from(
    new Set([...fieldErrorMessages(fieldErrors), ...errors]),
  );
  const messagesKey = messages.join("\n");

  // 新的错误出现时滚动到汇总处：对话框里保存按钮在表单外，汇总可能不在视野内
  useEffect(() => {
    if (messagesKey) {
      containerRef.current?.scrollIntoView?.({ block: "nearest" });
    }
  }, [messagesKey]);

  if (messages.length === 0) return null;

  return (
    <div
      ref={containerRef}
      id={PROVIDER_FORM_ERROR_SUMMARY_ID}
      role="alert"
      className="rounded-lg border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive"
    >
      <div className="flex items-center gap-2 font-medium">
        <AlertCircle className="h-4 w-4 shrink-0" aria-hidden="true" />
        {t("providerForm.errorSummary.title")}
      </div>
      <ul className="mt-2 list-disc space-y-1 pl-6">
        {messages.map((message) => (
          <li key={message}>{message}</li>
        ))}
      </ul>
    </div>
  );
}
