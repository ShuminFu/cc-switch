import { describe, expect, it } from "vitest";
import { parseNonNegativeIntInput } from "@/utils/numberInput";

describe("parseNonNegativeIntInput", () => {
  it("returns undefined for empty input", () => {
    expect(parseNonNegativeIntInput("")).toBeUndefined();
    expect(parseNonNegativeIntInput("   ")).toBeUndefined();
  });

  it("parses plain integers", () => {
    expect(parseNonNegativeIntInput("200000")).toBe(200000);
    expect(parseNonNegativeIntInput(" 32000 ")).toBe(32000);
    expect(parseNonNegativeIntInput("0")).toBe(0);
  });

  it("expands exponent notation instead of truncating it", () => {
    // parseInt("1e5") === 1
    expect(parseNonNegativeIntInput("1e5")).toBe(100000);
  });

  it("floors decimals", () => {
    expect(parseNonNegativeIntInput("1.9")).toBe(1);
  });

  it("strips non-digit garbage", () => {
    expect(parseNonNegativeIntInput("12abc")).toBe(12);
    expect(parseNonNegativeIntInput("-5")).toBe(5);
    expect(parseNonNegativeIntInput("abc")).toBeUndefined();
  });

  it("rejects values beyond the safe integer range", () => {
    expect(parseNonNegativeIntInput("1e400")).toBeUndefined();
    expect(parseNonNegativeIntInput("99999999999999999999")).toBeUndefined();
  });
});
