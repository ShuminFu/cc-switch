import { useCallback, useEffect, useMemo, useRef, useState } from "react";

/**
 * Local, editable copy ("draft") of a resource that lives on disk / in a
 * query cache, with protection against both directions of data loss:
 *
 * - Seeds the draft from `data`, and silently re-seeds whenever `data` changes
 *   while the draft has no unsaved edits.
 * - While the draft *is* dirty, a change in `data` never overwrites it.
 *   Instead `externalChange` becomes true so the UI can offer
 *   "Changed on disk — Reload / Keep mine".
 * - `isDirty` is derived by comparing the draft with the baseline it was
 *   seeded from, so echoes of programmatic updates (e.g. CodeMirror reporting
 *   the value it was just given) or edits that are typed and then undone do
 *   not count as unsaved changes.
 */
export interface UseEditableResourceOptions<TData, TDraft> {
  /** Source data. `undefined` means "not loaded (yet)". */
  data: TData | undefined;
  /** Derive the editable form state from source data. */
  toDraft: (data: TData) => TDraft;
  /** Draft used before `data` is available. */
  initialDraft: TDraft;
  /** Equality used for dirty / external-change detection. */
  isEqual?: (a: TDraft, b: TDraft) => boolean;
}

export interface EditableResource<TData, TDraft> {
  draft: TDraft;
  setDraft: (next: TDraft | ((prev: TDraft) => TDraft)) => void;
  /** True once the draft has been seeded from real data. */
  isSeeded: boolean;
  /** The draft differs from what it was seeded from / last saved as. */
  isDirty: boolean;
  /** `data` changed underneath unsaved edits. */
  externalChange: boolean;
  /** The source data the current baseline corresponds to. */
  baselineData: TData | undefined;
  /** Replace the draft with the latest source data (discards edits). */
  reload: () => void;
  /** Keep the draft; accept the latest source data as the new baseline. */
  keepMine: () => void;
  /**
   * Record a successful save. Pass the draft snapshot that was saved so edits
   * typed while the save was in flight remain dirty.
   */
  markSaved: (saved?: TDraft) => void;
}

interface State<TData, TDraft> {
  seeded: boolean;
  draft: TDraft;
  baseline: TDraft;
  baselineData: TData | undefined;
  pending: { data: TData; draft: TDraft } | null;
}

function seedState<TData, TDraft>(
  source: TData,
  toDraft: (data: TData) => TDraft,
): State<TData, TDraft> {
  const draft = toDraft(source);
  return {
    seeded: true,
    draft,
    baseline: draft,
    baselineData: source,
    pending: null,
  };
}

export const defaultDraftEquals = <T>(a: T, b: T): boolean =>
  Object.is(a, b) || JSON.stringify(a) === JSON.stringify(b);

export function useEditableResource<TData, TDraft>({
  data,
  toDraft,
  initialDraft,
  isEqual = defaultDraftEquals,
}: UseEditableResourceOptions<TData, TDraft>): EditableResource<TData, TDraft> {
  // Callers usually pass inline functions; keep the latest without making
  // them effect dependencies.
  const toDraftRef = useRef(toDraft);
  toDraftRef.current = toDraft;
  const isEqualRef = useRef(isEqual);
  isEqualRef.current = isEqual;
  const latestDataRef = useRef(data);
  latestDataRef.current = data;

  const [state, setState] = useState<State<TData, TDraft>>(() =>
    data === undefined
      ? {
          seeded: false,
          draft: initialDraft,
          baseline: initialDraft,
          baselineData: undefined,
          pending: null,
        }
      : seedState(data, toDraft),
  );

  useEffect(() => {
    if (data === undefined) return;
    setState((prev) => {
      if (prev.seeded && prev.baselineData === data) return prev;
      if (prev.pending?.data === data) return prev;

      const equals = isEqualRef.current;
      const nextDraft = toDraftRef.current(data);
      const dirty = !equals(prev.draft, prev.baseline);

      if (!dirty) return seedState(data, toDraftRef.current);
      if (equals(nextDraft, prev.baseline)) {
        // Same content as what the user started from: nothing to report.
        return { ...prev, seeded: true, baselineData: data, pending: null };
      }
      return { ...prev, pending: { data, draft: nextDraft } };
    });
  }, [data]);

  const setDraft = useCallback((next: TDraft | ((prev: TDraft) => TDraft)) => {
    setState((prev) => {
      const value =
        typeof next === "function"
          ? (next as (p: TDraft) => TDraft)(prev.draft)
          : next;
      return Object.is(value, prev.draft) ? prev : { ...prev, draft: value };
    });
  }, []);

  const reload = useCallback(() => {
    setState((prev) => {
      const source = prev.pending?.data ?? latestDataRef.current;
      if (source === undefined) {
        return { ...prev, draft: prev.baseline, pending: null };
      }
      return seedState(source, toDraftRef.current);
    });
  }, []);

  const keepMine = useCallback(() => {
    setState((prev) =>
      prev.pending
        ? {
            ...prev,
            baseline: prev.pending.draft,
            baselineData: prev.pending.data,
            pending: null,
          }
        : prev,
    );
  }, []);

  const markSaved = useCallback((saved?: TDraft) => {
    // Our write is now what's on disk, so any external change seen before is
    // superseded.
    setState((prev) => ({
      ...prev,
      seeded: true,
      baseline: saved === undefined ? prev.draft : saved,
      pending: null,
    }));
  }, []);

  const isDirty = useMemo(
    () => !isEqualRef.current(state.draft, state.baseline),
    [state.draft, state.baseline],
  );

  return {
    draft: state.draft,
    setDraft,
    isSeeded: state.seeded,
    isDirty,
    externalChange: state.pending !== null,
    baselineData: state.baselineData,
    reload,
    keepMine,
    markSaved,
  };
}
