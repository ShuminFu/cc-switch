import { useEffect, useState } from "react";

/**
 * 按需加载的预设模块。预设表体积较大，只在打开对应应用的编辑器
 * （或提交时需要查预设）时才 import()；加载一次后缓存在模块内，
 * 之后再打开表单可同步拿到。
 */
export interface PresetLoader<T> {
  load: () => Promise<T>;
  /** 已加载时返回缓存，未加载返回 undefined */
  peek: () => T | undefined;
}

export function createPresetLoader<T>(
  importer: () => Promise<T>,
): PresetLoader<T> {
  let cached: T | undefined;
  let pending: Promise<T> | undefined;
  return {
    load: () => {
      if (cached !== undefined) return Promise.resolve(cached);
      pending ??= importer().then(
        (value) => {
          cached = value;
          return value;
        },
        (error: unknown) => {
          pending = undefined;
          throw error;
        },
      );
      return pending;
    },
    peek: () => cached,
  };
}

/** 读取按需加载的预设；加载完成前返回 undefined */
export function useLazyPresets<T>(loader: PresetLoader<T>): T | undefined {
  const [value, setValue] = useState<T | undefined>(() => loader.peek());

  useEffect(() => {
    if (value !== undefined) return;
    let cancelled = false;
    loader
      .load()
      .then((loaded) => {
        if (!cancelled) setValue(loaded);
      })
      .catch((error: unknown) => {
        console.error("Failed to load provider presets:", error);
      });
    return () => {
      cancelled = true;
    };
  }, [loader, value]);

  return value;
}
