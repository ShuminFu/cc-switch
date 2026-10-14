import { describe, expect, it } from "vitest";
import {
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
