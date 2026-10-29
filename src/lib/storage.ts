/**
 * Safe wrappers around `window.localStorage`.
 *
 * Storage access can throw (disabled storage, quota exceeded, sandboxed
 * WebView, SSR). Persisted UI preferences are best-effort, so failures are
 * swallowed and callers fall back to their defaults.
 */

const getStorage = (): Storage | null => {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
};

/** Read a value; returns `null` when missing or when storage is unavailable. */
export function safeStorageGet(key: string): string | null {
  try {
    return getStorage()?.getItem(key) ?? null;
  } catch (error) {
    console.warn(`[storage] Failed to read "${key}"`, error);
    return null;
  }
}

/** Write a value; returns `false` when storage is unavailable. */
export function safeStorageSet(key: string, value: string): boolean {
  try {
    const storage = getStorage();
    if (!storage) return false;
    storage.setItem(key, value);
    return true;
  } catch (error) {
    console.warn(`[storage] Failed to write "${key}"`, error);
    return false;
  }
}

/** Remove a value; returns `false` when storage is unavailable. */
export function safeStorageRemove(key: string): boolean {
  try {
    const storage = getStorage();
    if (!storage) return false;
    storage.removeItem(key);
    return true;
  } catch (error) {
    console.warn(`[storage] Failed to remove "${key}"`, error);
    return false;
  }
}
