import { useCallback, useRef } from "react";

/**
 * Tracks whether a form seeded from query data has unsaved user edits.
 *
 * The OpenClaw panels seed local form state from react-query data. Without a
 * guard, any refetch that yields new data (window-focus refetch after the file
 * changed on disk, a health-check invalidation, …) would overwrite edits the
 * user has not saved yet. Panels call `markDirty()` from every user edit,
 * re-seed only when `isDirty()` is false, and call `beginSave()` /
 * `markSaved(token)` around a save so that edits made while the save was in
 * flight keep the form dirty.
 *
 * Refs are used (not state) so seeding effects can read the current value
 * without re-running, and stale closures (e.g. CodeMirror listeners) stay
 * correct.
 */
export function useDirtyTracker() {
  const dirtyRef = useRef(false);
  const versionRef = useRef(0);

  const isDirty = useCallback(() => dirtyRef.current, []);

  const markDirty = useCallback(() => {
    dirtyRef.current = true;
    versionRef.current += 1;
  }, []);

  /** Returns a token identifying the edit state being saved. */
  const beginSave = useCallback(() => versionRef.current, []);

  /** Clears the dirty flag unless the user edited again after `beginSave`. */
  const markSaved = useCallback((token: number) => {
    if (versionRef.current === token) {
      dirtyRef.current = false;
    }
  }, []);

  return { isDirty, markDirty, beginSave, markSaved };
}
