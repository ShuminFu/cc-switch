import { afterEach, describe, expect, it } from "vitest";
import {
  SESSION_GROUP_EXPANSION_STORAGE_KEY,
  filterSetToAllowedValues,
  readInitialSessionGroupExpansionState,
  serializeSessionGroupExpansionState,
} from "@/components/sessions/sessionListState";

describe("sessionListState (SESS-25)", () => {
  afterEach(() => {
    window.localStorage.removeItem(SESSION_GROUP_EXPANSION_STORAGE_KEY);
  });

  it("round-trips the persisted group expansion and ignores junk", () => {
    window.localStorage.setItem(
      SESSION_GROUP_EXPANSION_STORAGE_KEY,
      serializeSessionGroupExpansionState(new Set(["codex"]), new Set(["d1"])),
    );
    const state = readInitialSessionGroupExpansionState();
    expect([...state.expandedProviderIds]).toEqual(["codex"]);
    expect([...state.expandedDirectoryKeys]).toEqual(["d1"]);

    window.localStorage.setItem(SESSION_GROUP_EXPANSION_STORAGE_KEY, "{bad");
    expect(
      readInitialSessionGroupExpansionState().expandedProviderIds.size,
    ).toBe(0);
  });

  it("keeps the same set when nothing is filtered out", () => {
    const current = new Set(["a", "b"]);
    expect(filterSetToAllowedValues(current, new Set(["a", "b", "c"]))).toBe(
      current,
    );
    expect([...filterSetToAllowedValues(current, new Set(["b"]))]).toEqual([
      "b",
    ]);
  });
});
