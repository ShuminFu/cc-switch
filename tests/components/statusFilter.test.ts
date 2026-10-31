import { describe, expect, it } from "vitest";
import {
  STATUS_CODE_OPTIONS,
  statusFilterToFilters,
} from "@/components/usage/statusFilter";
import { usageKeys } from "@/lib/query/keys";

describe("request-log status filter (FE-UI-59)", () => {
  it("maps status classes to inclusive ranges", () => {
    expect(statusFilterToFilters("all")).toEqual({});
    expect(statusFilterToFilters("errors")).toEqual({ statusMin: 400 });
    expect(statusFilterToFilters("2xx")).toEqual({
      statusMin: 200,
      statusMax: 299,
    });
    expect(statusFilterToFilters("4xx")).toEqual({
      statusMin: 400,
      statusMax: 499,
    });
    expect(statusFilterToFilters("5xx")).toEqual({
      statusMin: 500,
      statusMax: 599,
    });
  });

  it("maps single codes to an exact filter and ignores junk", () => {
    expect(statusFilterToFilters("529")).toEqual({ statusCode: 529 });
    expect(statusFilterToFilters("abc")).toEqual({});
    expect(statusFilterToFilters("42")).toEqual({});
  });

  it("offers the codes proxy failures commonly return", () => {
    for (const code of [403, 404, 502, 503, 529]) {
      expect(STATUS_CODE_OPTIONS).toContain(code);
    }
  });

  it("gives each status range its own cache entry", () => {
    const base = { preset: "today" as const };
    const errors = usageKeys.logs({ ...base, statusMin: 400 }, 0, 20);
    const clientErrors = usageKeys.logs(
      { ...base, statusMin: 400, statusMax: 499 },
      0,
      20,
    );
    expect(errors).not.toEqual(clientErrors);
  });
});
