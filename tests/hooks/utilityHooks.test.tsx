/**
 * Unit tests for the small utility hooks that had none (TEST-18). Timing is
 * driven by fake timers, never real sleeps (TEST-10).
 */
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useAutoCompact } from "@/hooks/useAutoCompact";
import { useDarkMode } from "@/hooks/useDarkMode";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { useLastValidValue } from "@/hooks/useLastValidValue";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { emitTauriEvent } from "../msw/tauriMocks";

afterEach(() => {
  vi.useRealTimers();
  document.documentElement.classList.remove("dark");
});

describe("useDebouncedValue", () => {
  it("only publishes the last value after the delay", () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    const { result, rerender } = renderHook(
      ({ value }) => useDebouncedValue(value, 300),
      { initialProps: { value: "a" } },
    );
    expect(result.current).toBe("a");

    rerender({ value: "ab" });
    act(() => vi.advanceTimersByTime(299));
    expect(result.current).toBe("a");
    rerender({ value: "abc" }); // restarts the window
    act(() => vi.advanceTimersByTime(299));
    expect(result.current).toBe("a");
    act(() => vi.advanceTimersByTime(1));
    expect(result.current).toBe("abc");
  });
});

describe("useLastValidValue", () => {
  it("keeps the last non-null value while the current one is null", () => {
    const { result, rerender } = renderHook(
      ({ value }: { value: string | null | undefined }) =>
        useLastValidValue(value),
      { initialProps: { value: null as string | null | undefined } },
    );
    expect(result.current).toBeNull();
    rerender({ value: "first" });
    expect(result.current).toBe("first");
    rerender({ value: null });
    expect(result.current).toBe("first");
    rerender({ value: undefined });
    expect(result.current).toBe("first");
    rerender({ value: "second" });
    expect(result.current).toBe("second");
  });
});

describe("useDarkMode", () => {
  it("follows the `dark` class on <html>", async () => {
    const { result } = renderHook(() => useDarkMode());
    expect(result.current).toBe(false);
    act(() => document.documentElement.classList.add("dark"));
    await waitFor(() => expect(result.current).toBe(true));
    act(() => document.documentElement.classList.remove("dark"));
    await waitFor(() => expect(result.current).toBe(false));
  });

  it("reads the initial state synchronously", () => {
    document.documentElement.classList.add("dark");
    const { result } = renderHook(() => useDarkMode());
    expect(result.current).toBe(true);
  });
});

describe("useTauriEvent", () => {
  it("delivers payloads to the latest handler and unsubscribes on unmount", async () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender, unmount } = renderHook(
      ({ handler }) => useTauriEvent<{ n: number }>("test-event", handler),
      { initialProps: { handler: first } },
    );
    // Registration is async; wait until the listener is live.
    await waitFor(() => {
      emitTauriEvent("test-event", { n: 0 });
      expect(first).toHaveBeenCalled();
    });
    first.mockClear();

    rerender({ handler: second });
    emitTauriEvent("test-event", { n: 1 });
    expect(second).toHaveBeenCalledWith({ n: 1 });
    expect(first).not.toHaveBeenCalled();

    unmount();
    emitTauriEvent("test-event", { n: 2 });
    expect(second).toHaveBeenCalledTimes(1);
  });

  it("ignores events with another name", async () => {
    const handler = vi.fn();
    renderHook(() => useTauriEvent("mine", handler));
    await act(async () => {});
    emitTauriEvent("someone-else", {});
    expect(handler).not.toHaveBeenCalled();
  });
});

describe("useAutoCompact", () => {
  type Callback = () => void;
  let observers: Callback[] = [];

  function setup(width: { scroll: number; client: number }) {
    observers = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(cb: Callback) {
          observers.push(cb);
        }
        observe() {}
        disconnect() {}
        unobserve() {}
      },
    );
    const el = document.createElement("div");
    Object.defineProperty(el, "scrollWidth", { get: () => width.scroll });
    Object.defineProperty(el, "clientWidth", { get: () => width.client });
    return { ref: { current: el }, width };
  }

  const fire = () => act(() => observers.at(-1)?.());

  afterEach(() => vi.unstubAllGlobals());

  it("compacts on overflow and expands once the normal width fits again", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    const { ref, width } = setup({ scroll: 500, client: 400 });
    const { result } = renderHook(() => useAutoCompact(ref));
    expect(result.current).toBe(false);

    fire();
    expect(result.current).toBe(true);

    // Still too narrow for the cached normal width (500).
    width.client = 450;
    fire();
    expect(result.current).toBe(true);

    width.client = 520;
    fire();
    expect(result.current).toBe(false);

    // During the 250 ms expand lock, overflow is ignored...
    width.scroll = 600;
    width.client = 400;
    fire();
    expect(result.current).toBe(false);
    // ...and honoured afterwards.
    vi.advanceTimersByTime(251);
    fire();
    expect(result.current).toBe(true);
  });

  it("does nothing while the content fits", () => {
    const { ref } = setup({ scroll: 300, client: 300 });
    const { result } = renderHook(() => useAutoCompact(ref));
    fire();
    expect(result.current).toBe(false);
  });
});
