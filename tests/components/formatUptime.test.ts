import { describe, it, expect } from "vitest";
import { formatUptime } from "@/components/proxy/formatUptime";

describe("formatUptime (FE-UI-23)", () => {
  it("formats hours, minutes and seconds in English", () => {
    expect(formatUptime(3723, "en")).toBe("1h 2m 3s");
    expect(formatUptime(62, "en")).toBe("1m 2s");
    expect(formatUptime(5, "en")).toBe("5s");
  });

  it("uses the app locale instead of English suffixes", () => {
    const zh = formatUptime(3723, "zh-CN");
    expect(zh).not.toContain("h");
    expect(zh).toContain("1");
  });

  it("clamps negative or fractional input", () => {
    expect(formatUptime(-4, "en")).toBe("0s");
    expect(formatUptime(1.9, "en")).toBe("1s");
  });
});
