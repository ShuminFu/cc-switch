import type { z } from "zod";

/**
 * Typed, validated access to browser storage (FE-ARCH-22).
 *
 * Every client-side persisted value goes through `createStoredValue`, which
 * owns its key, validates what it reads (stale or hand-edited values fall
 * back to the default instead of being trusted with a cast) and swallows
 * storage exceptions (private mode, quota, disabled storage).
 *
 * Keys in use (document new ones here):
 * - `language`                    UI language (i18n, settings)
 * - `cc-switch-last-app`          last selected app (navigation)
 * - `cc-switch-last-view`         last open page (navigation)
 * - `cc-switch-theme`             theme (theme provider)
 * - `cc-switch:update-dismissed-version` dismissed update (UpdateContext)
 * - `env_banner_dismissed`        env conflict banner (sessionStorage)
 * - `cc-switch-*-common-config-snippet` legacy snippets, migrated to the DB
 */

export type StorageArea = "local" | "session";

function getStorage(area: StorageArea): Storage | undefined {
  try {
    if (typeof window === "undefined") return undefined;
    return area === "local" ? window.localStorage : window.sessionStorage;
  } catch {
    return undefined;
  }
}

export interface StoredValue<T> {
  readonly key: string;
  /** The stored value, or the default when missing/invalid/unreadable. */
  get(): T;
  /** Persist `value`; returns false when storage is unavailable. */
  set(value: T): boolean;
  remove(): void;
}

export interface StoredValueOptions<T> {
  area?: StorageArea;
  /** Older keys whose (valid) value is moved to `key` on first read. */
  legacyKeys?: string[];
  /** Encode/decode; defaults to raw strings for string schemas, else JSON. */
  serialize?: (value: T) => string;
  deserialize?: (raw: string) => unknown;
}

export function createStoredValue<T>(
  key: string,
  schema: z.ZodType<T>,
  defaultValue: T,
  options: StoredValueOptions<T> = {},
): StoredValue<T> {
  const area = options.area ?? "local";
  const serialize =
    options.serialize ??
    ((value: T) => (typeof value === "string" ? value : JSON.stringify(value)));
  const deserialize =
    options.deserialize ??
    ((raw: string) => {
      const asString = schema.safeParse(raw);
      if (asString.success) return raw;
      try {
        return JSON.parse(raw);
      } catch {
        return undefined;
      }
    });

  const parse = (raw: string | null): T | undefined => {
    if (raw === null) return undefined;
    const result = schema.safeParse(deserialize(raw));
    return result.success ? result.data : undefined;
  };

  const read = (storageKey: string): string | null => {
    try {
      return getStorage(area)?.getItem(storageKey) ?? null;
    } catch {
      return null;
    }
  };

  const write = (storageKey: string, raw: string): boolean => {
    const storage = getStorage(area);
    if (!storage) return false;
    try {
      storage.setItem(storageKey, raw);
      return true;
    } catch {
      return false;
    }
  };

  const removeKey = (storageKey: string) => {
    try {
      getStorage(area)?.removeItem(storageKey);
    } catch {
      // storage unavailable: nothing to remove
    }
  };

  return {
    key,
    get() {
      const current = parse(read(key));
      if (current !== undefined) return current;
      for (const legacyKey of options.legacyKeys ?? []) {
        const legacy = parse(read(legacyKey));
        if (legacy !== undefined) {
          write(key, serialize(legacy));
          removeKey(legacyKey);
          return legacy;
        }
      }
      return defaultValue;
    },
    set(value) {
      return write(key, serialize(value));
    },
    remove() {
      removeKey(key);
      for (const legacyKey of options.legacyKeys ?? []) removeKey(legacyKey);
    },
  };
}

/**
 * Read a legacy raw string value (used by one-time migrations of data that
 * now lives in the database); pair with `removeStoredKey` once migrated.
 */
export function readLegacyString(
  key: string,
  area: StorageArea = "local",
): string | undefined {
  const storage = getStorage(area);
  if (!storage) return undefined;
  try {
    const value = storage.getItem(key);
    return value === null ? undefined : value;
  } catch {
    return undefined;
  }
}

export function removeStoredKey(key: string, area: StorageArea = "local") {
  try {
    getStorage(area)?.removeItem(key);
  } catch {
    // ignore
  }
}
