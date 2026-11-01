import { describe, expect, it } from "vitest";
import {
  containsUnsafeKeys,
  hasCommonConfigSnippet,
  updateCommonConfigSnippet,
  validateJsonConfig,
} from "@/utils/providerConfigUtils";

// FE-ARCH-40: a snippet must never be able to write onto Object.prototype.
describe("common config snippet prototype safety", () => {
  it("does not pollute Object.prototype through __proto__", () => {
    const result = updateCommonConfigSnippet(
      "{}",
      '{"__proto__":{"polluted":1}}',
      true,
    );
    expect(({} as Record<string, unknown>).polluted).toBeUndefined();
    expect(result.error).toBeTruthy();
  });

  it("ignores constructor/prototype keys nested in a snippet", () => {
    const snippet = '{"env":{"constructor":{"prototype":{"polluted":2}}}}';
    updateCommonConfigSnippet('{"env":{}}', snippet, true);
    expect(({} as Record<string, unknown>).polluted).toBeUndefined();
    expect(hasCommonConfigSnippet("{}", snippet)).toBe(false);
  });

  it("flags unsafe keys in validation", () => {
    expect(containsUnsafeKeys(JSON.parse('{"a":[{"__proto__":{}}]}'))).toBe(
      true,
    );
    expect(containsUnsafeKeys({ a: { b: 1 } })).toBe(false);
    expect(validateJsonConfig('{"__proto__":{"x":1}}')).not.toBe("");
  });

  it("still merges and removes normal keys", () => {
    const merged = updateCommonConfigSnippet(
      '{"env":{"A":"1"}}',
      '{"env":{"B":"2"}}',
      true,
    );
    expect(JSON.parse(merged.updatedConfig)).toEqual({
      env: { A: "1", B: "2" },
    });
    const removed = updateCommonConfigSnippet(
      merged.updatedConfig,
      '{"env":{"B":"2"}}',
      false,
    );
    expect(JSON.parse(removed.updatedConfig)).toEqual({ env: { A: "1" } });
  });
});
