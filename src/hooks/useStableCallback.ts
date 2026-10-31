import { useCallback, useLayoutEffect, useRef } from "react";

/**
 * A function with a stable identity that always calls the latest `fn`
 * (the `useEvent` pattern). Use it for handlers passed to memoised children
 * so a parent re-render does not invalidate their props (PERF-20,
 * FE-ARCH-09). Returns `undefined` when `fn` is `undefined`, so optional
 * handler props keep their "absent" meaning.
 */
export function useStableCallback<A extends unknown[], R>(
  fn: (...args: A) => R,
): (...args: A) => R;
export function useStableCallback<A extends unknown[], R>(
  fn: ((...args: A) => R) | undefined,
): ((...args: A) => R) | undefined;
export function useStableCallback<A extends unknown[], R>(
  fn: ((...args: A) => R) | undefined,
): ((...args: A) => R) | undefined {
  const ref = useRef(fn);
  useLayoutEffect(() => {
    ref.current = fn;
  });
  const stable = useCallback(
    (...args: A) => (ref.current as (...args: A) => R)(...args),
    [],
  );
  return fn ? stable : undefined;
}
