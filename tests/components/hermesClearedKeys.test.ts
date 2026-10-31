import { describe, expect, it } from "vitest";
import { withClearedHermesKeys } from "@/components/providers/forms/hooks/useHermesFormState";

describe("withClearedHermesKeys (CFG-18)", () => {
  it("sends null for keys the user removed so the backend deletes them", () => {
    const initial = {
      base_url: "https://a.example/v1",
      api_key: "sk",
      models: [{ id: "m1" }],
      rate_limit_delay: 2,
      headers: { "X-Org": "t" },
    };
    const edited = JSON.stringify({
      base_url: "https://a.example/v1",
      api_key: "sk",
    });
    const out = JSON.parse(withClearedHermesKeys(initial, edited));
    expect(out).toEqual({
      base_url: "https://a.example/v1",
      api_key: "sk",
      models: null,
      rate_limit_delay: null,
      headers: null,
    });
  });

  it("leaves text unchanged when nothing was removed or it is not an object", () => {
    const text = '{\n  "base_url": "x"\n}';
    expect(withClearedHermesKeys({ base_url: "y" }, text)).toBe(text);
    expect(withClearedHermesKeys({ base_url: "y" }, "not json")).toBe(
      "not json",
    );
    expect(withClearedHermesKeys(undefined, "{}")).toBe("{}");
  });

  it("keeps explicit nulls already in the draft", () => {
    const out = JSON.parse(
      withClearedHermesKeys({ headers: null, api_key: "k" }, '{"api_key":"k"}'),
    );
    expect(out).toEqual({ api_key: "k", headers: null });
  });
});
