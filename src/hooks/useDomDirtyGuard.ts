import { useCallback, useMemo, useRef } from "react";

/**
 * Serialise what the user can edit inside `root`: input / textarea / select
 * values, checkbox and switch states, and CodeMirror documents.
 */
export function snapshotEditableState(root: HTMLElement): string {
  const parts: string[] = [];
  root
    .querySelectorAll<
      HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement
    >("input, textarea, select")
    .forEach((el) => {
      if (el instanceof HTMLInputElement) {
        if (el.type === "checkbox" || el.type === "radio") {
          parts.push(`c:${el.checked}`);
          return;
        }
        if (el.type === "hidden" || el.type === "file") return;
      }
      parts.push(`v:${el.value}`);
    });
  root
    .querySelectorAll<HTMLElement>(
      '[role="switch"], [role="checkbox"], [role="radio"], [role="combobox"]',
    )
    .forEach((el) => {
      parts.push(
        `a:${el.getAttribute("aria-checked") ?? el.getAttribute("data-state") ?? el.textContent ?? ""}`,
      );
    });
  root.querySelectorAll<HTMLElement>(".cm-content").forEach((el) => {
    parts.push(`e:${el.textContent ?? ""}`);
  });
  return parts.join("\u0000");
}

/**
 * Unsaved-changes detection for a form whose state is spread over many
 * components (FE-PROV-24). The baseline is taken at the user's first
 * interaction inside the container (capture phase, before the change lands),
 * so values the form normalises on mount do not count as edits. `isDirty()`
 * compares the current editable state with that baseline.
 */
export function useDomDirtyGuard() {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const baselineRef = useRef<string | null>(null);

  const captureBaseline = useCallback(() => {
    if (baselineRef.current !== null || !containerRef.current) return;
    baselineRef.current = snapshotEditableState(containerRef.current);
  }, []);

  const isDirty = useCallback(() => {
    if (baselineRef.current === null || !containerRef.current) return false;
    return snapshotEditableState(containerRef.current) !== baselineRef.current;
  }, []);

  const reset = useCallback(() => {
    baselineRef.current = null;
  }, []);

  const containerProps = useMemo(
    () => ({
      ref: containerRef,
      onKeyDownCapture: captureBaseline,
      onPointerDownCapture: captureBaseline,
      onPasteCapture: captureBaseline,
      onDropCapture: captureBaseline,
    }),
    [captureBaseline],
  );

  return { containerProps, isDirty, reset };
}
