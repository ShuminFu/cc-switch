import { useCallback, useState } from "react";

/**
 * Ask for confirmation before closing something that has unsaved edits.
 *
 * `requestClose` closes immediately when clean; otherwise it opens the
 * confirmation, and `confirmDiscard` / `cancelDiscard` resolve it. Pair with
 * `DiscardChangesDialog`.
 */
export function useCloseGuard(
  isDirty: boolean,
  onClose: () => void,
  onDiscard?: () => void,
) {
  const [confirmOpen, setConfirmOpen] = useState(false);

  const requestClose = useCallback(() => {
    if (isDirty) {
      setConfirmOpen(true);
    } else {
      onClose();
    }
  }, [isDirty, onClose]);

  const confirmDiscard = useCallback(() => {
    setConfirmOpen(false);
    onDiscard?.();
    onClose();
  }, [onClose, onDiscard]);

  const cancelDiscard = useCallback(() => setConfirmOpen(false), []);

  return { requestClose, confirmOpen, confirmDiscard, cancelDiscard };
}
