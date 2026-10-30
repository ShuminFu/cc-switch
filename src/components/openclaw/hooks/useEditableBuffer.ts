import { useCallback, useEffect, useRef, useState } from "react";

const sameJson = (a: unknown, b: unknown) =>
  JSON.stringify(a) === JSON.stringify(b);

/**
 * Local edit buffer for a server-backed config section (FE-UI-34).
 *
 * - Hydrates from `data` once (the first time it is not `undefined`).
 * - While the buffer is clean, later server data (refetch on focus, another
 *   window) is adopted silently.
 * - While the buffer is dirty, newer server data is NOT applied; instead
 *   `externalChange` turns true so the UI can offer "Reload".
 *
 * `toForm` maps server data to the form shape; `isEqual` compares two form
 * values (defaults to JSON equality) and drives `dirty`.
 */
export function useEditableBuffer<TData, TForm>(
  data: TData | undefined,
  toForm: (data: TData) => TForm,
  initialForm: TForm,
  isEqual: (a: TForm, b: TForm) => boolean = sameJson,
) {
  const [form, setForm] = useState<TForm>(initialForm);
  const [baseline, setBaseline] = useState<TForm | null>(null);
  const [pending, setPending] = useState<TData | undefined>(undefined);
  const hydratedFrom = useRef<TData | undefined>(undefined);
  const hydrated = useRef(false);

  const toFormRef = useRef(toForm);
  toFormRef.current = toForm;
  const isEqualRef = useRef(isEqual);
  isEqualRef.current = isEqual;

  const dirty = baseline !== null && !isEqual(form, baseline);
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;

  const adopt = useCallback((next: TData) => {
    const nextForm = toFormRef.current(next);
    hydratedFrom.current = next;
    hydrated.current = true;
    setForm(nextForm);
    setBaseline(nextForm);
    setPending(undefined);
  }, []);

  useEffect(() => {
    if (data === undefined) return;
    if (!hydrated.current) {
      adopt(data);
      return;
    }
    if (data === hydratedFrom.current) return;
    if (sameJson(data, hydratedFrom.current)) {
      hydratedFrom.current = data;
      return;
    }
    if (dirtyRef.current) {
      setPending(data);
    } else {
      adopt(data);
    }
  }, [data, adopt]);

  /** Discard local edits and load the latest server data. */
  const reload = useCallback(() => {
    const latest = pending ?? hydratedFrom.current;
    if (latest !== undefined) adopt(latest);
  }, [pending, adopt]);

  /** The current form was persisted: it becomes the clean baseline. */
  const formRef = useRef(form);
  formRef.current = form;
  const markSaved = useCallback(() => {
    setBaseline(formRef.current);
    setPending(undefined);
  }, []);

  return {
    form,
    setForm,
    dirty,
    externalChange: pending !== undefined,
    reload,
    markSaved,
    loaded: baseline !== null,
  };
}
