import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { decodeBase64Utf8 } from "@/lib/utils/base64";

const encodeStandard = (text: string) => {
  const bytes = new TextEncoder().encode(text);
  return btoa(String.fromCharCode(...bytes));
};

describe("decodeBase64Utf8", () => {
  beforeEach(() => {
    vi.spyOn(console, "error").mockImplementation(() => {});
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("decodes plain ASCII", () => {
    expect(decodeBase64Utf8(encodeStandard("hello world"))).toBe("hello world");
  });

  it("decodes multi-byte UTF-8 (CJK and emoji)", () => {
    const text = "你好，世界 🚀";
    expect(decodeBase64Utf8(encodeStandard(text))).toBe(text);
  });

  it("restores '+' that URL parsing turned into spaces", () => {
    // ">>>?" encodes to "Pj4+Pw==" which contains a '+'
    const encoded = encodeStandard(">>>?");
    expect(encoded).toContain("+");
    expect(decodeBase64Utf8(encoded.replace(/\+/g, " "))).toBe(">>>?");
  });

  it("accepts input with missing padding", () => {
    const encoded = encodeStandard("ab"); // "YWI="
    expect(decodeBase64Utf8(encoded.replace(/=+$/, ""))).toBe("ab");
  });

  it("trims surrounding whitespace and newlines", () => {
    expect(decodeBase64Utf8(`\n  ${encodeStandard("trim me")}  \n`)).toBe(
      "trim me",
    );
  });

  it("decodes URL-safe base64 (same variants the Rust deeplink parser accepts)", () => {
    // ">>>???~~~" yields both "+" and "/" in standard base64
    const text = '{"k":">>>???~~~"}';
    const urlSafe = encodeStandard(text)
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=+$/, "");
    expect(urlSafe).toMatch(/[-_]/);
    expect(decodeBase64Utf8(urlSafe)).toBe(text);
  });

  it("returns empty string for empty input", () => {
    expect(decodeBase64Utf8("")).toBe("");
  });

  it("returns the original string when it is not base64 at all", () => {
    expect(decodeBase64Utf8("not base64!!")).toBe("not base64!!");
  });
});
