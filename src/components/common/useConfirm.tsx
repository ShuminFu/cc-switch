import React, { useCallback, useMemo, useState } from "react";
import { ConfirmDialog } from "@/components/ConfirmDialog";

export interface ConfirmOptions {
  title: string;
  message: string;
  confirmText?: string;
  variant?: "destructive" | "info";
  zIndex?: "base" | "nested" | "alert" | "top";
  /**
   * Runs when the user confirms. The dialog closes once it resolves; if it
   * throws, the dialog stays open (the caller reports the error) so the user
   * can retry or cancel.
   */
  onConfirm: () => unknown | Promise<unknown>;
}

export interface ConfirmController {
  /** Open the confirmation dialog. */
  confirm: (options: ConfirmOptions) => void;
  /** Close it without running `onConfirm`. */
  close: () => void;
  /** The rendered dialog (or `null`); place it once in the tree. */
  dialog: React.ReactNode;
}

/**
 * One confirm-dialog state machine for list panels (FE-UI-18), replacing the
 * per-panel `confirmDialog` state objects in MCP, Skills and Prompts.
 */
export function useConfirm(): ConfirmController {
  const [options, setOptions] = useState<ConfirmOptions | null>(null);

  const close = useCallback(() => setOptions(null), []);

  const handleConfirm = useCallback(async () => {
    if (!options) return;
    try {
      await options.onConfirm();
      setOptions((current) => (current === options ? null : current));
    } catch {
      // Keep the dialog open; the caller already surfaced the error.
    }
  }, [options]);

  const dialog = useMemo(
    () =>
      options ? (
        <ConfirmDialog
          isOpen
          title={options.title}
          message={options.message}
          confirmText={options.confirmText}
          variant={options.variant}
          zIndex={options.zIndex}
          onConfirm={() => void handleConfirm()}
          onCancel={close}
        />
      ) : null,
    [options, handleConfirm, close],
  );

  return { confirm: setOptions, close, dialog };
}
