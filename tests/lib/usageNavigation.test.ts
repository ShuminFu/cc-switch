import { afterEach, describe, expect, it, vi } from "vitest";
import {
  requestLogSessionFilter,
  resetSessionFilterForTests,
  subscribeSessionFilter,
  takePendingSessionFilter,
} from "@/lib/usageNavigation";

describe("usage navigation session filter", () => {
  afterEach(() => resetSessionFilterForTests());

  it("hands the pending session id to the next consumer exactly once", () => {
    requestLogSessionFilter("  sess-1 ");
    expect(takePendingSessionFilter()).toBe("sess-1");
    expect(takePendingSessionFilter()).toBeNull();
  });

  it("notifies mounted subscribers and ignores blank ids", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeSessionFilter(listener);
    requestLogSessionFilter("sess-2");
    requestLogSessionFilter("   ");
    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener).toHaveBeenCalledWith("sess-2");
    unsubscribe();
    requestLogSessionFilter("sess-3");
    expect(listener).toHaveBeenCalledTimes(1);
  });
});
