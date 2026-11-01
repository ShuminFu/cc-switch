// @vitest-environment jsdom
// (uses localStorage / the DOM; the rest of this folder runs under node, TEST-24)
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import { useCodexConfigState } from "@/components/providers/forms/hooks/useCodexConfigState";
import { useCodexTomlValidation } from "@/components/providers/forms/hooks/useCodexTomlValidation";
import {
  setCodexBaseUrl,
  setCodexModelName,
} from "@/utils/providerConfigUtils";
import { normalizeTomlText, parseTomlLenient } from "@/utils/textNormalization";

const PROVIDER = `model_provider = "relay"
model = "gpt-5"

[model_providers.relay]
name = "relay"
base_url = "https://a.example.com/v1"
wire_api = "responses"
`;

// FE-ARCH-37: quotes inside TOML strings and comments are user content.
describe("TOML smart-quote handling", () => {
  const literal = `x = 'Don’t'\n`;
  const nested = `x = "“hi”"\n# he said “hello”\n`;

  it("leaves quotes inside ASCII strings and comments byte-identical", () => {
    expect(normalizeTomlText(literal)).toBe(literal);
    expect(normalizeTomlText(nested)).toBe(nested);
    expect(normalizeTomlText(`note = """他说“你好”"""`)).toBe(
      `note = """他说“你好”"""`,
    );
    expect(parseToml(nested)).toEqual({ x: "“hi”" });
  });

  it("replaces smart quotes used as delimiters", () => {
    expect(normalizeTomlText(`x = “hi”`)).toBe(`x = "hi"`);
    expect(normalizeTomlText(`x = ‘it’`)).toBe(`x = 'it'`);
    expect(parseTomlLenient(`x = “hi”`)).toEqual({ x: "hi" });
  });

  it("setters keep string contents untouched", () => {
    const config = `${literal}${PROVIDER}`;
    const withUrl = setCodexBaseUrl(config, "https://b.example.com/v1");
    expect(withUrl).toContain(`x = 'Don’t'`);
    expect(withUrl).toContain(`base_url = "https://b.example.com/v1"`);
    const withModel = setCodexModelName(`${nested}${PROVIDER}`, "gpt-6");
    expect(withModel).toContain(`x = "“hi”"`);
    expect(withModel).toContain(`# he said “hello”`);
    expect(() => parseToml(withModel)).not.toThrow();
  });

  it("handleCodexConfigChange stores the text as typed", () => {
    const initialData = { settingsConfig: { auth: {}, config: "" } };
    const { result } = renderHook(() => useCodexConfigState({ initialData }));
    act(() => {
      result.current.handleCodexConfigChange(literal);
    });
    expect(result.current.codexConfig).toBe(literal);
  });

  it("offers the fix only when smart-quote delimiters break parsing", () => {
    const { result } = renderHook(() => useCodexTomlValidation());
    act(() => {
      result.current.validateToml(`x = “hi”`);
    });
    expect(result.current.configError).not.toBe("");
    expect(result.current.smartQuoteFixAvailable).toBe(true);

    act(() => {
      result.current.validateToml(literal);
    });
    expect(result.current.configError).toBe("");
    expect(result.current.smartQuoteFixAvailable).toBe(false);

    act(() => {
      result.current.validateToml(`x = [`);
    });
    expect(result.current.smartQuoteFixAvailable).toBe(false);
  });
});
