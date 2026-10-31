import { afterEach, describe, expect, it, vi } from "vitest";
import { z } from "zod";
import { createStoredValue } from "@/lib/storage";
import { languagePreference } from "@/hooks/settingsPayload";

// FE-ARCH-22: persisted client state is validated, never trusted by a cast.
describe("createStoredValue", () => {
  afterEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    vi.restoreAllMocks();
  });

  const lastApp = createStoredValue(
    "test-last-app",
    z.enum(["claude", "codex"]),
    "claude",
  );

  it("returns the default for a missing or invalid stored value", () => {
    expect(lastApp.get()).toBe("claude");
    window.localStorage.setItem("test-last-app", "not-an-app");
    expect(lastApp.get()).toBe("claude");
  });

  it("round-trips a valid value as a raw string", () => {
    lastApp.set("codex");
    expect(window.localStorage.getItem("test-last-app")).toBe("codex");
    expect(lastApp.get()).toBe("codex");
  });

  it("stores structured values as JSON and validates them", () => {
    const filters = createStoredValue(
      "test-filters",
      z.object({ q: z.string() }),
      { q: "" },
    );
    filters.set({ q: "abc" });
    expect(filters.get()).toEqual({ q: "abc" });
    window.localStorage.setItem("test-filters", "{broken");
    expect(filters.get()).toEqual({ q: "" });
  });

  it("migrates a valid legacy key once", () => {
    const value = createStoredValue("test-new", z.string(), "", {
      legacyKeys: ["test-old"],
    });
    window.localStorage.setItem("test-old", "v1");
    expect(value.get()).toBe("v1");
    expect(window.localStorage.getItem("test-new")).toBe("v1");
    expect(window.localStorage.getItem("test-old")).toBeNull();
  });

  it("survives storage that throws", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    expect(lastApp.get()).toBe("claude");
    expect(lastApp.set("codex")).toBe(false);
  });

  it("supports sessionStorage", () => {
    const dismissed = createStoredValue(
      "test-dismissed",
      z.literal("true"),
      undefined as "true" | undefined,
      {
        area: "session",
      },
    );
    dismissed.set("true");
    expect(window.sessionStorage.getItem("test-dismissed")).toBe("true");
  });

  it("normalises language aliases and rejects unknown languages", () => {
    window.localStorage.setItem("language", "zh_Hant");
    expect(languagePreference.get()).toBe("zh-TW");
    window.localStorage.setItem("language", "klingon");
    expect(languagePreference.get()).toBeUndefined();
  });
});
