import { useCallback, useRef, useState } from "react";
import { toast } from "sonner";

/**
 * 保存闸门：所有供应商编辑器共用的保存前置检查状态。
 *
 * - 阻断错误（B 类）：`block()` 记录后保存不会进行；错误在表单内联汇总
 *   （见 ProviderFormErrorSummary）并同时 toast 提示。每次提交前清空，
 *   下一次提交重新校验。
 * - 软性问题（A 类）：`requestConfirm()` 打开"仍要保存"确认框，确认后才执行。
 */
export interface SaveGate {
  blockingErrors: string[];
  block: (message: string) => void;
  clearBlocking: () => void;
  softIssues: string[] | null;
  requestConfirm: (issues: string[], proceed: () => Promise<void>) => void;
  isConfirmSubmitting: boolean;
  confirm: () => Promise<void>;
  cancelConfirm: () => void;
}

export function useSaveGate(): SaveGate {
  const [blockingErrors, setBlockingErrors] = useState<string[]>([]);
  const [softIssues, setSoftIssues] = useState<string[] | null>(null);
  // 确认框走的提交路径绕过了 react-hook-form 的 isSubmitting，单独追踪
  const [isConfirmSubmitting, setIsConfirmSubmitting] = useState(false);
  const pendingRef = useRef<(() => Promise<void>) | null>(null);
  const confirmingRef = useRef(false);

  const block = useCallback((message: string) => {
    toast.error(message);
    setBlockingErrors((prev) =>
      prev.includes(message) ? prev : [...prev, message],
    );
  }, []);

  const clearBlocking = useCallback(() => {
    setBlockingErrors((prev) => (prev.length === 0 ? prev : []));
  }, []);

  const resetConfirm = useCallback(() => {
    pendingRef.current = null;
    setSoftIssues(null);
  }, []);

  const requestConfirm = useCallback(
    (issues: string[], proceed: () => Promise<void>) => {
      pendingRef.current = proceed;
      setSoftIssues(issues);
    },
    [],
  );

  const confirm = useCallback(async () => {
    if (confirmingRef.current) return;
    const proceed = pendingRef.current;
    if (!proceed) {
      resetConfirm();
      return;
    }
    confirmingRef.current = true;
    setIsConfirmSubmitting(true);
    try {
      await proceed();
      resetConfirm();
    } catch (error) {
      console.error("[ProviderForm] soft-confirm submit failed:", error);
      // 保留确认框和待提交的值，让用户可以重试或取消
    } finally {
      confirmingRef.current = false;
      setIsConfirmSubmitting(false);
    }
  }, [resetConfirm]);

  const cancelConfirm = useCallback(() => {
    if (confirmingRef.current) return;
    resetConfirm();
  }, [resetConfirm]);

  return {
    blockingErrors,
    block,
    clearBlocking,
    softIssues,
    requestConfirm,
    isConfirmSubmitting,
    confirm,
    cancelConfirm,
  };
}
