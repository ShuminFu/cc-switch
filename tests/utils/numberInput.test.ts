import { describe, expect, it } from "vitest";
import {
  parseNonNegativeIntInput,
  parseStrictInteger,
  parseStrictNonNegativeInt,
} from "@/utils/numberInput";

describe("parseStrictInteger", () => {
  it("accepts integers including negatives and exponent form", () => {
    expect(parseStrictInteger("-12")).toBe(-12);
    expect(parseStrictInteger("+7")).toBe(7);
    expect(parseStrictInteger("2e3")).toBe(2000);
    expect(parseStrictInteger("")).toBeUndefined();
  });

  it("rejects non-decimal literals that Number() would accept", () => {
    expect(parseStrictInteger("0x10")).toBeNull();
    expect(parseStrictInteger("0b11")).toBeNull();
    expect(parseStrictInteger("Infinity")).toBeNull();
    expect(parseStrictInteger("1.5")).toBeNull();
  });

  it("makes parseStrictNonNegativeInt reject hex too", () => {
    expect(parseStrictNonNegativeInt("0x10")).toBeNull();
  });
});

describe("parseStrictNonNegativeInt", () => {
  it("returns undefined for empty input", () => {
    expect(parseStrictNonNegativeInt("")).toBeUndefined();
    expect(parseStrictNonNegativeInt("  ")).toBeUndefined();
  });

  it("accepts non-negative integers", () => {
    expect(parseStrictNonNegativeInt("0")).toBe(0);
    expect(parseStrictNonNegativeInt(" 300 ")).toBe(300);
    expect(parseStrictNonNegativeInt("1e3")).toBe(1000);
  });

  it("rejects negatives, decimals, garbage and unsafe values", () => {
    expect(parseStrictNonNegativeInt("-1")).toBeNull();
    expect(parseStrictNonNegativeInt("1.5")).toBeNull();
    expect(parseStrictNonNegativeInt("12abc")).toBeNull();
    expect(parseStrictNonNegativeInt("1e400")).toBeNull();
  });
});

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
